use std::io;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

/// Stops a batch and the process tree it is running. Clones share the same state.
#[derive(Clone, Default)]
pub struct Cancel(Arc<Shared>);

#[derive(Default)]
struct Shared {
    requested: AtomicBool,
    /// The running process. It is reaped only while this is locked, so its ID stays valid here.
    child: Mutex<Option<Child>>,
}

impl Cancel {
    pub fn is_requested(&self) -> bool {
        self.0.requested.load(Ordering::SeqCst)
    }

    /// Stops the running process tree on another thread, so the caller does not wait for it.
    pub fn request(&self) {
        if self.0.requested.swap(true, Ordering::SeqCst) {
            return;
        }
        let cancel = self.clone();
        thread::spawn(move || {
            if let Some(child) = cancel.child().as_mut() {
                kill_tree(child);
            }
        });
    }

    /// Starts `command` as the root of a process tree that `request` can stop.
    /// Call `wait` after reading the returned output.
    pub fn spawn(&self, command: &mut Command) -> io::Result<(ChildStdout, ChildStderr)> {
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
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let mut slot = self.child();
        // A request made before the lock was taken found no process to stop.
        if self.is_requested() {
            kill_tree(&mut child);
        }
        *slot = Some(child);
        Ok((stdout, stderr))
    }

    /// Waits for the process from `spawn` without holding the lock that `request` needs.
    pub fn wait(&self) -> io::Result<ExitStatus> {
        loop {
            let mut slot = self.child();
            let child = slot.as_mut().expect("spawn was called");
            match child.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    *slot = None;
                    return Ok(status);
                }
                Err(error) => {
                    *slot = None;
                    return Err(error);
                }
            }
            drop(slot);
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn child(&self) -> MutexGuard<'_, Option<Child>> {
        self.0.child.lock().unwrap_or_else(PoisonError::into_inner)
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
