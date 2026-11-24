use eframe::egui::{self, Color32, Frame, Layout, Margin, Stroke, Ui, Vec2};

use crate::theme::{self, Palette};

pub fn card(ui: &mut Ui, width: f32, palette: &Palette, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0_f32, palette.line))
        .shadow(palette.card_shadow)
        .corner_radius(8)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.with_layout(Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width((width - 38.0).max(0.0));
                add_contents(ui);
            });
        });
}

pub fn pill(ui: &mut Ui, label: &str, background: Color32, foreground: Color32) {
    Frame::new()
        .fill(background)
        .corner_radius(12)
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(theme::medium(label).size(12.0).color(foreground));
        });
}

pub fn wave_mark(ui: &mut Ui, size: f32, background: Color32, foreground: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 11.0, background);
    for (index, height) in [0.16, 0.31, 0.46, 0.28, 0.16].into_iter().enumerate() {
        let x = rect.center().x + (index as f32 - 2.0) * (size / 9.0);
        let span = height * size / 2.0;
        painter.line_segment(
            [
                egui::pos2(x, rect.center().y - span),
                egui::pos2(x, rect.center().y + span),
            ],
            Stroke::new(2.3_f32, foreground),
        );
    }
}
