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

    pub fn download(
        &self,
        url: &str,
        format: &str,
        folder: &Path,
        mut progress: impl FnMut(Update),
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
                if let Some(error) = line.strip_prefix("ERROR: ") {
                    last_error = Some(error.to_owned());
                } else if let Some(warning) = line.strip_prefix("WARNING: ") {
                    last_warning = Some(warning.to_owned());
                }
            }
            last_error.or(last_warning)
        });
        let stdout = child.stdout.take().expect("stdout is piped");
        let mut output = None;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(title) = line.strip_prefix("bulk-title:") {
                progress(Update::Title(title.to_owned()));
            } else if let Some(path) = line.strip_prefix("bulk-file:") {
                output = Some(PathBuf::from(path));
            } else if let Some(percent) = line.strip_prefix("bulk-progress:") {
                if let Ok(value) = percent.trim().trim_end_matches('%').parse::<f32>() {
                    if value.is_finite() {
                        progress(Update::Progress((value / 100.0).clamp(0.0, 1.0)));
                    }
                }
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

pub enum Update {
    Title(String),
    Progress(f32),
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
