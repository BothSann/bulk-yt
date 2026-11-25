use eframe::egui::{
    pos2, ColorImage, Context, IconData, Rect, TextureFilter, TextureHandle, TextureOptions,
};
use eframe::icon_data;

const ICON: &[u8] = include_bytes!("../assets/icons/icon-512.png");
/// Padded to match the size of other icons in the Dock.
#[cfg(any(target_os = "macos", test))]
const MACOS_ICON: &[u8] = include_bytes!("../assets/icons/icon-macos-512.png");

/// The icon for the Dock, the taskbar, and the window.
pub fn app_icon() -> IconData {
    #[cfg(target_os = "macos")]
    let bytes = MACOS_ICON;
    #[cfg(not(target_os = "macos"))]
    let bytes = ICON;
    decode(bytes)
}

/// The icon as a texture. Mipmaps keep it smooth at the size of the header.
pub fn logo(ctx: &Context) -> TextureHandle {
    let icon = decode(ICON);
    let image =
        ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    let options = TextureOptions {
        mipmap_mode: Some(TextureFilter::Linear),
        ..TextureOptions::LINEAR
    };
    ctx.load_texture("logo", image, options)
}

/// The part of the logo inside its transparent margin, in texture coordinates.
pub fn logo_crop() -> Rect {
    Rect::from_min_max(pos2(15.5, 20.0), pos2(492.5, 497.0)) / 512.0
}

fn decode(bytes: &[u8]) -> IconData {
    icon_data::from_png_bytes(bytes).expect("embedded icons are valid PNG files")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icons_decode_and_are_square() {
        for bytes in [ICON, MACOS_ICON] {
            let icon = decode(bytes);
            assert_eq!((icon.width, icon.height), (512, 512));
        }
    }
}
