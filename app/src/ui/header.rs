use eframe::egui::{self, Color32, Layout, RichText, ThemePreference, Ui};

use super::widgets::wave_mark;
use crate::app::App;
use crate::theme::{self, Palette};

impl App {
    pub(super) fn header(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.horizontal(|ui| {
            wave_mark(ui, 40.0, palette.brand, Color32::WHITE);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(theme::heading("Batch Audio", 25.0));
                ui.label(
                    RichText::new("Save YouTube videos as audio files.")
                        .size(14.0)
                        .color(palette.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let spacing = ui.spacing();
                let dropdown_height = ui
                    .text_style_height(&egui::TextStyle::Button)
                    .max(spacing.icon_width)
                    + 2.0 * spacing.button_padding.y;
                ui.spacing_mut().interact_size.y = dropdown_height.max(spacing.interact_size.y);

                let previous = self.appearance;
                egui::ComboBox::new(
                    "appearance",
                    RichText::new("Appearance").color(palette.muted),
                )
                .width(84.0)
                .selected_text(match self.appearance {
                    ThemePreference::System => "System",
                    ThemePreference::Light => "Light",
                    ThemePreference::Dark => "Dark",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.appearance, ThemePreference::System, "System");
                    ui.selectable_value(&mut self.appearance, ThemePreference::Light, "Light");
                    ui.selectable_value(&mut self.appearance, ThemePreference::Dark, "Dark");
                });
                if self.appearance != previous {
                    ui.ctx().set_theme(self.appearance);
                    ui.ctx().request_repaint();
                }
            });
        });
    }
}
