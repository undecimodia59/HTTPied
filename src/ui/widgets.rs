use eframe::egui::{self, Color32, RichText};

pub const BG: Color32 = Color32::from_rgb(27, 28, 31);
pub const SURFACE: Color32 = Color32::from_rgb(30, 31, 34);
pub const BORDER: Color32 = Color32::from_rgb(69, 71, 76);
pub const MUTED: Color32 = Color32::from_rgb(144, 162, 184);
pub const ACCENT: Color32 = Color32::from_rgb(135, 184, 235);
pub const WARN: Color32 = Color32::from_rgb(244, 190, 106);
pub const ERROR: Color32 = Color32::from_rgb(255, 132, 141);

pub fn theme(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.override_text_color = Some(Color32::from_rgb(221, 230, 242));
    style.visuals.selection.bg_fill = Color32::from_rgb(50, 65, 85);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(35, 52, 70);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(35, 52, 70);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(9.0, 4.0);
    style.spacing.interact_size.y = 26.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(0)
        .inner_margin(6)
}
pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.label(RichText::new(text).color(color).size(12.0));
}
pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED).size(12.0));
}
