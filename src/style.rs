use eframe::egui;

/// Applies a darker, more spacious theme with a blue accent than egui's
/// stock dark visuals.
pub fn apply_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(22, 24, 30);
    visuals.window_fill = egui::Color32::from_rgb(22, 24, 30);
    visuals.extreme_bg_color = egui::Color32::from_rgb(16, 17, 22);
    visuals.faint_bg_color = egui::Color32::from_rgb(28, 30, 37);
    visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(22, 24, 30);
    visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(37, 40, 49);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(53, 92, 135);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(66, 133, 199);
    visuals.selection.bg_fill = egui::Color32::from_rgb(59, 110, 168);
    visuals.selection.stroke.color = egui::Color32::WHITE;
    visuals.window_rounding = egui::Rounding::same(8.0);
    visuals.menu_rounding = egui::Rounding::same(6.0);
    visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
    visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);
    visuals.widgets.active.rounding = egui::Rounding::same(6.0);
    visuals.widgets.noninteractive.rounding = egui::Rounding::same(6.0);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(10.0);
    style.spacing.indent = 18.0;
    for (text_style, font_id) in style.text_styles.iter_mut() {
        match text_style {
            egui::TextStyle::Heading => font_id.size = 20.0,
            egui::TextStyle::Body => font_id.size = 14.5,
            egui::TextStyle::Button => font_id.size = 14.5,
            egui::TextStyle::Small => font_id.size = 12.0,
            egui::TextStyle::Monospace => font_id.size = 13.5,
            _ => {}
        }
    }
    ctx.set_style(style);
}
