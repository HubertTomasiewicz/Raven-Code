//! Bottom panel: Output (run results), Search (find in files), Diagnostics
//! (LSP), Symbols (LSP document outline). Bounded buffers everywhere.

use std::sync::mpsc;
use std::time::Instant;

use eframe::egui;

use raven_editor::search::SearchQuery;

use crate::app::RavenApp;
use crate::jobs::{JobSnapshot, JobStatus, Stream};
use crate::panels;

/// Hard cap for find-in-files hits.
const SEARCH_HIT_CAP: usize = 1000;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: std::path::PathBuf,
    pub line: usize, // 0-based
    pub text: String,
}

pub fn show(app: &mut RavenApp, ui: &mut egui::Ui) {
    if !app.settings.workspace.bottom_visible {
        return;
    }
    let colors = app.colors();
    let fill = egui::Color32::from_rgba_unmultiplied(
        colors.panel_bg[0],
        colors.panel_bg[1],
        colors.panel_bg[2],
        255,
    );

    egui::Panel::bottom("raven-bottom-panel")
        .resizable(true)
        .default_size(200.0)
        .min_size(80.0)
        .frame(egui::Frame::new().fill(fill))
        .show(ui, |ui| {
            ui.add_space(3.0);
            // Tab selector row.
            ui.horizontal(|ui| {
                let tabs = ["Output", "Search", "Diagnostics", "Symbols"];
                for (index, label) in tabs.iter().enumerate() {
                    let selected = app.bottom_tab == index;
                    let text = egui::RichText::new(*label).small().strong();
                    if ui.selectable_label(selected, text).clicked() {
                        app.bottom_tab = index;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let _ctx = ui.ctx().clone();
                    match app.bottom_tab {
                        0 => output_actions(app, ui),
                        1 => search_actions(app, ui),
                        _ => {
                            if ui
                                .small_button("✕")
                                .on_hover_text("Hide panel (Ctrl+J)")
                                .clicked()
                            {
                                app.settings.workspace.bottom_visible = false;
                                app.settings_dirty = true;
                            }
                        }
                    }
                });
            });
            ui.separator();

            let ctx = ui.ctx().clone();
            match app.bottom_tab {
                0 => draw_output(app, ui),
                1 => draw_search(app, ui, &ctx),
                2 => draw_diagnostics(app, ui, &ctx),
                3 => draw_symbols(app, ui, &ctx),
                _ => {}
            }
        });
}

// ----------------------------------------------------------------- output ----

fn output_actions(app: &mut RavenApp, ui: &mut egui::Ui) {
    if app.jobs.any_running()
        && ui
            .button("■ Stop")
            .on_hover_text("Cancel running jobs")
            .clicked()
    {
        app.run_action(crate::actions::Action::CancelJob);
    }
    if ui.button("Clear").clicked() {
        app.jobs.buffer.clear();
    }
    if ui
        .small_button("✕")
        .on_hover_text("Hide panel (Ctrl+J)")
        .clicked()
    {
        app.settings.workspace.bottom_visible = false;
        app.settings_dirty = true;
    }
}

fn draw_output(app: &mut RavenApp, ui: &mut egui::Ui) {
    let (lines, jobs) = app.jobs.buffer.snapshot();
    let colors = app.colors();

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if !jobs.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for job in &jobs {
                        job_chip(ui, job, colors);
                    }
                });
                ui.separator();
            }
            if lines.is_empty() && jobs.is_empty() {
                ui.weak("No output yet — use Run ▸ Build/Test/Run (Ctrl+Shift+B / T) with a folder open.");
                ui.weak("Output streams from the Raven Pico SDK process layer; nothing blocks the editor.");
                return;
            }
            egui::Grid::new("output-grid")
                .num_columns(2)
                .spacing([8.0, 1.0])
                .show(ui, |ui| {
                    for line in &lines {
                        let time = line.at.format_time();
                        ui.label(
                            egui::RichText::new(time)
                                .small()
                                .monospace()
                                .color(egui::Color32::from_rgba_unmultiplied(
                                    colors.text_dim[0],
                                    colors.text_dim[1],
                                    colors.text_dim[2],
                                    255,
                                )),
                        );
                        let text = egui::RichText::new(&line.text).monospace().size(12.0);
                        let text = match line.stream {
                            Stream::Info => text,
                            Stream::Stdout => text,
                            Stream::Stderr => text.color(egui::Color32::from_rgba_unmultiplied(
                                colors.warning[0],
                                colors.warning[1],
                                colors.warning[2],
                                255,
                            )),
                            Stream::Error => text.color(egui::Color32::from_rgba_unmultiplied(
                                colors.error[0],
                                colors.error[1],
                                colors.error[2],
                                255,
                            )),
                        };
                        ui.add(egui::Label::new(text).selectable(true));
                        ui.end_row();
                    }
                });
        });
}

