//! Command palette (Ctrl+Shift+P) and quick-open (Ctrl+P).
//!
//! One overlay, two modes: command search over `Action::all()` (fuzzy) and
//! file search over the bounded workspace index. Keyboard driven: arrows,
//! Enter, Escape.

use eframe::egui;

use crate::actions::Action;
use crate::app::{PaletteMode, RavenApp};
use crate::workspace::fuzzy_score;

#[derive(Clone)]
enum PaletteItem {
    Action(Action),
    File(std::path::PathBuf),
}

pub fn handle_palette_keys(app: &mut RavenApp, ctx: &egui::Context) {
    let down = ctx.input(|input| input.key_pressed(egui::Key::ArrowDown));
    let up = ctx.input(|input| input.key_pressed(egui::Key::ArrowUp));
    let enter = ctx.input(|input| input.key_pressed(egui::Key::Enter) || false);
    let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    let tab = ctx.input(|input| input.key_pressed(egui::Key::Tab));

    if escape {
        app.palette.open = false;
        return;
    }
    if tab {
        // Toggle between command and file modes, preserving the query.
        app.palette.mode = match app.palette.mode {
            PaletteMode::Commands => PaletteMode::QuickOpen,
            PaletteMode::QuickOpen => PaletteMode::Commands,
        };
        app.palette.selected = 0;
        return;
    }
    let count = palette_items(app).len();
    if count == 0 {
        return;
    }
    if down {
        app.palette.selected = (app.palette.selected + 1).min(count - 1);
    }
    if up {
        app.palette.selected = app.palette.selected.saturating_sub(1);
    }
    if enter {
        let items = palette_items(app);
        if let Some(item) = items.get(app.palette.selected) {
            let item = item.clone();
            app.palette.open = false;
            execute(app, item);
        }
    }
}

pub fn show(app: &mut RavenApp, ctx: &egui::Context) {
    if !app.palette.open {
        return;
    }

    let items = palette_items(app);
    // Clamp selection when the list shrank.
    if app.palette.selected >= items.len() {
        app.palette.selected = items.len().saturating_sub(1);
    }

    egui::Area::new(egui::Id::new("raven-palette"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(
                    app.colors().overlay_bg[0],
                    app.colors().overlay_bg[1],
                    app.colors().overlay_bg[2],
                    255,
                ))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(
                        app.colors().border[0],
                        app.colors().border[1],
                        app.colors().border[2],
                        255,
                    ),
                ))
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_min_width(480.0);
                    ui.set_max_width(560.0);

                    let mode_hint = match app.palette.mode {
                        PaletteMode::Commands => "commands — Tab: files",
                        PaletteMode::QuickOpen => "files — Tab: commands",
                    };
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(mode_hint).weak().small());
                    });

                    let response = egui::TextEdit::singleline(&mut app.palette.query)
                        .hint_text("type to search…")
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::proportional(15.0))
                        .id(egui::Id::new("palette-input"))
                        .show(ui);

                    // Keep typing focus in the query field.
                    let input_id = response.response.id;
                    ctx.memory_mut(|memory| memory.request_focus(input_id));

                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            if items.is_empty() {
                                ui.weak("no matches");
                            }
                            for (index, item) in items.iter().enumerate() {
                                let selected = index == app.palette.selected;
                                let label = match item {
                                    PaletteItem::Action(action) => {
                                        let chord = action
                                            .chord(&app.settings)
                                            .map(|chord| chord.label())
                                            .unwrap_or_default();
                                        format!("{}   {}", action.label(), chord)
                                    }
                                    PaletteItem::File(path) => path.display().to_string(),
                                };
                                let text = egui::RichText::new(label).size(13.0);
                                if ui.selectable_label(selected, text).clicked() {
                                    app.palette.open = false;
                                    let item = item.clone();
                                    execute(app, item);
                                    return;
                                }
                            }
                        });

                    // Enter/Escape/arrows are handled globally in
                    // `handle_palette_keys` before this overlay draws.
                });
        });
}

/// Builds the filtered item list for the current mode.
fn palette_items(app: &RavenApp) -> Vec<PaletteItem> {
    let query = app.palette.query.trim().to_lowercase();
    let mut items = Vec::new();

    match app.palette.mode {
        PaletteMode::Commands => {
            for action in Action::all() {
                let label = action.label().to_lowercase();
                if query.is_empty() {
                    items.push(PaletteItem::Action(*action));
                } else if let Some(score) = fuzzy_score(&label, &query) {
                    items.push(PaletteItem::Action(*action));
                    let _ = score;
                }
            }
            if items.len() > 50 {
                items.truncate(50);
            }
        }
        PaletteMode::QuickOpen => {
            if query.is_empty() {
                // Recent files first, then nothing else (index is large).
                for path in app.settings.workspace.recent_files.iter().take(12) {
                    items.push(PaletteItem::File(std::path::PathBuf::from(path)));
                }
            } else {
                for (_score, path) in app.file_index.search(&query, 40) {
                    items.push(PaletteItem::File(path));
                }
                // Also allow opening recents that match by name.
                for path in &app.settings.workspace.recent_files {
                    let name = std::path::Path::new(path)
                        .file_name()
                        .map(|name| name.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    if name.contains(&query) {
                        items.push(PaletteItem::File(std::path::PathBuf::from(path)));
                    }
                }
            }
        }
    }
    items
}

fn execute(app: &mut RavenApp, item: PaletteItem) {
    match item {
        PaletteItem::Action(action) => app.run_action(action),
        PaletteItem::File(path) => {
            app.open_file(&path);
        }
    }
}
