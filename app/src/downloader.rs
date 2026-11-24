use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::Duration;

use crate::cancel::Cancel;
use crate::format::AudioFormat;
use crate::links::video_id;

#[derive(Debug)]
pub enum DownloadError {
    MissingDownloader,
    Start(io::Error),
    Wait(io::Error),
    /// The last error or warning that yt-dlp printed.
    Reported(String),
    /// yt-dlp failed without printing an error or warning.
    Exit(ExitStatus),
    NoSavedFile,
    Cancelled,
    /// The download crashed, or its worker thread ended before the job finished.
    WorkerStopped,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDownloader => f.write_str("Install yt-dlp to start downloading."),
            Self::Start(error) => write!(f, "Could not start yt-dlp. {error}"),
            Self::Wait(error) => write!(f, "{error}"),
            Self::Reported(message) => f.write_str(message),
            Self::Exit(status) => write!(f, "The download ended with {status}."),
            Self::NoSavedFile => {
                f.write_str("The downloader finished without reporting a saved audio file.")
            }
            Self::Cancelled => f.write_str("The download was cancelled."),
            Self::WorkerStopped => f.write_str("The download worker stopped unexpectedly."),
        }
    }
}

impl std::error::Error for DownloadError {}

pub trait Downloader {
    /// Saves the audio of `url` in `folder` and returns the saved file.
    /// Returns `DownloadError::Cancelled` soon after `cancel` is requested.
    fn download(
        &self,
        url: &str,
        format: AudioFormat,
        folder: &Path,
        cancel: &Cancel,
        progress: &mut dyn FnMut(Update),
    ) -> Result<PathBuf, DownloadError>;
}

pub struct Tools {
    path: OsString,
    downloader: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
    runtime: Option<(&'static str, PathBuf)>,
}

fn executable(directories: &[PathBuf], name: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    directories
        .iter()
        .map(|folder| folder.join(&name))
        .find(|file| {
            let Ok(metadata) = file.metadata() else {
                return false;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            metadata.is_file()
        })
}

impl Tools {
    pub fn detect() -> Self {
        let mut directories = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default();
        directories.retain(|path| path.is_absolute());
        if let Ok(current) = std::env::current_exe() {
            if let Some(folder) = current.parent() {
                directories.insert(0, folder.to_owned());
            }
        }
        #[cfg(unix)]
        {
            directories.extend(
                [
                    "/opt/homebrew/bin",
                    "/usr/local/bin",
                    "/opt/local/bin",
                    "/usr/bin",
                ]
                .map(PathBuf::from),
            );
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                directories
                    .extend([".local/bin", ".deno/bin", ".bun/bin"].map(|sub| home.join(sub)));
            }
        }
        let runtime = ["deno", "node", "bun"]
            .into_iter()
            .find_map(|name| executable(&directories, name).map(|path| (name, path)));
        Self {
            path: std::env::join_paths(&directories).unwrap_or_default(),
            downloader: executable(&directories, "yt-dlp"),
            ffmpeg: executable(&directories, "ffmpeg"),
            ffprobe: executable(&directories, "ffprobe"),
            runtime,
        }
    }

    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.downloader.is_none() {
            missing.push("yt-dlp");
        }
        if self.ffmpeg.is_none() || self.ffprobe.is_none() {
            missing.push("FFmpeg");
        }
        if self.runtime.is_none() {
            missing.push("Deno");
        }
        missing
    }
}

