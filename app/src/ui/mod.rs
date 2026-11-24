mod actions;
mod header;
mod links;
mod queue;
mod settings;
mod setup;
mod widgets;

use eframe::egui::{self, Context, Frame, Margin, ViewportCommand};

use crate::app::App;
use crate::theme::Palette;

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.queue.poll();
        if self.queue.is_running() && ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.notice = Some("You can close the window when this batch finishes.".to_owned());
        }

        let palette = Palette::for_theme(ctx.theme());
        egui::TopBottomPanel::top("header")
            .frame(
                Frame::new()
                    .fill(palette.header)
                    .inner_margin(Margin::symmetric(24, 17)),
            )
            .show_separator_line(true)
            .show(ctx, |ui| self.header(ui, &palette));
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(palette.canvas)
                    .inner_margin(Margin::symmetric(24, 24)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        if width >= 680.0 {
                            let links_width = (width - 16.0) * 0.56;
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 16.0;
                                self.links_panel(ui, links_width, &palette);
                                self.settings_panel(ui, width - 16.0 - links_width, &palette);
                            });
                        } else {
                            self.links_panel(ui, width, &palette);
                            ui.add_space(8.0);
                            self.settings_panel(ui, width, &palette);
                        }
                        ui.add_space(10.0);
                        self.actions(ui, &palette);
                        ui.add_space(16.0);
                        self.queue_panel(ui, &palette);
                    });
            });
        if self.show_setup {
            self.setup_dialog(ctx);
        }
        if let Some(result) = self.screenshot.as_mut().and_then(|shot| shot.update(ctx)) {
            self.screenshot = None;
            let _ = self.capture_result.send(result);
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}
