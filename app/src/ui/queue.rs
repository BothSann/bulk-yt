use eframe::egui::{self, Layout, RichText, Ui};

use super::widgets::{card, pill, wave_mark};
use crate::app::{self, App};
use crate::queue::State;
use crate::theme::{self, Palette};

impl App {
    pub(super) fn queue_panel(&mut self, ui: &mut Ui, palette: &Palette) {
        let width = ui.available_width();
        card(ui, width, palette, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::heading("Download queue", 19.0));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(folder) = &self.output_folder {
                        if ui.button("Open output folder").clicked() {
                            self.notice = app::open_folder(folder).err();
                        }
                    }
                    let saved = self
                        .queue
                        .jobs()
                        .iter()
                        .filter(|job| matches!(job.state, State::Saved(_)))
                        .count();
                    if !self.queue.jobs().is_empty() {
                        pill(
                            ui,
                            &format!("{saved} / {} saved", self.queue.jobs().len()),
                            palette.accent_surface,
                            palette.accent,
                        );
                    }
                });
            });
            ui.separator();
            if self.queue.jobs().is_empty() {
                ui.add_space(7.0);
                ui.horizontal(|ui| {
                    wave_mark(ui, 36.0, palette.accent_surface, palette.accent);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(theme::medium("No downloads yet").size(14.0));
                        ui.label(
                            RichText::new("Start a batch to track its progress here.")
                                .color(palette.muted),
                        );
                    });
                });
            }
            for job in self.queue.jobs() {
                ui.horizontal(|ui| {
                    let (status, color) = match job.state {
                        State::Waiting => ("Waiting", palette.muted),
                        State::Downloading(_) => ("Downloading", palette.accent),
                        State::Converting => ("Converting", palette.accent),
                        State::Saved(_) => ("Saved", palette.accent),
                        State::Failed(_) => ("Failed", palette.error),
                        State::Cancelled => ("Cancelled", palette.muted),
                    };
                    ui.add_sized(
                        [92.0, 20.0],
                        egui::Label::new(theme::medium(status).color(color)),
                    );
                    let name = job.title.as_deref().unwrap_or(&job.url);
                    ui.add(egui::Label::new(name).truncate())
                        .on_hover_text(name);
                });
                match &job.state {
                    State::Converting => {
                        ui.add(
                            egui::ProgressBar::new(1.0)
                                .fill(palette.accent_surface)
                                .text("Converting audio"),
                        );
                    }
                    State::Downloading(progress) => {
                        ui.add(
                            egui::ProgressBar::new(*progress)
                                .fill(palette.accent_surface)
                                .show_percentage(),
                        );
                    }
                    State::Saved(file) => {
                        ui.label(
                            RichText::new(file.file_name().unwrap_or_default().to_string_lossy())
                                .color(palette.muted),
                        );
                    }
                    State::Failed(error) => {
                        ui.label(RichText::new(error.to_string()).color(palette.error));
                    }
                    State::Waiting | State::Cancelled => {}
                }
                ui.separator();
            }
        });
    }
}