fn job_chip(ui: &mut egui::Ui, job: &JobSnapshot, colors: &raven_editor::theme::ThemeColors) {
    let (label, color) = match job.status {
        JobStatus::Running => ("RUNNING", colors.info),
        JobStatus::Succeeded => ("OK", colors.success),
        JobStatus::Failed => ("FAILED", colors.error),
        JobStatus::Cancelled => ("CANCELLED", colors.warning),
        JobStatus::LaunchFailed => ("LAUNCH FAILED", colors.error),
    };
    let chip = egui::RichText::new(format!("{label} · {}", job.title))
        .small()
        .monospace()
        .color(egui::Color32::from_rgba_unmultiplied(
            color[0], color[1], color[2], 255,
        ));
    ui.label(chip);
}

/// Time-of-day rendering for output lines.
trait TimeFormat {
    fn format_time(&self) -> String;
}

impl TimeFormat for Instant {
    fn format_time(&self) -> String {
        // Instant has no wall-clock; store the process start wall time and
        // offset. Good enough for a local output panel.
        let elapsed = self.elapsed();
        let secs = elapsed.as_secs() % 86400;
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    }
}

// ----------------------------------------------------------------- search ----

fn search_actions(app: &mut RavenApp, ui: &mut egui::Ui) {
    if app.file_search.running {
        ui.label(egui::RichText::new("searching…").small());
    } else if !app.file_search.results.is_empty() {
        ui.label(egui::RichText::new(format!("{} hits", app.file_search.results.len())).small());
    }
    if ui.button("Clear").clicked() {
        app.file_search.results.clear();
        app.file_search.query.clear();
    }
    if ui
        .small_button("✕")
        .on_hover_text("Hide panel (Ctrl+J)")
        .clicked()
    {
        app.settings.workspace.bottom_visible = false;
        app.settings_dirty = true;
    }
}

fn draw_search(app: &mut RavenApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let colors = app.colors();
    let mut start_search = false;

    ui.horizontal(|ui| {
        let response = egui::TextEdit::singleline(&mut app.file_search.query)
            .hint_text("find in files (workspace)")
            .desired_width(320.0)
            .id(egui::Id::new("find-in-files-input"))
            .show(ui);
        let enter =
            response.response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if enter {
            start_search = true;
            ui.memory_mut(|memory| memory.request_focus(egui::Id::new("find-in-files-input")));
        }
        toggle(
            ui,
            "Aa",
            app.file_search.case_sensitive,
            "Match case",
            |value| {
                app.file_search.case_sensitive = value;
            },
        );
        toggle(
            ui,
            ".*",
            app.file_search.regex,
            "Regular expression",
            |value| {
                app.file_search.regex = value;
            },
        );
        if ui.button("Search").clicked() {
            start_search = true;
        }
    });

    if start_search && !app.file_search.query.is_empty() {
        start_file_search(app);
    }

    if let Some(error) = &app.file_search.error {
        ui.label(
            egui::RichText::new(error)
                .small()
                .color(egui::Color32::from_rgba_unmultiplied(
                    colors.error[0],
                    colors.error[1],
                    colors.error[2],
                    255,
                )),
        );
    }

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if app.file_search.query.is_empty() {
                ui.weak("Type a query and press Enter. Searches the indexed workspace files (bounded: skips target/, .git/, …).");
                return;
            }
            let query = SearchQuery {
                text: app.file_search.query.clone(),
                case_sensitive: app.file_search.case_sensitive,
                whole_word: false,
                regex: app.file_search.regex,
            };
            let hits: Vec<SearchHit> = app.file_search.results.clone();
            let mut shown = 0usize;
            for hit in &hits {
                let ranges =
                    raven_editor::search::find_all(&hit.text, &query, 4).unwrap_or_default();
                if ranges.is_empty() {
                    continue;
                }
                let label = format!(
                    "{}:{}",
                    hit.path.display(),
                    hit.line + 1
                );
                let row = ui.horizontal(|ui| {
                    if ui.link(egui::RichText::new(label).monospace().small()).clicked() {
                        app.open_file(&hit.path);
                        if let Some(id) = app.active_doc_id() {
                            app.jump_to_line(ctx, id, hit.line);
                        }
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(hit.text.trim())
                                .monospace()
                                .small()
                                .weak(),
                        )
                        .selectable(true),
                    );
                });
                let _ = row;
                shown += 1;
                if shown >= 300 {
                    ui.weak("… showing first 300 result rows");
                    break;
                }
            }
            if shown == 0 && !app.file_search.running {
                ui.weak("no matches in the workspace index");
            }
        });
}

