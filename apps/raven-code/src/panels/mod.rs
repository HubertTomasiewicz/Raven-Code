//! UI panels: menu bar, explorer, editor area, bottom panel, status bar,
//! command palette and dialogs. Shared styling helpers live in `mod.rs`.

pub mod activity_bar;
pub mod bottom_panel;
pub mod dialogs;
pub mod editor_area;
pub mod explorer;
pub mod menu_bar;
pub mod notifications;
pub mod palette;
pub mod status_bar;

use eframe::egui;

use raven_editor::theme::Theme;

/// Applies the Raven theme to egui's style + visuals for this frame.
pub fn apply_theme(ctx: &egui::Context, theme: &'static Theme, reduce_motion: bool) {
    let colors = &theme.colors;
    let color =
        |rgba: [u8; 4]| egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]);

    ctx.all_styles_mut(|style| {
        let visuals = &mut style.visuals;

        visuals.window_fill = color(colors.overlay_bg);
        visuals.panel_fill = color(colors.panel_bg);
        visuals.extreme_bg_color = color(colors.input_bg);
        visuals.faint_bg_color = color(colors.line_highlight);
        visuals.override_text_color = Some(color(colors.text));

        visuals.widgets.noninteractive.bg_fill = color(colors.panel_bg);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, color(colors.text));
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, color(colors.border));

        visuals.widgets.inactive.bg_fill = color(colors.panel_header_bg);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, color(colors.text_dim));
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, color(colors.border));

        visuals.widgets.hovered.bg_fill = color(colors.hover);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, color(colors.text));
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, color(colors.accent_dim));

        visuals.widgets.active.bg_fill = color(colors.accent_dim);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, color(colors.text_bright));
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, color(colors.accent));

        visuals.selection.bg_fill = color(colors.selection);
        visuals.selection.stroke = egui::Stroke::new(1.0, color(colors.accent));
        visuals.hyperlink_color = color(colors.accent);

        visuals.window_stroke = egui::Stroke::new(1.0, color(colors.border));

        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.animation_time = if reduce_motion { 0.0 } else { 0.16 };
        style.spacing.button_padding = egui::vec2(6.0, 3.0);
        style.spacing.menu_margin = egui::Margin::symmetric(4, 2);
        style.spacing.icon_width = 14.0;
        style.spacing.icon_width_inner = 10.0;
    });
}

/// Small colored dot (diagnostics counters).
pub fn status_dot(ui: &mut egui::Ui, color: [u8; 4]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
    ui.painter().circle_filled(
        rect.center(),
        3.5,
        egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
    );
}

/// Formats a byte count for humans.
pub fn human_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes >= MB {
        format!("{:.1} MiB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KiB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
