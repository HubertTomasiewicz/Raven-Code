//! Modal dialogs: unsaved-changes confirmation, Save As, generic input
//! (new file/folder, rename, go to line, run command), About, shortcuts
//! and the manual open-path fallback for systems without a dialog backend.
//!
//! Ownership model: `show` *takes* the dialog out of the app, mutates the
//! dialog's text fields in place during the frame, and puts the dialog back
//! when no terminal button was pressed — so typed text always survives.

use eframe::egui;

use crate::actions::Action;
use crate::app::{Dialog, InputKind, PendingClose, RavenApp};
use crate::icons;

pub fn show(app: &mut RavenApp, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.take() else {
        return;
    };
    match dialog {
        Dialog::UnsavedConfirm {
            doc_ids,
            pending,
            error,
        } => {
            unsaved_confirm(app, ctx, doc_ids, pending, error);
        }
        Dialog::ConfirmDelete { path, is_dir } => {
            confirm_delete(app, ctx, path, is_dir);
        }
        Dialog::Input {
            kind,
            title,
            mut value,
        } => {
            input_dialog(app, ctx, kind, title, &mut value);
        }
        Dialog::About => about(app, ctx),
        Dialog::Shortcuts => shortcuts(app, ctx),
        Dialog::OpenPath { kind, mut value } => {
            open_path(app, ctx, kind, &mut value);
        }
    }
}

fn confirm_delete(app: &mut RavenApp, ctx: &egui::Context, path: std::path::PathBuf, is_dir: bool) {
    let open_docs: Vec<u64> = app
        .docs
        .values()
        .filter(|doc| {
            doc.doc
                .path()
                .is_some_and(|open| open == path || (is_dir && open.starts_with(&path)))
        })
        .map(|doc| doc.id)
        .collect();
    let dirty_names: Vec<String> = open_docs
        .iter()
        .filter_map(|id| {
            app.docs
                .get(id)
                .filter(|doc| doc.modified)
                .map(|doc| doc.display_name())
        })
        .collect();
    let mut confirm = false;
    let mut cancel = false;

    modal(
        app,
        ctx,
        "delete-confirm-dialog",
        "Delete file or folder?",
        |ui| {
            ui.label(format!("Permanently delete {}?", path.display()));
            if !dirty_names.is_empty() {
                ui.add_space(6.0);
                ui.colored_label(
                    egui::Color32::from_rgb(255, 190, 90),
                    "Save or close these modified files before deleting:",
                );
                for name in &dirty_names {
                    ui.label(format!("  • {name}"));
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(dirty_names.is_empty(), egui::Button::new("Delete"))
                    .clicked()
                {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        },
    );

    if confirm {
        let result = if is_dir {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match result {
            Ok(()) => {
                for id in open_docs {
                    if let Some(pane) = app.panes.iter().position(|pane| pane.tabs.contains(&id)) {
                        app.do_close_tab(pane, id);
                    }
                }
                refresh_after_fs_change(app, path.parent().unwrap_or(std::path::Path::new(".")));
                app.set_notice(format!("deleted {}", path.display()), false);
            }
            Err(error) => app.set_notice(format!("delete failed: {error}"), true),
        }
    } else if !cancel {
        app.dialog = Some(Dialog::ConfirmDelete { path, is_dir });
    }
}

fn modal(
    app: &RavenApp,
    ctx: &egui::Context,
    id: &str,
    title: &str,
    content: impl FnOnce(&mut egui::Ui),
) -> bool {
    let colors = app.colors();
    let fill = egui::Color32::from_rgba_unmultiplied(
        colors.overlay_bg[0],
        colors.overlay_bg[1],
        colors.overlay_bg[2],
        255,
    );
    let stroke = egui::Stroke::new(
        1.0,
        egui::Color32::from_rgba_unmultiplied(
            colors.border[0],
            colors.border[1],
            colors.border[2],
            255,
        ),
    );

    let mut closed = false;
    egui::Window::new(egui::RichText::new(title).strong())
        .id(egui::Id::new(id))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .order(egui::Order::Foreground)
        .frame(
            egui::Frame::new()
                .fill(fill)
                .stroke(stroke)
                .inner_margin(14.0),
        )
        .show(ctx, |ui| {
            content(ui);
            closed = true;
        });
    closed
}

fn unsaved_confirm(
    app: &mut RavenApp,
    ctx: &egui::Context,
    doc_ids: Vec<u64>,
    pending: PendingClose,
    error: Option<String>,
) {
    let names: Vec<String> = doc_ids
        .iter()
        .filter_map(|id| app.docs.get(id))
        .map(|doc| doc.display_name())
        .collect();

    #[derive(Clone, Copy)]
    enum Choice {
        Save,
        Discard,
        Cancel,
    }
    let (save_label, discard_label, title) = match &pending {
        PendingClose::Quit => ("Save All & Exit", "Exit Without Saving", "Unsaved changes"),
        PendingClose::CloseAll => (
            "Save All & Close",
            "Close Without Saving",
            "Unsaved changes",
        ),
        PendingClose::CloseFolder => (
            "Save & Close Workspace",
            "Close Workspace Without Saving",
            "Unsaved changes",
        ),
        PendingClose::CloseTab(_, _) => ("Save & Close", "Close Without Saving", "Unsaved changes"),
    };
    let mut choice: Option<Choice> = None;

    modal(app, ctx, "unsaved-dialog", title, |ui| {
        let count = names.len();
        ui.label(format!(
            "You have {count} unsaved file{}.",
            if count == 1 { "" } else { "s" }
        ));
        ui.label("Save changes before continuing?");
        if let Some(error) = &error {
            ui.add_space(5.0);
            ui.colored_label(egui::Color32::from_rgb(255, 155, 130), error);
        }
        ui.add_space(4.0);
        for name in &names {
            ui.label(
                egui::RichText::new(format!("  ● {name}"))
                    .monospace()
                    .small(),
            );
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(save_label).clicked() {
                choice = Some(Choice::Save);
            }
            if ui.button(discard_label).clicked() {
                choice = Some(Choice::Discard);
            }
            if ui.button("Cancel").clicked() {
                choice = Some(Choice::Cancel);
            }
        });
    });

    match choice {
        Some(Choice::Save) => app.save_for_pending_close(&doc_ids, pending),
        Some(Choice::Discard) => finish_pending(app, pending),
        Some(Choice::Cancel) | None => {}
    }
}

fn finish_pending(app: &mut RavenApp, pending: PendingClose) {
    app.finish_pending_close(pending);
}

fn input_dialog(
    app: &mut RavenApp,
    ctx: &egui::Context,
    kind: InputKind,
    title: String,
    value: &mut String,
) {
    let mut submitted = false;
    let mut cancelled = false;

    modal(app, ctx, "input-dialog", &title, |ui| {
        let field = egui::TextEdit::singleline(value)
            .desired_width(420.0)
            .font(egui::FontId::monospace(13.0))
            .id(egui::Id::new("input-dialog-field"))
            .show(ui);
        if field.response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            submitted = true;
        }
        ui.memory_mut(|memory| memory.request_focus(field.response.id));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                submitted = true;
            }
            if ui.button("Cancel").clicked() {
                cancelled = true;
            }
        });
    });

    if submitted {
        let value = value.trim().to_string();
        apply_input(app, ctx, kind, &value);
    } else if !cancelled {
        app.dialog = Some(Dialog::Input {
            kind,
            title,
            value: value.clone(),
        });
    }
}

