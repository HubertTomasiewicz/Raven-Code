//! Status bar: workspace/project info, LSP status, diagnostics counters,
//! cursor position, encoding, line endings, language and file size.

use eframe::egui;

use raven_editor::buffer::LoadMode;

use crate::app::RavenApp;
use crate::panels;

pub fn show(app: &mut RavenApp, ui: &mut egui::Ui) {
    let colors = app.colors();
    let fill = egui::Color32::from_rgba_unmultiplied(
        colors.status_bg[0],
        colors.status_bg[1],
        colors.status_bg[2],
        255,
    );
    let dim = |rgba: [u8; 4]| egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], 255);

    egui::Panel::bottom("raven-status-bar")
        .resizable(false)
        .frame(
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(8, 3)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                // Transient notice (toasts) — errors in red, info dimmed.
                if let Some(notice) = &app.notice {
                    if notice.at.elapsed() < std::time::Duration::from_secs(4) {
                        let color = if notice.is_error {
                            colors.error
                        } else {
                            colors.accent
                        };
                        ui.label(egui::RichText::new(&notice.text).small().color(
                            egui::Color32::from_rgba_unmultiplied(
                                color[0], color[1], color[2], 255,
                            ),
                        ));
                    }
                }

                // Left: workspace + project kind.
                if let Some(workspace) = app.workspace.as_ref() {
                    ui.label(egui::RichText::new(workspace.display_name()).small());
                }
                match &app.project_info {
                    Some((kind, root)) => {
                        ui.label(
                            egui::RichText::new(format!("{kind} · {}", root.display()))
                                .small()
                                .color(dim(colors.accent)),
                        );
                    }
                    None if app.workspace.is_some() => {
                        ui.label(egui::RichText::new("detecting project…").small().weak());
                    }
                    None => {}
                }

                // LSP status.
                let lsp_status = app
                    .active_doc()
                    .map(|doc| app.lsp.status_for(Some(doc.language)))
                    .unwrap_or_default();
                if !lsp_status.is_empty() {
                    ui.label(egui::RichText::new(lsp_status).small().weak());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let active_notifications = app.notifications.len();
                    if ui
                        .small_button(format!("🔔 Notifications ({active_notifications})"))
                        .on_hover_text("Open notification history")
                        .clicked()
                    {
                        app.notifications_open = !app.notifications_open;
                    }
                    // Diagnostics counters.
                    let (errors, warnings) = diagnostic_counts(app);
                    if errors > 0 {
                        panels::status_dot(ui, colors.error);
                        ui.label(egui::RichText::new(errors.to_string()).small());
                    }
                    if warnings > 0 {
                        panels::status_dot(ui, colors.warning);
                        ui.label(egui::RichText::new(warnings.to_string()).small());
                    }

                    // Encoding / EOL / language. Extract document facts
                    // first so the immutable borrow ends before mutations.
                    let doc_facts = app.active_doc().map(|state| {
                        (
                            state.doc.encoding().display_name(),
                            state.doc.line_ending(),
                            state.language.name,
                            state.doc.mode,
                            panels::human_bytes(state.doc.encoded_size()),
                        )
                    });
                    if let Some((encoding_name, current_ending, language_name, mode, size)) =
                        doc_facts
                    {
                        ui.label(egui::RichText::new(encoding_name).small());
                        let clicked = ui
                            .label(egui::RichText::new(current_ending.as_str()).small())
                            .clicked();
                        if clicked {
                            let next = match current_ending {
                                raven_editor::buffer::LineEnding::Lf => {
                                    raven_editor::buffer::LineEnding::Crlf
                                }
                                raven_editor::buffer::LineEnding::Crlf => {
                                    raven_editor::buffer::LineEnding::Lf
                                }
                            };
                            let changed = {
                                let state = app.active_doc_mut().expect("checked");
                                let changed = state.doc.set_line_ending(next);
                                if changed {
                                    state.modified = true;
                                }
                                changed
                            };
                            if changed {
                                app.set_notice(
                                    format!("line endings set to {}", next.as_str()),
                                    false,
                                );
                            }
                        }
                        ui.label(egui::RichText::new(language_name).small());
                        if mode == LoadMode::Plain {
                            ui.label(
                                egui::RichText::new("large file: highlighting off")
                                    .small()
                                    .color(dim(colors.warning)),
                            );
                        } else if mode == LoadMode::ReadOnly {
                            ui.label(
                                egui::RichText::new("read-only")
                                    .small()
                                    .color(dim(colors.warning)),
                            );
                        }
                        ui.label(egui::RichText::new(size).small().weak());
                    }

                    // Cursor position.
                    let pane = app.active_pane();
                    let (line, col) = pane.cursor;
                    let selection = pane.selection_chars;
                    let position = if selection > 0 {
                        format!("Ln {}, Col {} ({} selected)", line + 1, col + 1, selection)
                    } else {
                        format!("Ln {}, Col {}", line + 1, col + 1)
                    };
                    ui.label(egui::RichText::new(position).small());

                    // Save state stays visible until the disk write succeeds.
                    if let Some(state) = app.active_doc() {
                        if state.modified {
                            if state.doc.path().is_some()
                                && app.settings.editor.autosave_delay_ms > 0
                            {
                                let delay = std::time::Duration::from_millis(
                                    app.settings.editor.autosave_delay_ms,
                                );
                                let remaining = delay.saturating_sub(state.last_edit_at.elapsed());
                                let label = if remaining <= std::time::Duration::from_millis(200) {
                                    "◌ Saving…".to_string()
                                } else {
                                    format!("◷ Auto-save in {:.1}s", remaining.as_secs_f32())
                                };
                                ui.label(
                                    egui::RichText::new(label)
                                        .small()
                                        .color(dim(colors.warning)),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("● Unsaved")
                                        .small()
                                        .color(dim(colors.accent)),
                                );
                            }
                        } else if let Some(saved_at) = state
                            .last_saved_at
                            .filter(|time| time.elapsed() < std::time::Duration::from_secs(3))
                        {
                            let label = if state.last_save_was_auto {
                                "✓ Autosaved"
                            } else {
                                "✓ Saved"
                            };
                            let _ = saved_at;
                            ui.label(
                                egui::RichText::new(label)
                                    .small()
                                    .color(dim(colors.success)),
                            );
                        }
                    }
                });
            });
        });
}

fn diagnostic_counts(app: &RavenApp) -> (usize, usize) {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    for diagnostics in app.lsp.diagnostics.values() {
        for diagnostic in diagnostics {
            if diagnostic.severity.is_error() {
                errors += 1;
            } else if diagnostic.severity.is_warning() {
                warnings += 1;
            }
        }
    }
    (errors, warnings)
}
