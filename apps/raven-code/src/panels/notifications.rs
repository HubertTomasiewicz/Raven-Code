//! Non-blocking bottom-right notifications and the session notification center.

use eframe::egui;

use crate::app::{RavenApp, Toast, ToastLevel};

pub fn show(app: &mut RavenApp, ctx: &egui::Context) {
    let now = std::time::Instant::now();
    let elapsed = now.saturating_duration_since(app.last_notification_frame);
    app.last_notification_frame = now;
    let max_visible = app.settings.notifications.max_visible as usize;
    let mut dismiss = Vec::new();
    let mut hovered = Vec::new();

    if !app.notifications.is_empty() {
        egui::Area::new(egui::Id::new("raven-notification-stack"))
            // Keep the cards well clear of the status bar and avoid placing
            // them below a clipped central-panel region on smaller windows.
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-14.0, 54.0))
            .order(egui::Order::Foreground)
            .constrain(true)
            .show(ctx, |ui| {
                ui.set_max_width(390.0);
                for toast in app.notifications.iter().rev().take(max_visible) {
                    let frame = egui::Frame::popup(ui.style())
                        .fill(egui::Color32::from_rgb(31, 35, 42))
                        .stroke(egui::Stroke::new(1.0, level_color(ui, toast.level)))
                        .inner_margin(egui::Margin::symmetric(12, 9));
                    let response = frame.show(ui, |ui| toast_contents(ui, toast, &mut dismiss));
                    if response.response.hovered() {
                        hovered.push(toast.id);
                    }
                    ui.add_space(5.0);
                }
            });
    }

    if !hovered.is_empty() {
        for toast in &mut app.notifications {
            if hovered.contains(&toast.id) {
                toast.created += elapsed;
            }
        }
    }
    let duration = std::time::Duration::from_millis(app.settings.notifications.duration_ms);
    app.notifications.retain(|toast| {
        !dismiss.contains(&toast.id) && (toast.persistent || toast.created.elapsed() < duration)
    });

    if app.notifications_open {
        let mut open = true;
        let mut settings_changed = false;
        let mut send_test = false;
        egui::Window::new("Notification Center")
            .open(&mut open)
            .resizable(true)
            .default_size(egui::vec2(420.0, 360.0))
            .show(ctx, |ui| {
                settings_changed |= ui
                    .checkbox(
                        &mut app.settings.notifications.enabled,
                        "Enable notifications",
                    )
                    .changed();
                ui.horizontal(|ui| {
                    settings_changed |= ui
                        .checkbox(
                            &mut app.settings.notifications.save_success,
                            "Save notifications",
                        )
                        .changed();
                    settings_changed |= ui
                        .checkbox(
                            &mut app.settings.notifications.build_results,
                            "Build results",
                        )
                        .changed();
                });
                settings_changed |= ui
                    .checkbox(
                        &mut app.settings.appearance.reduce_motion,
                        "Reduce nonessential motion",
                    )
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Notification duration (ms)");
                    settings_changed |= ui
                        .add(
                            egui::DragValue::new(&mut app.settings.notifications.duration_ms)
                                .range(1_500..=30_000)
                                .speed(250),
                        )
                        .changed();
                });
                if ui.button("Send test notification").clicked() {
                    send_test = true;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Recent activity").weak());
                    if ui.button("Clear history").clicked() {
                        app.notification_history.clear();
                    }
                });
                ui.separator();
                if app.notification_history.is_empty() {
                    ui.weak("No notifications yet.");
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for toast in app.notification_history.iter().rev() {
                            history_row(ui, toast);
                        }
                    });
                }
            });
        app.notifications_open = open;
        app.settings_dirty |= settings_changed;
        if send_test {
            app.push_notification(
                "Notifications are working.".to_string(),
                ToastLevel::Success,
                false,
            );
        }
    }
}

fn toast_contents(ui: &mut egui::Ui, toast: &Toast, dismiss: &mut Vec<u64>) {
    let color = level_color(ui, toast.level);
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(9.0, 22.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.0, color);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(level_label(toast.level))
                    .small()
                    .strong()
                    .color(color),
            );
            let text = if toast.repeats > 1 {
                format!("{}  ×{}", toast.text, toast.repeats)
            } else {
                toast.text.clone()
            };
            ui.add(egui::Label::new(egui::RichText::new(text).small()).wrap());
        });
        if ui
            .small_button("×")
            .on_hover_text("Dismiss notification")
            .clicked()
        {
            dismiss.push(toast.id);
        }
    });
}

fn history_row(ui: &mut egui::Ui, toast: &Toast) {
    let color = level_color(ui, toast.level);
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 18.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.5, color);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(level_label(toast.level))
                    .small()
                    .strong()
                    .color(color),
            );
            ui.label(egui::RichText::new(&toast.text).small());
        });
    });
    ui.separator();
}

fn level_label(level: ToastLevel) -> &'static str {
    match level {
        ToastLevel::Info => "Info",
        ToastLevel::Success => "Success",
        ToastLevel::Warning => "Warning",
        ToastLevel::Error => "Error",
    }
}

fn level_color(ui: &egui::Ui, level: ToastLevel) -> egui::Color32 {
    match level {
        ToastLevel::Info => ui.visuals().hyperlink_color,
        ToastLevel::Success => egui::Color32::from_rgb(111, 187, 119),
        ToastLevel::Warning => egui::Color32::from_rgb(231, 174, 73),
        ToastLevel::Error => egui::Color32::from_rgb(235, 103, 92),
    }
}
