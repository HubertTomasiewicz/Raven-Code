//! Top menu bar. Every item dispatches through `RavenApp::run_action`, so
//! menus, shortcuts and the command palette stay perfectly consistent.

use eframe::egui;

use crate::actions::Action;
use crate::app::RavenApp;

pub fn show(app: &mut RavenApp, ui: &mut egui::Ui) {
    egui::Panel::top("raven-menu-bar").show(ui, |ui| {
        ui.add_space(2.0);
        egui::MenuBar::new().ui(ui, |ui| {
            let chord_label = |app: &RavenApp, action: Action| {
                action.chord(&app.settings).map(|chord| chord.label())
            };

            ui.menu_button("File", |ui| {
                ui.set_min_width(320.0);
                menu_item(ui, app, Action::NewFile);
                menu_item(ui, app, Action::OpenFile);
                menu_item(ui, app, Action::OpenFolder);
                menu_item(ui, app, Action::OpenWorkspace);
                menu_item(ui, app, Action::OpenPath);
                ui.separator();
                recent_menu(ui, app);
                ui.separator();
                menu_item(ui, app, Action::Save);
                menu_item(ui, app, Action::SaveAs);
                menu_item(ui, app, Action::SaveWorkspaceAs);
                menu_item(ui, app, Action::SaveAll);
                ui.separator();
                menu_item(ui, app, Action::CloseTab);
                menu_item(ui, app, Action::CloseAllTabs);
                menu_item(ui, app, Action::CloseFolder);
                ui.separator();
                menu_item(ui, app, Action::Quit);
            });

            ui.menu_button("Edit", |ui| {
                ui.set_min_width(320.0);
                menu_item(ui, app, Action::Undo);
                menu_item(ui, app, Action::Redo);
                ui.separator();
                menu_item(ui, app, Action::Cut);
                menu_item(ui, app, Action::Copy);
                menu_item(ui, app, Action::Paste);
                menu_item(ui, app, Action::SelectAll);
                menu_item(ui, app, Action::FormatJson);
                ui.separator();
                menu_item(ui, app, Action::Find);
                menu_item(ui, app, Action::Replace);
            });

            ui.menu_button("View", |ui| {
                ui.set_min_width(320.0);
                menu_item(ui, app, Action::CommandPalette);
                ui.separator();
                menu_item(ui, app, Action::ToggleSidebar);
                menu_item(ui, app, Action::ToggleBottomPanel);
                toggle_item(ui, "Word Wrap", app.settings.editor.word_wrap, || {
                    app.run_action(Action::ToggleWordWrap)
                });
                menu_item(ui, app, Action::SplitEditor);
                menu_item(ui, app, Action::TogglePreview);
                ui.separator();
                menu_item(ui, app, Action::ZoomIn);
                menu_item(ui, app, Action::ZoomOut);
                menu_item(ui, app, Action::ResetZoom);
                ui.separator();
                theme_menu(ui, app);
                menu_item(ui, app, Action::RefreshExplorer);
            });

            ui.menu_button("Go", |ui| {
                ui.set_min_width(320.0);
                menu_item(ui, app, Action::QuickOpen);
                menu_item(ui, app, Action::GoToLine);
                ui.separator();
                menu_item(ui, app, Action::NextTab);
                menu_item(ui, app, Action::PreviousTab);
            });

            ui.menu_button("Run", |ui| {
                ui.set_min_width(320.0);
                menu_item(ui, app, Action::BuildProject);
                menu_item(ui, app, Action::TestProject);
                menu_item(ui, app, Action::RunProject);
                ui.separator();
                menu_item(ui, app, Action::RunCommand);
                ui.separator();
                menu_item(ui, app, Action::OpenTerminal);
                menu_item(ui, app, Action::CancelJob);
                menu_item(ui, app, Action::ClearOutput);
            });

            ui.menu_button("Help", |ui| {
                ui.set_min_width(320.0);
                menu_item(ui, app, Action::Shortcuts);
                menu_item(ui, app, Action::About);
                ui.separator();
                if ui.button("Open User Syntaxes Folder…").clicked() {
                    let folder = app.settings_dir.join("syntaxes");
                    match std::fs::create_dir_all(&folder)
                        .map_err(|error| error.to_string())
                        .and_then(|()| crate::config_paths::open_in_file_manager(&folder))
                    {
                        Ok(()) => app.set_notice(
                            format!(
                                "Add .sublime-syntax files to {} and restart Raven Code",
                                folder.display()
                            ),
                            false,
                        ),
                        Err(error) => app.set_notice(error, true),
                    }
                }
            });

            let _ = chord_label;
        });
        ui.add_space(2.0);
    });
}

fn menu_item(ui: &mut egui::Ui, app: &mut RavenApp, action: Action) {
    let label = action.label();
    let chord = action.chord(&app.settings).map(|chord| chord.label());
    let button = ui.add_sized([320.0, 24.0], egui::Button::new(format!("{label}\u{2003}")));
    if button.clicked() {
        app.run_action(action);
    }
    if let Some(chord) = chord {
        // Right-aligned hint inside the same row is complex in egui menus;
        // show the chord inline instead.
        if ui.is_rect_visible(button.rect) {
            let hint = egui::RichText::new(chord).weak().small();
            ui.painter().text(
                button.rect.right_top() - egui::vec2(6.0, -4.0),
                egui::Align2::RIGHT_TOP,
                hint.text(),
                egui::FontId::proportional(11.0),
                ui.visuals().widgets.noninteractive.fg_stroke.color,
            );
        }
    }
}

fn toggle_item(ui: &mut egui::Ui, label: &str, value: bool, toggle: impl FnOnce()) {
    let mark = if value { "✓" } else { " " };
    if ui.button(format!("{mark}  {label}")).clicked() {
        toggle();
    }
}

fn recent_menu(ui: &mut egui::Ui, app: &mut RavenApp) {
    ui.menu_button("Open Recent", |ui| {
        let files = app.settings.workspace.recent_files.clone();
        let folders = app.settings.workspace.recent_folders.clone();
        if files.is_empty() && folders.is_empty() {
            ui.weak("no recent files or folders");
            return;
        }
        if !folders.is_empty() {
            ui.label(egui::RichText::new("Folders").weak().small());
            for folder in folders.iter().take(5) {
                if ui.button(folder).clicked() {
                    app.open_folder(std::path::Path::new(folder));
                }
            }
            ui.separator();
        }
        if !files.is_empty() {
            ui.label(egui::RichText::new("Files").weak().small());
            for file in files.iter().take(8) {
                if ui.button(file).clicked() {
                    app.open_file(std::path::Path::new(file));
                }
            }
        }
    });
}

fn theme_menu(ui: &mut egui::Ui, app: &mut RavenApp) {
    ui.menu_button("Theme", |ui| {
        if ui.button("Raven Dark").clicked() {
            app.apply_theme_by_id("raven-dark");
        }
        if ui.button("Raven Light").clicked() {
            app.apply_theme_by_id("raven-light");
        }
        ui.separator();
        if ui.button("Toggle Dark/Light (Ctrl+Shift+D)").clicked() {
            app.run_action(crate::actions::Action::ThemeToggle);
        }
    });
}