fn start_file_search(app: &mut RavenApp) {
    if app.workspace.is_none() {
        app.file_search.error = Some("open a folder to search it".to_string());
        return;
    }
    app.file_search.error = None;
    let indexed_files = app.file_index.files();
    let query = app.file_search.query.clone();
    let case_sensitive = app.file_search.case_sensitive;
    let regex = app.file_search.regex;
    let (sender, receiver) = mpsc::channel();

    app.file_search.running = true;
    app.file_search.receiver = Some(receiver);
    app.settings.workspace.bottom_visible = true;
    app.settings_dirty = true;

    std::thread::Builder::new()
        .name("raven-code-filesearch".into())
        .spawn(move || {
            let files = indexed_files;
            let mut hits = Vec::new();
            let compiled = SearchQuery {
                text: query.clone(),
                case_sensitive,
                whole_word: false,
                regex,
            }
            .compile();
            let Ok(matcher) = compiled else {
                let _ = sender.send(Vec::new());
                return;
            };
            'outer: for path in files {
                if hits.len() >= SEARCH_HIT_CAP {
                    break;
                }
                // Skip obvious binaries by NUL sniff.
                let Ok(metadata) = std::fs::metadata(&path) else {
                    continue;
                };
                if metadata.len() > 1_048_576 {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                if bytes.contains(&0) {
                    continue;
                }
                let Ok(content) = String::from_utf8(bytes) else {
                    continue;
                };
                for (index, line) in content.split('\n').enumerate() {
                    if matcher.is_match(line) {
                        hits.push(SearchHit {
                            path: path.clone(),
                            line: index,
                            text: line.chars().take(200).collect(),
                        });
                        if hits.len() >= SEARCH_HIT_CAP {
                            break 'outer;
                        }
                    }
                }
            }
            let _ = sender.send(hits);
        })
        .ok();
}

fn toggle(ui: &mut egui::Ui, label: &str, value: bool, hint: &str, set: impl FnOnce(bool)) {
    let selection = ui.style().visuals.selection.bg_fill;
    let button = egui::Button::new(egui::RichText::new(label).small()).fill(if value {
        egui::Color32::from_rgba_unmultiplied(selection.r(), selection.g(), selection.b(), 160)
    } else {
        egui::Color32::TRANSPARENT
    });
    if ui.add(button).on_hover_text(hint).clicked() {
        set(!value);
    }
}

// ------------------------------------------------------------ diagnostics ----

fn draw_diagnostics(app: &mut RavenApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let colors = app.colors();
    if app.lsp.diagnostics.is_empty() {
        ui.weak("No diagnostics. Language servers configured under [lsp.servers] in settings publish diagnostics here.");
        return;
    }
    let mut entries: Vec<(std::path::PathBuf, Vec<crate::lsp_bridge::UiDiagnostic>)> = app
        .lsp
        .diagnostics
        .iter()
        .map(|(path, diagnostics)| (path.clone(), diagnostics.clone()))
        .collect();
    entries.sort_by(|a, b| a.0.as_os_str().cmp(b.0.as_os_str()));

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (path, diagnostics) in entries {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(path.display().to_string())
                            .strong()
                            .small(),
                    );
                    ui.label(
                        egui::RichText::new(format!("({})", diagnostics.len()))
                            .small()
                            .weak(),
                    );
                });
                for diagnostic in diagnostics {
                    let (mark, color) = if diagnostic.severity.is_error() {
                        ("✖", colors.error)
                    } else if diagnostic.severity.is_warning() {
                        ("▲", colors.warning)
                    } else {
                        ("ⓘ", colors.info)
                    };
                    ui.horizontal(|ui| {
                        panels::status_dot(ui, color);
                        if ui
                            .link(
                                egui::RichText::new(format!(
                                    "{mark} L{}:{} — {}",
                                    diagnostic.range.start.line + 1,
                                    diagnostic.range.start.character + 1,
                                    diagnostic.message
                                ))
                                .small(),
                            )
                            .clicked()
                        {
                            if let Some(id) = app.open_file(path.as_path()) {
                                app.jump_to_position_at(
                                    ctx,
                                    id,
                                    diagnostic.range.start.line,
                                    diagnostic.range.start.character,
                                );
                            }
                        }
                        if let Some(source) = &diagnostic.source {
                            ui.label(egui::RichText::new(source).small().weak());
                        }
                    });
                }
                ui.separator();
            }
        });
}

// ---------------------------------------------------------------- symbols ----

fn draw_symbols(app: &mut RavenApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let Some(state) = app.active_doc() else {
        ui.weak("Open a file to see its document symbols (requires a configured language server).");
        return;
    };
    let Some(path) = state.doc.path() else {
        ui.weak("Save the file (it needs a path) and open it with a configured language server to load symbols.");
        return;
    };
    let Some(symbols) = app.lsp.symbols.get(path).cloned() else {
        ui.weak(format!(
            "No symbols for {}. Configure a server for {} under [lsp.servers] in settings.",
            path.display(),
            state.language.name
        ));
        return;
    };
    if symbols.is_empty() {
        ui.weak("The language server reported no symbols for this document.");
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for (name, kind, line) in symbols {
                if ui
                    .link(
                        egui::RichText::new(format!("{kind}  {name}"))
                            .monospace()
                            .small(),
                    )
                    .clicked()
                {
                    if let Some(id) = app.active_doc_id() {
                        app.jump_to_line(ctx, id, line as usize);
                    }
                }
            }
        });
}
