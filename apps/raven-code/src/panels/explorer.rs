//! File explorer sidebar: lazy directory tree with context-menu file
//! operations (new/rename/delete) and a workspace header with refresh.

use eframe::egui;

use raven_editor::language;

use crate::app::{Dialog, InputKind, RavenApp};

pub fn show(app: &mut RavenApp, ui: &mut egui::Ui) {
    let Some(workspace_root) = app
        .workspace
        .as_ref()
        .map(|workspace| workspace.root.clone())
    else {
        return;
    };

    egui::Panel::left("raven-explorer")
        .resizable(true)
        .default_size(220.0)
        .min_size(140.0)
        .show(ui, |ui| {
            ui.add_space(4.0);

            // Header: workspace name + actions.
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("EXPLORER").small().strong().color(
                    egui::Color32::from_rgba_unmultiplied(
                        app.colors().text_dim[0],
                        app.colors().text_dim[1],
                        app.colors().text_dim[2],
                        255,
                    ),
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let refresh = ui
                        .add_sized([22.0, 22.0], egui::Button::new(""))
                        .on_hover_text("Refresh (F5)");
                    let center = refresh.rect.center();
                    let refresh_stroke = egui::Stroke::new(1.4, ui.visuals().text_color());
                    ui.painter().circle_stroke(center, 5.2, refresh_stroke);
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(3.5, -4.1),
                            center + egui::vec2(5.8, -4.1),
                        ],
                        refresh_stroke,
                    );
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(5.8, -4.1),
                            center + egui::vec2(5.8, -1.8),
                        ],
                        refresh_stroke,
                    );
                    if refresh.clicked() {
                        app.run_action(crate::actions::Action::RefreshExplorer);
                    }
                    let plus_icon = crate::icons::add_file_texture(ui.ctx());
                    let add_file = ui.add_sized(
                        [22.0, 22.0],
                        egui::Button::image(
                            egui::Image::new(&plus_icon).fit_to_exact_size(egui::vec2(16.0, 16.0)),
                        ),
                    );
                    if add_file
                        .on_hover_text("New file at workspace root")
                        .clicked()
                    {
                        app.dialog = Some(Dialog::Input {
                            kind: InputKind::NewFile(workspace_root.clone()),
                            title: "New file (path relative to workspace root)".to_string(),
                            value: String::new(),
                        });
                    }
                });
            });
            ui.separator();

            let name = app
                .workspace
                .as_ref()
                .map(|workspace| workspace.display_name())
                .unwrap_or_default();
            ui.add(egui::Label::new(egui::RichText::new(&name).strong()).truncate());

            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.add_space(2.0);
                    let root = workspace_root.clone();
                    render_dir(app, ui, &root, &name, true, 0);
                });
        });
}

fn render_dir(
    app: &mut RavenApp,
    ui: &mut egui::Ui,
    path: &std::path::Path,
    label: &str,
    default_open: bool,
    depth: usize,
) {
    let id = egui::Id::new(("explorer", path));

    let response =
        egui::CollapsingHeader::new(egui::RichText::new(format!("   {label}")).size(13.0))
            .id_salt(id)
            .default_open(default_open)
            .show(ui, |ui| {
                // Body: read children lazily through the workspace cache.
                let Some(workspace) = app.workspace.as_mut() else {
                    return;
                };
                let listing = workspace.listing(path).clone();

                for (name, child) in &listing.dirs {
                    render_dir(app, ui, child, name, false, depth + 1);
                }
                for (name, child) in &listing.files {
                    render_file(app, ui, child, name, depth + 1);
                }
                if listing.dirs.is_empty() && listing.files.is_empty() {
                    ui.weak(egui::RichText::new("(empty)").small());
                }
            });

    let folder_icon = crate::icons::folder_texture(ui.ctx(), response.openness > 0.5);
    let header_rect = response.header_response.rect;
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(header_rect.left() + 20.0, header_rect.center().y - 8.0),
        egui::vec2(16.0, 16.0),
    );
    ui.painter().image(
        folder_icon.id(),
        icon_rect,
        egui::Rect::EVERYTHING,
        egui::Color32::WHITE,
    );

    // Context menu for directories.
    response.header_response.context_menu(|ui| {
        let path = path.to_path_buf();
        if ui.button("New file here…").clicked() {
            app.dialog = Some(Dialog::Input {
                kind: InputKind::NewFile(path.clone()),
                title: format!("New file inside {}", path.display()),
                value: String::new(),
            });
        }
        if ui.button("New folder here…").clicked() {
            app.dialog = Some(Dialog::Input {
                kind: InputKind::NewFolder(path.clone()),
                title: format!("New folder inside {}", path.display()),
                value: String::new(),
            });
        }
        if depth > 0 && ui.button("Rename…").clicked() {
            app.dialog = Some(Dialog::Input {
                kind: InputKind::Rename(path.clone()),
                title: format!("Rename {}", path.display()),
                value: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            });
        }
        if depth > 0 && ui.button("Delete…").clicked() {
            app.dialog = Some(Dialog::ConfirmDelete {
                path: path.clone(),
                is_dir: true,
            });
        }
        if ui.button("Refresh").clicked() {
            refresh(app, &path);
        }
    });
}

fn render_file(
    app: &mut RavenApp,
    ui: &mut egui::Ui,
    path: &std::path::Path,
    label: &str,
    depth: usize,
) {
    let _ = depth;
    let lang = language::for_path(path);
    let is_active = app
        .active_doc()
        .and_then(|doc| doc.doc.path())
        .is_some_and(|active| active == path);

    let mut text = String::new();
    if is_active {
        text.push_str("▶ ");
    }
    text.push_str(label);
    let mut rich = egui::RichText::new(text).size(13.0);
    if is_active {
        rich = rich.color(egui::Color32::from_rgba_unmultiplied(
            app.colors().accent[0],
            app.colors().accent[1],
            app.colors().accent[2],
            255,
        ));
    }

    let response = ui
        .horizontal(|ui| {
            let icon = crate::icons::file_texture(ui.ctx(), path);
            ui.add(egui::Image::new(&icon).fit_to_exact_size(egui::vec2(16.0, 16.0)));
            ui.selectable_label(is_active, rich)
        })
        .inner;
    if response.clicked() {
        app.open_file(path);
    }
    response.context_menu(|ui| {
        let path = path.to_path_buf();
        if ui.button("Open").clicked() {
            app.open_file(&path);
        }
        if ui.button("Rename…").clicked() {
            app.dialog = Some(Dialog::Input {
                kind: InputKind::Rename(path.clone()),
                title: format!("Rename {}", path.display()),
                value: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            });
        }
        if ui.button("Delete…").clicked() {
            app.dialog = Some(Dialog::ConfirmDelete {
                path: path.clone(),
                is_dir: false,
            });
        }
        ui.separator();
        ui.weak(egui::RichText::new(lang.name).small());
    });
}

fn refresh(app: &mut RavenApp, path: &std::path::Path) {
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
