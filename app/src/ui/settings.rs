use eframe::egui::{self, Button, Layout, RichText, Ui};

use super::widgets::card;
use crate::app::App;
use crate::format::AudioFormat;
use crate::theme::{self, Palette};

impl App {
    pub(super) fn settings_panel(&mut self, ui: &mut Ui, width: f32, palette: &Palette) {
        let idle = !self.queue.is_running();
        card(ui, width, palette, |ui| {
            ui.set_min_height(217.0);
            ui.label(theme::heading("Audio format", 19.0));
            ui.label(
                RichText::new(self.format.description())
                    .size(13.0)
                    .color(palette.muted),
            );
            ui.separator();
            ui.add_enabled_ui(idle, |ui| {
                let button_width = (ui.available_width() - ui.spacing().item_spacing.x * 2.0) / 3.0;
                for formats in AudioFormat::ALL.chunks(3) {
                    ui.horizontal(|ui| {
                        for &format in formats {
                            let selected = self.format == format;
                            let text = if selected {
                                theme::medium(format.label()).color(palette.accent)
                            } else {
                                RichText::new(format.label())
                            };
                            if ui
                                .add_sized(
                                    [button_width, 30.0],
                                    Button::new(text).selected(selected).corner_radius(6),
                                )
                                .clicked()
                            {
                                self.format = format;
                                self.notice = None;
                            }
                        }
                    });
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(theme::medium("Save to"));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(idle, Button::new("Choose folder…"))
                        .clicked()
                    {
                        let dialog = rfd::FileDialog::new().set_title("Choose where to save audio");
                        let start = self.folder.ancestors().find(|path| path.is_dir());
                        let dialog = if let Some(path) = start {
                            dialog.set_directory(path)
                        } else {
                            dialog
                        };
                        if let Some(folder) = dialog.pick_folder() {
                            self.folder = folder;
                            self.notice = None;
                        }
                    }
                });
            });
            let path = self.folder.display().to_string();
            ui.add(
                egui::Label::new(
                    RichText::new(&path)
                        .monospace()
                        .size(12.0)
                        .color(palette.muted),
                )
                .truncate(),
            )
            .on_hover_text(path);
        });
    }
}
