use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

pub const FORMATS: [(&str, &str, &str); 6] = [
    ("MP3", "mp3", "Works with most players and editors."),
    ("WAV", "wav", "Uncompressed audio for editing."),
    ("M4A", "m4a", "Compact audio for Apple devices."),
    ("FLAC", "flac", "Lossless compression."),
    ("Opus", "opus", "Small files with good sound quality."),
    ("AAC", "aac", "Efficient audio for everyday listening."),
];

pub struct Links {
    pub urls: Vec<String>,
    pub invalid: usize,
    pub duplicates: usize,
}

pub fn parse_links(text: &str) -> Links {
    let mut result = Links {
        urls: Vec::new(),
        invalid: 0,
        duplicates: 0,
    };
    let mut seen = HashSet::new();
    for part in text.split(|c: char| c.is_whitespace() || c == ',') {
        let token = part.trim_matches(['<', '>', '(', ')', '[', ']', '"', '\'']);
        if token.is_empty() {
            continue;
        }
        match video_id(token) {
            Some(id) if seen.insert(id) => result
                .urls
                .push(format!("https://www.youtube.com/watch?v={id}")),
            Some(_) => result.duplicates += 1,
            None => result.invalid += 1,
        }
    }
    result
}

fn video_id(token: &str) -> Option<&str> {
    let rest = token
        .strip_prefix("https://")
        .or_else(|| token.strip_prefix("http://"))
        .unwrap_or(token);
    let (host, path) = rest.split_once('/')?;
    let (path, query) = path
        .split('#')
        .next()?
        .split_once('?')
        .unwrap_or((path, ""));
    let id = match host.to_ascii_lowercase().as_str() {
        "youtu.be" => path,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            if path == "watch" {
                query
                    .split('&')
                    .find_map(|parameter| parameter.strip_prefix("v="))?
            } else {
                let (kind, id) = path.split_once('/')?;
                match kind {
                    "shorts" | "live" | "embed" | "v" => id,
                    _ => return None,
                }
            }
        }
        _ => return None,
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
    .then_some(id)
}

pub trait Downloader {
    /// Saves the audio of `url` in `folder` and returns the saved file.
    fn download(
        &self,
        url: &str,
        format: &str,
        folder: &Path,
        progress: &mut dyn FnMut(Update),
    ) -> Result<PathBuf, String>;
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
            if !metadata.is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            true
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
        format: &str,
        folder: &Path,
        progress: &mut dyn FnMut(Update),
    ) -> Result<PathBuf, String> {
        let downloader = self
            .downloader
            .as_ref()
            .ok_or("Install yt-dlp to start downloading.")?;
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
                format,
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
            .env("PYTHONIOENCODING", "utf-8");
        if let Some((name, path)) = &self.runtime {
            let mut runtime = OsString::from(format!("{name}:"));
            runtime.push(path);
            command.arg("--js-runtimes").arg(runtime);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command
            .arg("--")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("Could not start yt-dlp. {err}"))?;
        let stderr = child.stderr.take().expect("stderr is piped");
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
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut output = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            match parse_output(&line) {
                Some(Output::Update(update)) => progress(update),
                Some(Output::Saved(file)) => output = Some(file),
                None => {}
            }
        }
        let status = child.wait().map_err(|err| err.to_string())?;
        let diagnostic = errors.join().ok().flatten();
        if !status.success() {
            return Err(diagnostic.unwrap_or_else(|| format!("The download ended with {status}.")));
        }
        output.filter(|file| file.is_file()).ok_or_else(|| {
            "The downloader finished without reporting a saved audio file.".to_owned()
        })
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

pub fn open_folder(folder: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let mut child = Command::new(program)
        .arg(folder)
        .spawn()
        .map_err(|err| format!("Could not open the folder. {err}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dQw4w9WgXcQ";

    #[test]
    fn video_id_accepts_every_supported_link() {
        for link in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "http://youtube.com/watch?v=dQw4w9WgXcQ",
            "youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=abc",
            "https://WWW.YouTube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?list=PL123&v=dQw4w9WgXcQ&t=5s",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ#comments",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=10",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ?feature=share",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/v/dQw4w9WgXcQ",
        ] {
            assert_eq!(video_id(link), Some(ID), "{link}");
        }
    }

    #[test]
    fn video_id_rejects_other_links() {
        for link in [
            "",
            "dQw4w9WgXcQ",
            "https://www.youtube.com",
            "https://www.youtube.com/",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/watch?list=PL123",
            "https://www.youtube.com/playlist?list=PL123",
            "https://www.youtube.com/channel/dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=tooshort",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQQ",
            "https://youtu.be/dQw4w9WgXc!",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.example.com/watch?v=dQw4w9WgXcQ",
            "ftp://youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(video_id(link), None, "{link}");
        }
    }

    #[test]
    fn parse_links_of_empty_text_finds_nothing() {
        for text in ["", " \n\t , ", "<>"] {
            let links = parse_links(text);
            assert!(links.urls.is_empty());
            assert_eq!((links.invalid, links.duplicates), (0, 0));
        }
    }

    #[test]
    fn parse_links_splits_and_normalizes_in_order() {
        let links = parse_links(
            "https://youtu.be/aaaaaaaaaaa, <https://youtu.be/bbbbbbbbbbb>\n\"youtube.com/shorts/ccccccccccc\"\t(https://youtu.be/d-d_ddddddd)",
        );
        assert_eq!(
            links.urls,
            [
                "https://www.youtube.com/watch?v=aaaaaaaaaaa",
                "https://www.youtube.com/watch?v=bbbbbbbbbbb",
                "https://www.youtube.com/watch?v=ccccccccccc",
                "https://www.youtube.com/watch?v=d-d_ddddddd",
            ]
        );
        assert_eq!((links.invalid, links.duplicates), (0, 0));
    }

    #[test]
    fn parse_links_counts_invalid_and_duplicate_links() {
        let links = parse_links(
            "https://youtu.be/dQw4w9WgXcQ https://www.youtube.com/watch?v=dQw4w9WgXcQ hello https://example.com/",
        );
        assert_eq!(links.urls, ["https://www.youtube.com/watch?v=dQw4w9WgXcQ"]);
        assert_eq!((links.invalid, links.duplicates), (2, 1));
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
}