fn apply_input(app: &mut RavenApp, ctx: &egui::Context, kind: InputKind, value: &str) {
    match kind {
        InputKind::NewFile(base) => {
            let path = resolve_path(&base, value);
            match create_file(&path) {
                Ok(()) => {
                    app.open_file(&path);
                    app.set_notice(format!("created {}", path.display()), false);
                }
                Err(error) => app.set_notice(format!("could not create file: {error}"), true),
            }
        }
        InputKind::NewFolder(base) => {
            let path = resolve_path(&base, value);
            match std::fs::create_dir_all(&path) {
                Ok(()) => {
                    refresh_after_fs_change(app, &base);
                    app.set_notice(format!("created {}", path.display()), false);
                }
                Err(error) => app.set_notice(format!("could not create folder: {error}"), true),
            }
        }
        InputKind::Rename(old) => {
            if value.is_empty() {
                app.set_notice("rename: empty name", true);
                return;
            }
            let target = old
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join(value);
            match std::fs::rename(&old, &target) {
                Ok(()) => {
                    // Reopen the doc under its new path if it was open.
                    let open_ids: Vec<u64> = app
                        .docs
                        .values()
                        .filter(|doc| doc.doc.path() == Some(old.as_path()))
                        .map(|doc| doc.id)
                        .collect();
                    for id in open_ids {
                        let pane = app
                            .panes
                            .iter()
                            .position(|pane| pane.tabs.contains(&id))
                            .unwrap_or(0);
                        app.do_close_tab(pane, id);
                        app.open_file(&target);
                    }
                    refresh_after_fs_change(app, old.parent().unwrap_or(std::path::Path::new(".")));
                    app.set_notice(format!("renamed to {}", target.display()), false);
                }
                Err(error) => app.set_notice(format!("rename failed: {error}"), true),
            }
        }
        InputKind::GoToLine => {
            let Ok(line_number) = value.parse::<usize>() else {
                app.set_notice(format!("'{value}' is not a line number"), true);
                return;
            };
            let Some(id) = app.active_doc_id() else {
                app.set_notice("no open document", true);
                return;
            };
            let line = line_number.saturating_sub(1);
            app.jump_to_line(ctx, id, line);
        }
        InputKind::RunCommand => {
            let trimmed = value;
            if trimmed.is_empty() {
                return;
            }
            let mut parts = trimmed.split_whitespace();
            let Some(program) = parts.next() else {
                return;
            };
            let args: Vec<String> = parts.map(String::from).collect();
            let cwd = app
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            app.settings.workspace.bottom_visible = true;
            app.bottom_tab = 0;
            app.jobs.start_command(program.to_string(), args, cwd);
        }
    }
}

