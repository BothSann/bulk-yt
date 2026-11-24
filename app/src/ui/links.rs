use eframe::egui::{self, Button, FontId, Frame, Layout, RichText, Stroke, TextEdit, Ui};

use super::widgets::card;
use crate::app::App;
use crate::links::parse_links;
use crate::theme::{self, Palette};

impl App {
    pub(super) fn links_panel(&mut self, ui: &mut Ui, width: f32, palette: &Palette) {
        let idle = !self.queue.is_running();
        card(ui, width, palette, |ui| {
            ui.set_min_height(217.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(theme::heading("YouTube links", 19.0));
                    ui.label(RichText::new("Paste one video link per line.").color(palette.muted));
                });
                if !self.text.is_empty() {
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(idle, Button::new("Clear")).clicked() {
                            self.text.clear();
                            self.links = parse_links("");
                        }
                    });
                }
            });
            ui.separator();
            Frame::new()
                .fill(palette.input)
                .stroke(Stroke::new(1.0_f32, palette.line))
                .corner_radius(6)
                .inner_margin(12)
                .show(ui, |ui| {
                    let changed = egui::ScrollArea::vertical()
                        .max_height(112.0)
                        .show(ui, |ui| {
                            let edit = TextEdit::multiline(&mut self.text)
                                .font(FontId::monospace(13.0))
                                .hint_text(
                                    RichText::new("https://www.youtube.com/watch?v=…")
                                        .color(palette.muted),
                                )
                                .desired_width(f32::INFINITY)
                                .desired_rows(6)
                                .frame(false);
                            ui.add_enabled(idle, edit).changed()
                        })
                        .inner;
                    if changed {
                        self.links = parse_links(&self.text);
                    }
                });
            ui.horizontal_wrapped(|ui| {
                let count = self.links.urls.len();
                let label = if count == 1 {
                    "1 video link".to_owned()
                } else {
                    format!("{count} video links")
                };
                let color = if count == 0 {
                    palette.muted
                } else {
                    palette.accent
                };
                ui.label(RichText::new(label).size(13.0).color(color));
                if self.links.invalid > 0 {
                    ui.label(
                        RichText::new(format!("{} invalid", self.links.invalid))
                            .color(palette.error),
                    );
                }
                if self.links.duplicates > 0 {
                    ui.label(
                        RichText::new(format!("{} duplicate", self.links.duplicates))
                            .color(palette.muted),
                    );
                }
            });
        });
    }
}