impl Downloader for Tools {
    fn download(
        &self,
        url: &str,
        format: AudioFormat,
        folder: &Path,
        cancel: &Cancel,
        progress: &mut dyn FnMut(Update),
    ) -> Result<PathBuf, DownloadError> {
        let downloader = self
            .downloader
            .as_ref()
            .ok_or(DownloadError::MissingDownloader)?;
        // yt-dlp converts straight into the final file name, and treats a file with that name as
        // already downloaded. Unfinished files stay in this folder, so a cancelled conversion
        // cannot pass for a saved one.
        let partial = format!(".batch-audio-{}", video_id(url).unwrap_or("download"));
        let mut command = Command::new(downloader);
        command
            .env("PATH", &self.path)
            .args([
                "--ignore-config",
                "--no-playlist",
                "--no-overwrites",
                "--no-colors",
                "--no-simulate",
                "--newline",
                "--progress",
                "--socket-timeout",
                "20",
                "--retries",
                "3",
                "--fragment-retries",
                "3",
                "--format",
                "bestaudio/best",
                "--extract-audio",
                "--audio-format",
                format.yt_dlp_name(),
                "--progress-template",
                "download:bulk-progress:%(progress._percent_str)s",
                "--print",
                "before_dl:bulk-title:%(title)s",
                "--print",
                "after_move:bulk-file:%(filepath)s",
                "--output",
                "%(title).140B [%(id)s].%(ext)s",
                "--paths",
            ])
            .arg(folder)
            .arg("--paths")
            .arg(format!("temp:{partial}"))
            .env("PYTHONIOENCODING", "utf-8");
        if let Some((name, path)) = &self.runtime {
            let mut runtime = OsString::from(format!("{name}:"));
            runtime.push(path);
            command.arg("--js-runtimes").arg(runtime);
        }
        command.arg("--").arg(url);
        let result = run(&mut command, cancel, progress);
        let partial = folder.join(partial);
        if matches!(result, Ok(_) | Err(DownloadError::Cancelled)) {
            remove_partial(&partial);
        } else {
            // Removes the folder only if it is empty, so a failed download can resume.
            let _ = fs::remove_dir(&partial);
        }
        result
    }
}

/// Deletes the temporary files of a saved or cancelled download.
fn remove_partial(folder: &Path) {
    // Windows can keep a file locked for a moment after its process stops.
    for _ in 0..20 {
        match fs::remove_dir_all(folder) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                thread::sleep(Duration::from_millis(50));
            }
            _ => return,
        }
    }
}

/// Runs yt-dlp and returns the file it saved.
fn run(
    command: &mut Command,
    cancel: &Cancel,
    progress: &mut dyn FnMut(Update),
) -> Result<PathBuf, DownloadError> {
    if cancel.is_requested() {
        return Err(DownloadError::Cancelled);
    }
    let (running, stdout, stderr) = cancel.spawn(command).map_err(DownloadError::Start)?;
    let errors = thread::spawn(move || {
        let mut last_error = None;
        let mut last_warning = None;
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            match parse_diagnostic(&line) {
                Some(Diagnostic::Error(error)) => last_error = Some(error),
                Some(Diagnostic::Warning(warning)) => last_warning = Some(warning),
                None => {}
            }
        }
        last_error.or(last_warning)
    });
    let mut output = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        match parse_output(&line) {
            Some(Output::Update(update)) => progress(update),
            Some(Output::Saved(file)) => output = Some(file),
            None => {}
        }
    }
    let status = running.wait().map_err(DownloadError::Wait)?;
    let diagnostic = errors.join().ok().flatten();
    let saved = output.filter(|file| file.is_file());
    if status.success() {
        saved.ok_or(DownloadError::NoSavedFile)
    } else if cancel.is_requested() {
        // yt-dlp reports a file only after moving it, so a reported file is complete.
        saved.ok_or(DownloadError::Cancelled)
    } else {
        Err(diagnostic.map_or(DownloadError::Exit(status), DownloadError::Reported))
    }
}

#[derive(Debug, PartialEq)]
pub enum Update {
    Title(String),
    Progress(f32),
}

#[derive(Debug, PartialEq)]
enum Output {
    Update(Update),
    Saved(PathBuf),
}

/// Reads one line of yt-dlp standard output. The prefixes match the templates in `download`.
fn parse_output(line: &str) -> Option<Output> {
    if let Some(title) = line.strip_prefix("bulk-title:") {
        Some(Output::Update(Update::Title(title.to_owned())))
    } else if let Some(path) = line.strip_prefix("bulk-file:") {
        Some(Output::Saved(PathBuf::from(path)))
    } else {
        let percent = line.strip_prefix("bulk-progress:")?;
        let value: f32 = percent.trim().trim_end_matches('%').parse().ok()?;
        value
            .is_finite()
            .then(|| Output::Update(Update::Progress((value / 100.0).clamp(0.0, 1.0))))
    }
}

#[derive(Debug, PartialEq)]
enum Diagnostic {
    Error(String),
    Warning(String),
}

