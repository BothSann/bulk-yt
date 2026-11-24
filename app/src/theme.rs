use std::sync::Arc;

use eframe::egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, RichText,
    Shadow, Stroke, TextStyle, Theme, Vec2,
};

const DISPLAY: &str = "Batch Audio Display";
const MEDIUM: &str = "Inter Medium";

pub fn heading(text: &str, size: f32) -> RichText {
    RichText::new(text.to_ascii_uppercase()).font(FontId::new(size, named_font(DISPLAY)))
}

pub fn medium(text: impl Into<String>) -> RichText {
    RichText::new(text).family(named_font(MEDIUM))
}

fn named_font(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

pub struct Palette {
    pub canvas: Color32,
    pub header: Color32,
    pub surface: Color32,
    pub input: Color32,
    pub line: Color32,
    pub card_shadow: Shadow,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_surface: Color32,
    pub brand: Color32,
    pub primary: Color32,
    pub primary_text: Color32,
    pub error: Color32,
}

impl Palette {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self {
                canvas: Color32::from_gray(250),
                header: Color32::from_gray(237),
                surface: Color32::WHITE,
                input: Color32::from_gray(246),
                line: Color32::from_gray(222),
                card_shadow: Shadow {
                    offset: [0, 1],
                    blur: 3,
                    spread: 0,
                    color: Color32::from_black_alpha(14),
                },
                text: Color32::from_gray(24),
                muted: Color32::from_gray(100),
                accent: Color32::from_rgb(75, 65, 202),
                accent_surface: Color32::from_rgb(235, 233, 254),
                brand: Color32::from_rgb(91, 76, 226),
                primary: Color32::from_gray(24),
                primary_text: Color32::WHITE,
                error: Color32::from_rgb(170, 44, 52),
            },
            Theme::Dark => Self {
                canvas: Color32::from_rgb(23, 23, 27),
                header: Color32::from_rgb(30, 30, 35),
                surface: Color32::from_rgb(31, 31, 36),
                input: Color32::from_rgb(25, 25, 30),
                line: Color32::from_rgb(57, 57, 65),
                card_shadow: Shadow::NONE,
                text: Color32::from_rgb(244, 244, 247),
                muted: Color32::from_rgb(172, 172, 182),
                accent: Color32::from_rgb(182, 172, 255),
                accent_surface: Color32::from_rgb(50, 43, 78),
                brand: Color32::from_rgb(101, 82, 229),
                primary: Color32::from_gray(244),
                primary_text: Color32::from_gray(24),
                error: Color32::from_rgb(255, 160, 164),
            },
        }
    }
}

pub fn configure(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    for (name, data) in [
        (
            "Inter",
            include_bytes!("../assets/fonts/Inter-Regular.otf").as_slice(),
        ),
        (
            MEDIUM,
            include_bytes!("../assets/fonts/Inter-Medium.otf").as_slice(),
        ),
        (
            DISPLAY,
            include_bytes!("../assets/fonts/BatchAudioDisplay.ttf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(data)));
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".to_owned());
    let proportional = proportional.clone();
    for name in [MEDIUM, DISPLAY] {
        let mut family = proportional.clone();
        family.insert(0, name.to_owned());
        fonts.families.insert(named_font(name), family);
    }
    ctx.set_fonts(fonts);

    for theme in [Theme::Light, Theme::Dark] {
        let palette = Palette::for_theme(theme);
        ctx.style_mut_of(theme, |style| {
            let visuals = &mut style.visuals;
            visuals.panel_fill = palette.canvas;
            visuals.window_fill = palette.surface;
            visuals.window_stroke = Stroke::new(1.0_f32, palette.line);
            visuals.extreme_bg_color = palette.input;
            visuals.faint_bg_color = palette.input;
            visuals.code_bg_color = palette.input;
            visuals.hyperlink_color = palette.accent;
            visuals.error_fg_color = palette.error;
            visuals.selection.bg_fill = palette.accent_surface;
            visuals.selection.stroke = Stroke::new(1.0_f32, palette.accent);
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, palette.line);
            visuals.widgets.noninteractive.fg_stroke.color = palette.text;

            for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.open] {
                widget.bg_fill = palette.surface;
                widget.weak_bg_fill = palette.surface;
                widget.bg_stroke = Stroke::new(1.0_f32, palette.line);
                widget.fg_stroke.color = palette.text;
                widget.corner_radius = CornerRadius::same(6);
            }
            for widget in [&mut visuals.widgets.hovered, &mut visuals.widgets.active] {
                widget.bg_fill = palette.accent_surface;
                widget.weak_bg_fill = palette.accent_surface;
                widget.bg_stroke = Stroke::new(1.0_f32, palette.accent);
                widget.fg_stroke.color = palette.text;
                widget.corner_radius = CornerRadius::same(6);
                widget.expansion = 0.0;
            }
            style.spacing.item_spacing = Vec2::new(8.0, 8.0);
            style.spacing.button_padding = Vec2::new(11.0, 7.0);
            for (text_style, size) in [(TextStyle::Body, 14.0), (TextStyle::Button, 14.0)] {
                style
                    .text_styles
                    .insert(text_style, FontId::proportional(size));
            }
        });
    }
}
