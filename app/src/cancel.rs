use std::collections::HashMap;
use std::io;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

/// Stops a batch and the process trees it is running. Clones share the same state.
#[derive(Clone, Default)]
pub struct Cancel(Arc<Shared>);

#[derive(Default)]
struct Shared {
    requested: AtomicBool,
    /// The running processes. Each one is reaped only while this is locked or after it leaves
    /// the map, so the ID of every process in the map stays valid.
    children: Mutex<HashMap<u64, Child>>,
    /// The next key in `children`. Process IDs are not keys, because the system reuses them.
    next_key: AtomicU64,
}

/// A process that `Cancel::spawn` started. Dropping it without calling `wait` stops its tree.
pub struct Running {
    cancel: Cancel,
    key: u64,
}

impl Cancel {
    pub fn is_requested(&self) -> bool {
        self.0.requested.load(Ordering::SeqCst)
    }

    /// Stops every running process tree on another thread, so the caller does not wait for it.
    pub fn request(&self) {
        if self.0.requested.swap(true, Ordering::SeqCst) {
            return;
        }
        let cancel = self.clone();
        thread::spawn(move || {
            for child in cancel.children().values_mut() {
                kill_tree(child);
            }
        });
    }

    /// Starts `command` as the root of a process tree that `request` can stop.
    /// Call `Running::wait` after reading the returned output.
    pub fn spawn(&self, command: &mut Command) -> io::Result<(Running, ChildStdout, ChildStderr)> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        hide_console(command);
        // Locked before the start. macOS cannot create a pipe that is closed on exec in one step,
        // so a process that starts at the same time could inherit this one's pipes and keep its
        // output open.
        let mut children = self.children();
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        // A request made before the lock was taken did not find this process.
        if self.is_requested() {
            kill_tree(&mut child);
        }
        let key = self.0.next_key.fetch_add(1, Ordering::SeqCst);
        children.insert(key, child);
        let cancel = self.clone();
        Ok((Running { cancel, key }, stdout, stderr))
    }

    fn children(&self) -> MutexGuard<'_, HashMap<u64, Child>> {
        self.0
            .children
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Running {
    /// Waits for the process without holding the lock that `Cancel::request` needs.
    pub fn wait(self) -> io::Result<ExitStatus> {
        loop {
            let mut children = self.cancel.children();
            let child = children
                .get_mut(&self.key)
                .expect("only wait and drop remove it");
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    children.remove(&self.key);
                    return Ok(status);
                }
                Err(error) => {
                    children.remove(&self.key);
                    return Err(error);
                }
            }
            drop(children);
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let child = self.cancel.children().remove(&self.key);
        if let Some(mut child) = child {
            kill_tree(&mut child);
            let _ = child.wait();
        }
    }
}

#[cfg(unix)]
fn kill_tree(child: &mut Child) {
    let Ok(group) = libc::pid_t::try_from(child.id()) else {
        return;
    };
    // SAFETY: `kill` takes plain integers. The process has not been reaped, so its ID, which is
    // also its group ID, cannot belong to another process.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn kill_tree(child: &mut Child) {
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let mut command = Command::new(
        std::path::Path::new(&system)
            .join("System32")
            .join("taskkill.exe"),
    );
    command
        .args(["/T", "/F", "/PID", &child.id().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console(&mut command);
    let stopped = command.status().is_ok_and(|status| status.success());
    if !stopped {
        let _ = child.kill();
    }
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
}
