use eframe::egui::{self, Button, Layout, RichText, Stroke, Ui, Vec2};

use crate::app::App;
use crate::theme::{self, Palette};

impl App {
    pub(super) fn actions(&mut self, ui: &mut Ui, palette: &Palette) {
        let busy = self.queue.is_running();
        let height = 36.0;
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), height),
            Layout::right_to_left(egui::Align::Center),
            |ui| {
                let label = if busy {
                    "Downloading…".to_owned()
                } else if self.links.urls.is_empty() {
                    "Download audio".to_owned()
                } else {
                    let count = self.links.urls.len();
                    format!(
                        "Download {count} {}",
                        if count == 1 { "file" } else { "files" }
                    )
                };
                let button = Button::new(theme::medium(label).color(palette.primary_text))
                    .fill(palette.primary)
                    .stroke(Stroke::NONE)
                    .min_size(Vec2::new(156.0, height))
                    .corner_radius(6);
                if ui
                    .add_enabled(
                        !busy && !self.links.urls.is_empty() && self.links.invalid == 0,
                        button,
                    )
                    .clicked()
                {
                    self.start(ui.ctx());
                }
                if busy {
                    let cancelling = self.queue.is_cancelling();
                    let label = if cancelling {
                        "Cancelling…"
                    } else {
                        "Cancel"
                    };
                    let button = Button::new(theme::medium(label))
                        .min_size(Vec2::new(96.0, height))
                        .corner_radius(6);
                    if ui.add_enabled(!cancelling, button).clicked() {
                        self.queue.cancel();
                    }
                }
                ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                    if !self.missing.is_empty() && ui.button("Set up downloads…").clicked() {
                        self.show_setup = true;
                    }
                });
            },
        );
        if self.links.invalid > 0 {
            ui.label(RichText::new("Remove invalid links to start.").color(palette.error));
        }
        if let Some(notice) = &self.notice {
            ui.label(RichText::new(notice).color(palette.error));
        }
    }
}
