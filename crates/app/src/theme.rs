//! The DriveCraft design system: the Craft suite "Studio" look.
//!
//! The Studio palettes (dark and light) come from PhotoCraft
//! (`crates/ui-egui/src/theme.rs`, MIT OR Apache-2.0), so DriveCraft looks
//! like the other Craft apps: Inter, Lucide icons, rounded cards, and a soft
//! violet accent. DriveCraft adds a `success` color for sync status.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    Style, TextStyle, Theme, Visuals,
};

#[derive(Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    /// Main content background.
    pub chrome: Color32,
    /// Sidebar background.
    pub dock: Color32,
    pub card: Color32,
    pub card_border: Color32,
    pub field: Color32,
    pub field_border: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_border: Color32,
    pub accent_text: Color32,
    pub separator: Color32,
    pub shadow: Color32,
    pub primary_bg: Color32,
    pub primary_text: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
}

pub const RADIUS_SM: u8 = 6;
pub const RADIUS: u8 = 8;
pub const RADIUS_LG: u8 = 12;

pub const DARK: Tokens = Tokens {
    dark: true,
    chrome: Color32::from_rgb(20, 20, 21),
    dock: Color32::from_rgb(17, 17, 18),
    card: Color32::from_rgb(26, 26, 28),
    card_border: Color32::from_rgb(40, 40, 44),
    field: Color32::from_rgb(35, 35, 38),
    field_border: Color32::from_rgb(52, 52, 57),
    hover: Color32::from_rgb(44, 44, 48),
    pressed: Color32::from_rgb(56, 56, 62),
    text: Color32::from_rgb(236, 236, 240),
    text_dim: Color32::from_rgb(150, 150, 158),
    text_faint: Color32::from_rgb(96, 96, 104),
    icon: Color32::from_rgb(196, 196, 204),
    accent: Color32::from_rgb(139, 124, 246),
    accent_soft: Color32::from_rgba_premultiplied(25, 22, 44, 46),
    accent_border: Color32::from_rgba_premultiplied(69, 64, 110, 110),
    accent_text: Color32::from_rgb(214, 208, 255),
    separator: Color32::from_rgb(38, 38, 42),
    shadow: Color32::from_black_alpha(140),
    primary_bg: Color32::from_rgb(246, 246, 248),
    primary_text: Color32::from_rgb(12, 12, 14),
    danger: Color32::from_rgb(240, 96, 96),
    danger_soft: Color32::from_rgba_premultiplied(43, 17, 17, 46),
    warning: Color32::from_rgb(240, 190, 90),
    warning_soft: Color32::from_rgba_premultiplied(43, 34, 16, 46),
    success: Color32::from_rgb(92, 204, 140),
    success_soft: Color32::from_rgba_premultiplied(17, 37, 25, 46),
};

pub const LIGHT: Tokens = Tokens {
    dark: false,
    chrome: Color32::from_rgb(246, 246, 248),
    dock: Color32::from_rgb(240, 240, 243),
    card: Color32::from_rgb(252, 252, 253),
    card_border: Color32::from_rgb(222, 222, 228),
    field: Color32::from_rgb(242, 242, 245),
    field_border: Color32::from_rgb(214, 214, 220),
    hover: Color32::from_rgb(232, 232, 237),
    pressed: Color32::from_rgb(220, 220, 226),
    text: Color32::from_rgb(24, 24, 28),
    text_dim: Color32::from_rgb(96, 96, 106),
    text_faint: Color32::from_rgb(150, 150, 160),
    icon: Color32::from_rgb(60, 60, 68),
    accent: Color32::from_rgb(108, 92, 231),
    accent_soft: Color32::from_rgba_premultiplied(15, 13, 33, 36),
    accent_border: Color32::from_rgba_premultiplied(51, 43, 109, 120),
    accent_text: Color32::from_rgb(80, 64, 200),
    separator: Color32::from_rgb(226, 226, 232),
    shadow: Color32::from_black_alpha(50),
    primary_bg: Color32::from_rgb(20, 20, 24),
    primary_text: Color32::from_rgb(250, 250, 252),
    danger: Color32::from_rgb(210, 60, 60),
    danger_soft: Color32::from_rgba_premultiplied(30, 9, 9, 36),
    warning: Color32::from_rgb(190, 130, 20),
    warning_soft: Color32::from_rgba_premultiplied(27, 18, 3, 36),
    success: Color32::from_rgb(22, 150, 80),
    success_soft: Color32::from_rgba_premultiplied(3, 21, 11, 36),
};

impl Tokens {
    pub fn of(ui: &egui::Ui) -> &'static Tokens {
        if ui.visuals().dark_mode {
            &DARK
        } else {
            &LIGHT
        }
    }
}

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/icons/lucide.ttf");

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

/// Installs the fonts and the light and dark styles. `ThemePreference`
/// then selects the style.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_style_of(Theme::Light, style(&LIGHT));
    ctx.set_style_of(Theme::Dark, style(&DARK));
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("Inter", INTER_REGULAR),
        ("Inter-Medium", INTER_MEDIUM),
        ("Inter-SemiBold", INTER_SEMIBOLD),
        ("lucide", LUCIDE),
    ] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "Inter".into());
    proportional.insert(1, "lucide".into());
    let fallback = proportional.clone();
    for (family, first) in [("medium", "Inter-Medium"), ("semibold", "Inter-SemiBold")] {
        let mut chain = vec![first.to_string()];
        chain.extend(fallback.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(family.into()), chain);
    }
    fonts
        .families
        .insert(FontFamily::Name("icons".into()), vec!["lucide".into()]);
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("lucide".into());
    ctx.set_fonts(fonts);
}

fn style(t: &Tokens) -> Style {
    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.chrome;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.card_border);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.card;
    v.code_bg_color = t.field;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.danger;
    v.window_corner_radius = CornerRadius::same(RADIUS_LG);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: t.shadow,
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: t.shadow,
    };
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent_text);
    v.text_cursor.stroke = Stroke::new(1.5, t.accent);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.striped = false;
    v.indent_has_left_vline = false;
    let r = CornerRadius::same(RADIUS_SM);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.card;
    w.noninteractive.weak_bg_fill = t.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    w.noninteractive.corner_radius = r;
    for (wv, bg, stroke) in [
        (&mut w.inactive, t.field, t.field_border),
        (&mut w.hovered, t.hover, t.field_border),
        (&mut w.active, t.pressed, t.accent_border),
        (&mut w.open, t.hover, t.field_border),
    ] {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::new(1.0, stroke);
        wv.fg_stroke = Stroke::new(1.0, t.text);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }

    let mut s = Style {
        visuals: v,
        ..Style::default()
    };
    s.text_styles = [
        (TextStyle::Small, regular(11.0)),
        (TextStyle::Body, regular(13.0)),
        (TextStyle::Button, medium(13.0)),
        (TextStyle::Heading, semibold(20.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    s.spacing.item_spacing = egui::vec2(8.0, 8.0);
    s.spacing.button_padding = egui::vec2(12.0, 6.0);
    s.spacing.interact_size = egui::vec2(24.0, 28.0);
    s.spacing.combo_width = 160.0;
    s.spacing.menu_margin = Margin::same(6);
    s.spacing.window_margin = Margin::same(16);
    s.spacing.icon_width = 14.0;
    s.spacing.tooltip_width = 280.0;
    let mut scroll = egui::style::ScrollStyle::thin();
    scroll.interact_handle_opacity = 1.0;
    scroll.interact_background_opacity = 0.4;
    scroll.floating_width = 4.0;
    s.spacing.scroll = scroll;
    s.interaction.tooltip_delay = 0.35;
    s
}
