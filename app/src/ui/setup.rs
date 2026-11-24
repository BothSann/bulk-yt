use eframe::egui::{self, Context};

use crate::app::App;
use crate::downloader::Tools;

impl App {
    pub(super) fn setup_dialog(&mut self, ctx: &Context) {
        egui::Window::new("Set up downloads")
            .open(&mut self.show_setup)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .show(ctx, |ui| {
                if self.missing.is_empty() {
                    ui.label("Everything is ready. You can start downloading.");
                    return;
                }
                ui.label(format!(
                    "Still needed on this computer: {}.",
                    self.missing.join(", ")
                ));
                #[cfg(target_os = "macos")]
                {
                    ui.label("If you have Homebrew, run this in Terminal:");
                    ui.monospace("brew install yt-dlp ffmpeg deno");
                    ui.hyperlink_to("Get Homebrew", "https://brew.sh/");
                }
                #[cfg(not(target_os = "macos"))]
                ui.label("Install the missing tools and make them available on your system PATH.");
                ui.hyperlink_to(
                    "yt-dlp installation guide",
                    "https://github.com/yt-dlp/yt-dlp/wiki/Installation",
                );
                ui.hyperlink_to("FFmpeg downloads", "https://ffmpeg.org/download.html");
                ui.hyperlink_to(
                    "Deno installation guide",
                    "https://docs.deno.com/runtime/getting_started/installation/",
                );
                if ui.button("Check again").clicked() {
                    self.missing = Tools::detect().missing();
                }
            });
    }
}
