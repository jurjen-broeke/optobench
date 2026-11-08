//! Colours, sizes and widget style, in SolidCraft's design language: its dark theme's
//! colours, radii, spacing and type sizes (adapted from SolidCraft's
//! `assets/styles/ui/palettes.rs` and `crates/ui-egui/src/theme.rs`, MIT OR Apache-2.0;
//! ATTRIBUTION.md), with optobench's own workspace accents.

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

/// The colours and sizes the interface draws with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// The title bar.
    pub app_bar: Color32,
    /// The ribbon and panels.
    pub panel: Color32,
    /// Panel headers.
    pub panel_header: Color32,
    /// Borders and separators.
    pub border: Color32,
    /// Text.
    pub text: Color32,
    /// Secondary text, counts and hints.
    pub text_dim: Color32,
    /// Hovered rows and buttons.
    pub hover: Color32,
    /// Selected rows.
    pub selected_bg: Color32,
    /// Text on selected rows.
    pub selected_ink: Color32,
    /// Input fields.
    pub control_fill: Color32,
    /// Input field borders.
    pub control_border: Color32,
    /// Errors.
    pub error: Color32,
    /// Warnings.
    pub warning: Color32,
    /// The 3D view's background, top and bottom.
    pub viewport_top: Color32,
    /// See `viewport_top`.
    pub viewport_bottom: Color32,
    /// Corner radii (logical pixels).
    pub radius_sm: u8,
    /// See `radius_sm`.
    pub radius_md: u8,
    /// See `radius_sm`.
    pub radius_lg: u8,
    /// Type sizes (points).
    pub font_small: f32,
    /// See `font_small`.
    pub font_body: f32,
    /// See `font_small`.
    pub font_button: f32,
    /// See `font_small`.
    pub font_heading: f32,
    /// Height of controls and ribbon rows.
    pub control_height: f32,
}

/// SolidCraft's dark theme (its default).
pub const DARK: Tokens = Tokens {
    app_bar: Color32::from_rgb(28, 31, 38),
    panel: Color32::from_rgb(39, 43, 52),
    panel_header: Color32::from_rgb(47, 52, 62),
    border: Color32::from_rgb(63, 69, 82),
    text: Color32::from_rgb(222, 226, 232),
    text_dim: Color32::from_rgb(148, 155, 168),
    hover: Color32::from_rgb(54, 61, 74),
    selected_bg: Color32::from_rgb(42, 70, 110),
    selected_ink: Color32::from_rgb(156, 199, 245),
    control_fill: Color32::from_rgb(28, 31, 38),
    control_border: Color32::from_rgb(74, 82, 98),
    error: Color32::from_rgb(240, 100, 96),
    warning: Color32::from_rgb(236, 170, 70),
    viewport_top: Color32::from_rgb(52, 59, 72),
    viewport_bottom: Color32::from_rgb(38, 42, 52),
    radius_sm: 4,
    radius_md: 6,
    radius_lg: 8,
    font_small: 11.0,
    font_body: 12.0,
    font_button: 13.0,
    font_heading: 14.0,
    control_height: 28.0,
};

/// A workspace's accent: a strong fill (the tile, primary buttons), its ink (the active
/// tab's text) and a soft fill (selected tiles), as SolidCraft gives each workspace one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accent {
    /// Strong fill.
    pub fill: Color32,
    /// Ink.
    pub ink: Color32,
    /// Soft fill.
    pub soft: Color32,
}

/// CAD: SolidCraft's own blue, since it works on CAD assemblies.
pub const CAD: Accent = Accent {
    fill: Color32::from_rgb(58, 136, 238),
    ink: Color32::from_rgb(156, 199, 245),
    soft: Color32::from_rgb(42, 70, 110),
};

/// Layout: optobench's turquoise.
pub const LAYOUT: Accent = Accent {
    fill: Color32::from_rgb(22, 150, 140),
    ink: Color32::from_rgb(88, 212, 198),
    soft: Color32::from_rgb(24, 60, 60),
};

/// Simulate: a laser amber, apart from SolidCraft's render pink.
pub const SIMULATE: Accent = Accent {
    fill: Color32::from_rgb(214, 128, 36),
    ink: Color32::from_rgb(244, 178, 92),
    soft: Color32::from_rgb(70, 48, 26),
};

/// Applies the tokens to egui's style.
pub fn install(ctx: &egui::Context) {
    let t = DARK;
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.panel_fill = t.panel;
        v.window_fill = t.panel;
        v.extreme_bg_color = t.control_fill;
        v.faint_bg_color = t.panel_header;
        v.override_text_color = Some(t.text);
        v.selection.bg_fill = t.selected_bg;
        v.selection.stroke = Stroke::new(1.0, t.selected_ink);
        v.window_corner_radius = CornerRadius::same(t.radius_lg);
        v.menu_corner_radius = CornerRadius::same(t.radius_md);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(t.radius_md);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_fill = t.control_fill;
        v.widgets.hovered.weak_bg_fill = t.hover;
        v.widgets.hovered.bg_fill = t.hover;
        v.widgets.active.weak_bg_fill = t.selected_bg;
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 22.0;
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(t.font_small, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(t.font_body, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(t.font_button, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(t.font_heading, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(t.font_body, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