fn create_file(path: &std::path::Path) -> std::io::Result<()> {
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "file already exists",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, b"")
}

fn resolve_path(base: &std::path::Path, value: &str) -> std::path::PathBuf {
    let candidate = std::path::PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        base.join(candidate)
    }
}

fn refresh_after_fs_change(app: &mut RavenApp, path: &std::path::Path) {
    if let Some(workspace) = app.workspace.as_mut() {
        workspace.refresh(path);
    }
    if let Some(root) = app
        .workspace
        .as_ref()
        .map(|workspace| workspace.root.clone())
    {
        app.file_index.scan(root);
    }
}

fn about(app: &mut RavenApp, ctx: &egui::Context) {
    let mut closed = false;
    modal(app, ctx, "about-dialog", "About Raven Code", |ui| {
        ui.horizontal(|ui| {
            let texture = icons::brand_texture(ui.ctx());
            ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(
                texture.id(),
                [72.0, 72.0],
            )));
            ui.vertical(|ui| {
                ui.heading("Raven Code");
                ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
                ui.weak("The Raven ecosystem's lightweight code and text editor.");
                ui.weak("Built on the Raven Pico SDK (raven-core, raven-projects, raven-build).");
            });
        });
        ui.add_space(6.0);
        ui.separator();
        ui.label(
            egui::RichText::new(
                "Raven Code works on its own: no compiler, language server or\n\
                 Raven Studio installation is required. Language servers are\n\
                 optional external processes configured in settings.",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);
        if ui.button("Close").clicked() {
            closed = true;
        }
    });
    if !closed {
        app.dialog = Some(Dialog::About);
    }
}

fn shortcuts(app: &mut RavenApp, ctx: &egui::Context) {
    let mut closed = false;
    modal(app, ctx, "shortcuts-dialog", "Keyboard Shortcuts", |ui| {
        egui::ScrollArea::vertical()
            .max_height(420.0)
            .show(ui, |ui| {
                egui::Grid::new("shortcuts-grid")
                    .num_columns(2)
                    .spacing([16.0, 3.0])
                    .show(ui, |ui| {
                        for action in Action::all() {
                            if let Some(chord) = action.chord(&app.settings) {
                                ui.label(egui::RichText::new(chord.label()).monospace().small());
                                ui.label(egui::RichText::new(action.label()).small());
                                ui.end_row();
                            }
                        }
                    });
                ui.add_space(6.0);
                ui.weak("Rebind by action id under [keys] in raven-code.toml (e.g. save = \"Ctrl+S\").");
                ui.weak("Editor-native: Ctrl+Z/Ctrl+Y undo/redo, Ctrl+X/C/V clipboard, Ctrl+A select all,");
                ui.weak("Tab / Shift+Tab indent, F12 definition, Ctrl+Space completion (needs a configured LSP server).");
            });
        ui.add_space(8.0);
        if ui.button("Close").clicked() {
            closed = true;
        }
    });
    if !closed {
        app.dialog = Some(Dialog::Shortcuts);
    }
}

fn open_path(
    app: &mut RavenApp,
    ctx: &egui::Context,
    kind: crate::config_paths::PickKind,
    value: &mut String,
) {
    let mut confirmed = false;
    let mut cancelled = false;
    let title = match kind {
        crate::config_paths::PickKind::File => "Open File (path)",
        crate::config_paths::PickKind::Folder => "Open Folder (path)",
    };

    modal(app, ctx, "open-path-dialog", title, |ui| {
        ui.weak("The native file dialog is unavailable on this system — enter a path.");
        let field = egui::TextEdit::singleline(value)
            .desired_width(420.0)
            .font(egui::FontId::monospace(13.0))
            .id(egui::Id::new("open-path-field"))
            .show(ui);
        if field.response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            confirmed = true;
        }
        ui.memory_mut(|memory| memory.request_focus(field.response.id));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Open").clicked() {
                confirmed = true;
            }
            if ui.button("Cancel").clicked() {
                cancelled = true;
            }
        });
    });

    if confirmed {
        let path = std::path::PathBuf::from(value.trim());
        if path.exists() {
            if path.is_dir() && kind == crate::config_paths::PickKind::Folder {
                app.open_folder(&path);
            } else {
                app.open_file(&path);
            }
        } else {
            app.set_notice(format!("path does not exist: {}", path.display()), true);
            app.dialog = Some(Dialog::OpenPath {
                kind,
                value: value.clone(),
            });
        }
    } else if !cancelled {
        app.dialog = Some(Dialog::OpenPath {
            kind,
            value: value.clone(),
        });
    }
}
