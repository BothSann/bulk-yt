mod download;
mod queue;
mod theme;

use std::ffi::OsString;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use download::{Links, Tools, FORMATS};
use eframe::egui::{
    self, Button, Color32, Context, FontId, Frame, Layout, Margin, RichText, Stroke, TextEdit,
    ThemePreference, Ui, Vec2, ViewportCommand,
};
use queue::{Queue, State};
use theme::Palette;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let screenshot = match (args.next(), args.next(), args.next()) {
        (None, None, None) => None,
        (Some(flag), Some(path), None) if flag == OsString::from("--screenshot") => {
            Some(Screenshot {
                path: path.into(),
                frames: 0,
                requested: None,
            })
        }
        _ => {
            eprintln!("Usage: bulk-yt [--screenshot <file.png>]");
            return ExitCode::FAILURE;
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1030.0, 740.0])
        .with_min_inner_size([540.0, 480.0]);
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
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
            Ok(Box::new(App::new(screenshot, capture_tx)))
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

struct Screenshot {
    path: PathBuf,
    frames: u32,
    requested: Option<Instant>,
}

impl Screenshot {
    fn update(&mut self, ctx: &Context) -> Option<Result<(), String>> {
        self.frames += 1;
        if self.frames == 120 {
            self.requested = Some(Instant::now());
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
        }
        if let Some(requested) = self.requested {
            let image = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                return Some(save_png(&self.path, &image).map_err(|error| error.to_string()));
            }
            if requested.elapsed() > Duration::from_secs(8) {
                return Some(Err("The window did not return an image.".to_owned()));
            }
        }
        ctx.request_repaint_after(Duration::from_millis(16));
        None
    }
}

fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut encoder = png::Encoder::new(
        BufWriter::new(fs::File::create(path)?),
        image.size[0].try_into()?,
        image.size[1].try_into()?,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let pixels: Vec<u8> = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_array())
        .collect();
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&pixels)?;
    writer.finish()?;
    Ok(())
}

struct App {
    appearance: ThemePreference,
    text: String,
    links: Links,
    format: usize,
    folder: PathBuf,
    missing: Vec<&'static str>,
    queue: Queue,
    output_folder: Option<PathBuf>,
    notice: Option<String>,
    show_setup: bool,
    screenshot: Option<Screenshot>,
    capture_result: mpsc::SyncSender<Result<(), String>>,
}

impl App {
    fn new(
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
            links: download::parse_links(""),
            format: 0,
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

    fn start(&mut self, ctx: &Context) {
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
            FORMATS[self.format].1,
            self.folder.clone(),
            move || ctx.request_repaint(),
        );
    }

    fn header(&mut self, ui: &mut Ui, palette: &Palette) {
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

    fn links_panel(&mut self, ui: &mut Ui, width: f32, palette: &Palette) {
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
                            self.links = download::parse_links("");
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
                        self.links = download::parse_links(&self.text);
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

    fn settings_panel(&mut self, ui: &mut Ui, width: f32, palette: &Palette) {
        let idle = !self.queue.is_running();
        card(ui, width, palette, |ui| {
            ui.set_min_height(217.0);
            ui.label(theme::heading("Audio format", 19.0));
            ui.label(
                RichText::new(FORMATS[self.format].2)
                    .size(13.0)
                    .color(palette.muted),
            );
            ui.separator();
            ui.add_enabled_ui(idle, |ui| {
                let button_width = (ui.available_width() - ui.spacing().item_spacing.x * 2.0) / 3.0;
                for (row, formats) in FORMATS.chunks(3).enumerate() {
                    ui.horizontal(|ui| {
                        for (column, (name, _, _)) in formats.iter().enumerate() {
                            let index = row * 3 + column;
                            let selected = self.format == index;
                            let text = if selected {
                                theme::medium(*name).color(palette.accent)
                            } else {
                                RichText::new(*name)
                            };
                            if ui
                                .add_sized(
                                    [button_width, 30.0],
                                    Button::new(text).selected(selected).corner_radius(6),
                                )
                                .clicked()
                            {
                                self.format = index;
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

    fn actions(&mut self, ui: &mut Ui, palette: &Palette) {
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

    fn queue_panel(&mut self, ui: &mut Ui, palette: &Palette) {
        let width = ui.available_width();
        card(ui, width, palette, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::heading("Download queue", 19.0));
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(folder) = &self.output_folder {
                        if ui.button("Open output folder").clicked() {
                            self.notice = download::open_folder(folder).err();
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
                        State::Downloading(1.0) => ("Converting", palette.accent),
                        State::Downloading(_) => ("Downloading", palette.accent),
                        State::Saved(_) => ("Saved", palette.accent),
                        State::Failed(_) => ("Failed", palette.error),
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
                    State::Downloading(progress) if *progress == 1.0 => {
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
                        ui.label(RichText::new(error).color(palette.error));
                    }
                    State::Waiting => {}
                }
                ui.separator();
            }
        });
    }

    fn setup_dialog(&mut self, ctx: &Context) {
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

fn card(ui: &mut Ui, width: f32, palette: &Palette, add_contents: impl FnOnce(&mut Ui)) {
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

fn pill(ui: &mut Ui, label: &str, background: Color32, foreground: Color32) {
    Frame::new()
        .fill(background)
        .corner_radius(12)
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.label(theme::medium(label).size(12.0).color(foreground));
        });
}

fn wave_mark(ui: &mut Ui, size: f32, background: Color32, foreground: Color32) {
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
