use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use eframe::egui::{Context, ThemePreference};

use crate::downloader::Tools;
use crate::format::AudioFormat;
use crate::links::{self, Links};
use crate::queue::Queue;
use crate::screenshot::Screenshot;

/// The state of the app and what it can do. The interface in `ui` reads and changes it.
pub struct App {
    pub appearance: ThemePreference,
    pub text: String,
    pub links: Links,
    pub format: AudioFormat,
    pub folder: PathBuf,
    pub missing: Vec<&'static str>,
    pub queue: Queue,
    pub output_folder: Option<PathBuf>,
    pub notice: Option<String>,
    pub show_setup: bool,
    pub screenshot: Option<Screenshot>,
    pub capture_result: mpsc::SyncSender<Result<(), String>>,
}

impl App {
    pub fn new(
        screenshot: Option<Screenshot>,
        capture_result: mpsc::SyncSender<Result<(), String>>,
    ) -> Self {
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
        let folder = home
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Downloads")
            .join("Batch Audio");
        Self {
            appearance: ThemePreference::System,
            text: String::new(),
            links: links::parse_links(""),
            format: AudioFormat::default(),
            folder,
            missing: Tools::detect().missing(),
            queue: Queue::default(),
            output_folder: None,
            notice: None,
            show_setup: false,
            screenshot,
            capture_result,
        }
    }

    pub fn start(&mut self, ctx: &Context) {
        let tools = Tools::detect();
        self.missing = tools.missing();
        if !self.missing.is_empty() {
            self.show_setup = true;
            return;
        }
        if let Err(error) = fs::create_dir_all(&self.folder) {
            self.notice = Some(format!("Could not create the output folder. {error}"));
            return;
        }
        self.notice = None;
        self.output_folder = Some(self.folder.clone());
        let ctx = ctx.clone();
        self.queue.start(
            tools,
            self.links.urls.clone(),
            self.format,
            self.folder.clone(),
            move || ctx.request_repaint(),
        );
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
