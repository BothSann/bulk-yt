mod app;
mod cancel;
mod downloader;
mod format;
mod icon;
mod links;
mod queue;
mod screenshot;
mod theme;
mod ui;

use std::process::ExitCode;
use std::sync::mpsc;

use app::App;
use eframe::egui::{self, ThemePreference};
use screenshot::Screenshot;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let screenshot = match (args.next(), args.next(), args.next()) {
        (None, None, None) => None,
        (Some(flag), Some(path), None) if flag == "--screenshot" => {
            Some(Screenshot::new(path.into()))
        }
        _ => {
            eprintln!("Usage: bulk-yt [--screenshot <file.png>]");
            return ExitCode::FAILURE;
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_icon(icon::app_icon())
        .with_inner_size([1030.0, 740.0])
        .with_min_inner_size([540.0, 480.0]);
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        options.event_loop_builder = Some(Box::new(|event_loop| {
            event_loop.with_activate_ignoring_other_apps(false);
        }));
    }
    // Share the result with main so a failed capture returns a failing exit status.
    let (capture_tx, capture_rx) = mpsc::sync_channel(1);
    let capturing = screenshot.is_some();
    let result = eframe::run_native(
        "Batch Audio",
        options,
        Box::new(move |cc| {
            theme::configure(&cc.egui_ctx);
            cc.egui_ctx.set_theme(ThemePreference::System);
            Ok(Box::new(App::new(&cc.egui_ctx, screenshot, capture_tx)))
        }),
    );
    if let Err(error) = result {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    if capturing {
        match capture_rx.try_recv() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                eprintln!("Screenshot failed: {error}");
                return ExitCode::FAILURE;
            }
            Err(_) => {
                eprintln!("The window closed before saving the screenshot.");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