/// Reads one line of yt-dlp standard error.
fn parse_diagnostic(line: &str) -> Option<Diagnostic> {
    if let Some(error) = line.strip_prefix("ERROR: ") {
        Some(Diagnostic::Error(error.to_owned()))
    } else {
        line.strip_prefix("WARNING: ")
            .map(|warning| Diagnostic::Warning(warning.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;

    #[test]
    fn download_errors_keep_their_text() {
        let status = ExitStatus::default();
        for (error, expected) in [
            (
                DownloadError::MissingDownloader,
                "Install yt-dlp to start downloading.".to_owned(),
            ),
            (
                DownloadError::Start(io::Error::other("not allowed")),
                "Could not start yt-dlp. not allowed".to_owned(),
            ),
            (
                DownloadError::Wait(io::Error::other("no child")),
                "no child".to_owned(),
            ),
            (
                DownloadError::Reported("Video unavailable".to_owned()),
                "Video unavailable".to_owned(),
            ),
            (
                DownloadError::Exit(status),
                format!("The download ended with {status}."),
            ),
            (
                DownloadError::NoSavedFile,
                "The downloader finished without reporting a saved audio file.".to_owned(),
            ),
            (
                DownloadError::Cancelled,
                "The download was cancelled.".to_owned(),
            ),
            (
                DownloadError::WorkerStopped,
                "The download worker stopped unexpectedly.".to_owned(),
            ),
        ] {
            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn parse_output_reads_title_and_file() {
        assert_eq!(
            parse_output("bulk-title:A: title, 100%"),
            Some(Output::Update(Update::Title("A: title, 100%".to_owned())))
        );
        assert_eq!(
            parse_output("bulk-file:/tmp/My song [dQw4w9WgXcQ].mp3"),
            Some(Output::Saved(PathBuf::from(
                "/tmp/My song [dQw4w9WgXcQ].mp3"
            )))
        );
    }

    #[test]
    fn parse_output_reads_and_clamps_progress() {
        for (line, expected) in [
            ("bulk-progress:  0.0%", 0.0),
            ("bulk-progress: 50.0%", 0.5),
            ("bulk-progress:100.0%", 1.0),
            ("bulk-progress:150%", 1.0),
            ("bulk-progress:-5%", 0.0),
        ] {
            assert_eq!(
                parse_output(line),
                Some(Output::Update(Update::Progress(expected))),
                "{line}"
            );
        }
    }

    #[test]
    fn parse_output_ignores_other_lines() {
        for line in [
            "",
            "[youtube] Extracting URL",
            "bulk-progress:N/A",
            "bulk-progress:NaN%",
            "bulk-progress:inf%",
            " bulk-title:indented",
        ] {
            assert_eq!(parse_output(line), None, "{line}");
        }
    }

    #[test]
    fn parse_diagnostic_separates_errors_and_warnings() {
        assert_eq!(
            parse_diagnostic("ERROR: Video unavailable"),
            Some(Diagnostic::Error("Video unavailable".to_owned()))
        );
        assert_eq!(
            parse_diagnostic("WARNING: Slow connection"),
            Some(Diagnostic::Warning("Slow connection".to_owned()))
        );
        assert_eq!(parse_diagnostic("[debug] ERROR: nested"), None);
    }

    const FAKE: &str = "BULK_YT_FAKE_DOWNLOADER";

    /// Not a test. `fake_downloader` runs this binary again to act as yt-dlp and its child.
    #[test]
    #[ignore = "runs only as a child process of other tests"]
    fn act_as_fake_downloader() {
        let Ok(setting) = std::env::var(FAKE) else {
            return;
        };
        let (role, address) = setting.split_once(' ').unwrap();
        let _connection = TcpStream::connect(address).unwrap();
        if role == "parent" {
            let mut child = fake_downloader("child", address).spawn().unwrap();
            println!("\nbulk-progress:50%");
            child.wait().unwrap();
            return;
        }
        thread::sleep(Duration::from_secs(60));
    }

    fn fake_downloader(role: &str, address: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "downloader::tests::act_as_fake_downloader"])
            .args(["--ignored", "--nocapture"])
            .env(FAKE, format!("{role} {address}"));
        command
    }

    /// Waits for a process to connect. The connection closes when that process ends.
    fn accept(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            match listener.accept() {
                Ok((connection, _)) => {
                    connection.set_nonblocking(false).unwrap();
                    return connection;
                }
                Err(error) => {
                    assert!(Instant::now() < deadline, "no process connected: {error}");
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }

    fn assert_ended(mut connection: TcpStream) {
        connection
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        match connection.read(&mut [0]) {
            Ok(0) => {}
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            other => panic!("the process is still running: {other:?}"),
        }
    }

    #[test]
    fn cancel_stops_the_downloader_and_its_child() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let cancel = Cancel::default();
        let worker = thread::spawn({
            let cancel = cancel.clone();
            move || {
                let mut updates = Vec::new();
                let result = run(
                    &mut fake_downloader("parent", &address),
                    &cancel,
                    &mut |update| updates.push(update),
                );
                (result, updates)
            }
        });
        let connections = [accept(&listener), accept(&listener)];
        // Both processes are running and write nothing more, so only the kill can end `run`.
        thread::sleep(Duration::from_millis(200));
        cancel.request();

        for connection in connections {
            assert_ended(connection);
        }
        let (result, updates) = worker.join().unwrap();
        assert!(
            matches!(result, Err(DownloadError::Cancelled)),
            "{result:?}"
        );
        assert_eq!(updates, [Update::Progress(0.5)]);
    }

    #[test]
    fn cancel_before_the_start_runs_nothing() {
        let cancel = Cancel::default();
        cancel.request();
        let mut command = Command::new("this-program-does-not-exist");
        let result = run(&mut command, &cancel, &mut |_| {});
        assert!(
            matches!(result, Err(DownloadError::Cancelled)),
            "{result:?}"
        );
    }

    #[test]
    fn cancel_between_the_check_and_the_start_still_stops_the_process() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let cancel = Cancel::default();
        cancel.request();
        // Time for the request to finish, so that it cannot find the process.
        thread::sleep(Duration::from_millis(200));
        // `spawn` is what `run` calls after its own check.
        let (running, _stdout, _stderr) = cancel
            .spawn(&mut fake_downloader("child", &address))
            .unwrap();
        assert!(!running.wait().unwrap().success());
    }

    #[test]
    fn cancel_stops_every_running_downloader() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let cancel = Cancel::default();
        let workers: Vec<_> = (0..3)
            .map(|_| {
                let (cancel, address) = (cancel.clone(), address.clone());
                thread::spawn(move || {
                    run(
                        &mut fake_downloader("parent", &address),
                        &cancel,
                        &mut |_| {},
                    )
                })
            })
            .collect();
        // Each downloader has a child.
        let connections: Vec<_> = (0..6).map(|_| accept(&listener)).collect();
        cancel.request();

        for connection in connections {
            assert_ended(connection);
        }
        for worker in workers {
            let result = worker.join().unwrap();
            assert!(
                matches!(result, Err(DownloadError::Cancelled)),
                "{result:?}"
            );
        }
    }

    #[test]
    fn crash_stops_its_own_downloader_and_no_other() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let cancel = Cancel::default();
        let (other, _stdout, _stderr) = cancel
            .spawn(&mut fake_downloader("child", &address))
            .unwrap();
        let mut other_connection = accept(&listener);

        let (crash, blocked) = mpsc::channel::<()>();
        let worker = thread::spawn({
            let cancel = cancel.clone();
            move || {
                run(
                    &mut fake_downloader("parent", &address),
                    &cancel,
                    &mut |_| {
                        let _ = blocked.recv();
                        panic!("expected by this test");
                    },
                )
            }
        });
        let connections = [accept(&listener), accept(&listener)];
        // Both processes are running, so only the crash can end them.
        crash.send(()).unwrap();
        assert!(worker.join().is_err());
        for connection in connections {
            assert_ended(connection);
        }
        other_connection
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let error = other_connection.read(&mut [0]).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ),
            "{error}"
        );

        cancel.request();
        assert!(!other.wait().unwrap().success());
    }

    #[test]
    fn remove_partial_deletes_files_and_accepts_a_missing_folder() {
        let folder =
            std::env::temp_dir().join(format!("bulk-yt-partial-test-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("audio.wav"), "partial").unwrap();
        remove_partial(&folder);
        assert!(!folder.exists());
        remove_partial(&folder);
    }
}
