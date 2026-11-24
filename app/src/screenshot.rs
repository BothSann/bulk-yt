use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, Context, ViewportCommand};

/// Saves one image of the window once its layout has settled.
pub struct Screenshot {
    path: PathBuf,
    frames: u32,
    requested: Option<Instant>,
}

impl Screenshot {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            frames: 0,
            requested: None,
        }
    }

    /// Call once per frame. Returns the result when the capture is done.
    pub fn update(&mut self, ctx: &Context) -> Option<Result<(), String>> {
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
