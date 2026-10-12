//! Compact left rail for switching the primary workbench views.

use eframe::egui;

use crate::app::RavenApp;

pub fn show(app: &mut RavenApp, ui: &mut egui::Ui) {
    let colors = app.colors();
    let rail = egui::Color32::from_rgba_unmultiplied(
        colors.panel_bg[0],
        colors.panel_bg[1],
        colors.panel_bg[2],
        255,
    );
    egui::Panel::left("raven-activity-bar")
        .resizable(false)
        .exact_size(44.0)
        .frame(egui::Frame::new().fill(rail))
        .show(ui, |ui| {
            ui.add_space(8.0);
            let explorer_active = app.settings.workspace.sidebar_visible;
            let explorer = rail_button(
                ui,
                "explorer",
                "Explorer",
                explorer_active,
                app.settings.appearance.reduce_motion,
            );
            if explorer.clicked() {
                if app.workspace.is_some() {
                    app.settings.workspace.sidebar_visible = !explorer_active;
                    app.settings_dirty = true;
                } else {
                    app.run_action(crate::actions::Action::OpenFolder);
                }
            }

            ui.add_space(5.0);
            let search_active = app.settings.workspace.bottom_visible && app.bottom_tab == 1;
            let search = rail_button(
                ui,
                "search",
                "Search in workspace",
                search_active,
                app.settings.appearance.reduce_motion,
            );
            if search.clicked() {
                if search_active {
                    app.settings.workspace.bottom_visible = false;
                } else {
                    app.settings.workspace.bottom_visible = true;
                    app.bottom_tab = 1;
                }
                app.settings_dirty = true;
            }
        });
}

fn rail_button(
    ui: &mut egui::Ui,
    id: &'static str,
    label: &str,
    active: bool,
    reduce_motion: bool,
) -> egui::Response {
    let ctx = ui.ctx().clone();
    let motion_time = if reduce_motion { 0.0 } else { 0.16 };
    let t = ctx.animate_bool_with_time(egui::Id::new(("activity", id)), active, motion_time);
    let response = ui
        .add_sized(
            [36.0, 36.0],
            egui::Button::new("")
                .fill(egui::Color32::from_rgba_unmultiplied(
                    70,
                    70,
                    70,
                    (t * 180.0) as u8,
                ))
                .stroke(egui::Stroke::NONE),
        )
        .on_hover_text(label);
    let icon_color = if active {
        egui::Color32::from_rgb(160, 204, 104)
    } else {
        egui::Color32::from_rgb(218, 222, 228)
    };
    let center = response.rect.center();
    let stroke = egui::Stroke::new(1.7, icon_color);
    if id == "explorer" {
        let left = center.x - 7.5;
        let right = center.x + 7.5;
        let top = center.y - 8.0;
        let bottom = center.y + 8.0;
        ui.painter().add(egui::Shape::line(
            vec![
                egui::pos2(left, top + 2.0),
                egui::pos2(left + 2.0, top),
                egui::pos2(right - 2.0, top),
                egui::pos2(right, top + 2.0),
                egui::pos2(right, bottom - 2.0),
                egui::pos2(right - 2.0, bottom),
                egui::pos2(left + 2.0, bottom),
                egui::pos2(left, bottom - 2.0),
                egui::pos2(left, top + 2.0),
            ],
            stroke,
        ));
        ui.painter().line_segment(
            [egui::pos2(left, top + 4.5), egui::pos2(right, top + 4.5)],
            stroke,
        );
        ui.painter().line_segment(
            [
                egui::pos2(center.x - 3.0, top + 4.5),
                egui::pos2(center.x - 3.0, bottom),
            ],
            stroke,
        );
    } else {
        ui.painter()
            .circle_stroke(center - egui::vec2(1.5, 1.5), 5.5, stroke);
        ui.painter().line_segment(
            [center + egui::vec2(2.5, 2.5), center + egui::vec2(7.0, 7.0)],
            stroke,
        );
    }
    if active {
        let accent = egui::Color32::from_rgb(112, 164, 51);
        let marker = egui::Rect::from_min_size(
            egui::pos2(response.rect.left() - 4.0, response.rect.top() + 5.0),
            egui::vec2(2.0, response.rect.height() - 10.0),
        );
        ui.painter().rect_filled(marker, 1.0, accent);
    }
    response
}
