use eframe::egui::{self, Color32, RichText};

pub const BG: Color32 = Color32::from_rgb(19, 22, 28);
pub const SURFACE: Color32 = Color32::from_rgb(25, 30, 38);
pub const BORDER: Color32 = Color32::from_rgb(53, 63, 77);
pub const MUTED: Color32 = Color32::from_rgb(155, 170, 191);
pub const ACCENT: Color32 = Color32::from_rgb(123, 177, 255);
pub const WARN: Color32 = Color32::from_rgb(244, 190, 106);
pub const ERROR: Color32 = Color32::from_rgb(255, 132, 141);

pub fn theme(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = Color32::from_rgb(16, 20, 26);
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.override_text_color = Some(Color32::from_rgb(221, 230, 242));
    style.visuals.selection.bg_fill = Color32::from_rgb(40, 66, 103);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(35, 52, 70);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(35, 52, 70);
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
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
pub fn primary(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).color(Color32::WHITE))
            .fill(Color32::from_rgb(46, 99, 172)),
    )
}
pub fn empty_state(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.add_space(24.0);
    ui.strong(title);
    ui.weak(detail);
}
pub fn resize_split_with_window(ui: &egui::Ui, id: egui::Id, width: f32) {
    ui.ctx().data_mut(|data| {
        let key = id.with("available-width");
        if data
            .get_temp::<f32>(key)
            .is_some_and(|old| (old - width).abs() > 1.0)
        {
            data.remove::<egui::containers::panel::PanelState>(id);
        }
        data.insert_temp(key, width);
    });
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
