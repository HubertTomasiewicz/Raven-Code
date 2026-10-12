//! The central editor area: tab strips, find & replace bar, one or two
//! editor panes (split view), each backed by the code editor widget inside
//! a shared ScrollArea.

use eframe::egui;

use crate::actions::Action;
use crate::app::{PreviewKind, RavenApp};
use crate::code_editor::{EditorAssets, show_code_editor};

pub fn show(app: &mut RavenApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(
                app.colors().editor_bg[0],
                app.colors().editor_bg[1],
                app.colors().editor_bg[2],
                app.colors().editor_bg[3],
            )),
        )
        .show(ui, |ui| {
            if !app.split {
                draw_pane(app, ui, ctx, 0);
            } else {
                ui.columns(2, |columns| {
                    columns[0].set_min_width(columns[0].available_width());
                    draw_pane(app, &mut columns[0], ctx, 0);
                    draw_pane(app, &mut columns[1], ctx, 1);
                });
            }
        });

    draw_completion_popup(app, ctx);
    draw_hover_window(app, ctx);
}

/// Floating completion list (LSP), anchored top-left under the tab strip.
fn draw_completion_popup(app: &mut RavenApp, ctx: &egui::Context) {
    if !app.completion_open {
        return;
    }
    let Some((uri, items)) = app.lsp.completions.clone() else {
        return;
    };
    // Only show while the originating document is still active.
    let matches_active = app
        .active_doc()
        .and_then(|doc| doc.doc.path())
        .map(|path| raven_lsp::client::path_to_uri(path) == uri)
        .unwrap_or(false);
    if !matches_active || items.is_empty() {
        app.completion_open = false;
        return;
    }

    let colors = app.colors();
    egui::Area::new(egui::Id::new("completion-popup"))
        .anchor(egui::Align2::LEFT_TOP, [240.0, 92.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(
                    colors.overlay_bg[0],
                    colors.overlay_bg[1],
                    colors.overlay_bg[2],
                    255,
                ))
                .inner_margin(4.0)
                .show(ui, |ui| {
                    ui.set_min_width(280.0);
                    ui.set_max_width(420.0);
                    ui.label(
                        egui::RichText::new("completions — Enter accepts, Esc closes")
                            .weak()
                            .small(),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for (index, item) in items.iter().enumerate() {
                                let selected = index == app.completion_selected;
                                let label = match &item.detail {
                                    Some(detail) => format!("{}   {}", item.label, detail),
                                    None => item.label.clone(),
                                };
                                if ui
                                    .selectable_label(selected, egui::RichText::new(label).small())
                                    .clicked()
                                {
                                    app.accept_completion(index);
                                    return;
                                }
                            }
                        });
                });
        });
}

/// Transient hover-info window (LSP, Alt+H).
fn draw_hover_window(app: &mut RavenApp, ctx: &egui::Context) {
    let visible_until = app.hover_until;
    let Some(until) = visible_until else {
        return;
    };
    if std::time::Instant::now() > until {
        app.hover_until = None;
        return;
    }
    let Some((path, text)) = app.lsp.hover.clone() else {
        return;
    };
    egui::Window::new(egui::RichText::new(format!("Hover — {}", path.display())).small())
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, [-24.0, 96.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.add(egui::Label::new(
                egui::RichText::new(text).monospace().small(),
            ));
        });
}

fn draw_pane(app: &mut RavenApp, ui: &mut egui::Ui, ctx: &egui::Context, pane_index: usize) {
    // Clicking anywhere in a pane focuses it (status bar & shortcuts follow).
    if ui.input(|input| input.pointer.any_click()) && ui.ui_contains_pointer() {
        app.active_pane = pane_index;
    }

    draw_tab_strip(app, ui, pane_index);
    draw_find_bar(app, ui, pane_index);

    let Some(doc_id) = app.panes[pane_index].active else {
        draw_welcome(app, ui);
        return;
    };
    if !app.docs.contains_key(&doc_id) {
        app.panes[pane_index].active = None;
        return;
    }

    if app.image_previews.contains_key(&doc_id) {
        draw_image_preview(app, ui, doc_id);
        return;
    }
    if app.hex_previews.contains_key(&doc_id) {
        draw_hex_preview(app, ui, doc_id);
        return;
    }
    if app.preview_modes.contains(&doc_id) {
        draw_text_preview(app, ui, doc_id);
        return;
    }

    CURRENT_ACTIVE_DOC.with(|cell| cell.set(doc_id));

    // Consume pending focus requests (find bar -> editor).
    let focus_editor = std::mem::take(&mut app.find.focus_editor);

    let mut output = None;
    // With soft wrapping enabled, constrain the editor to the viewport width.
    // A two-axis scroll area gives the TextEdit layouter an unbounded horizontal
    // extent, which can make the final token on a wrapped row appear clipped.
    let horizontal_scroll = !app.settings.editor.word_wrap;
    egui::ScrollArea::new([horizontal_scroll, true])
        .id_salt(("editor_scroll", pane_index, doc_id))
        .auto_shrink(false)
        .show(ui, |ui| {
            let _font_size = app.settings.effective_font_size();
            let highlighting_enabled = {
                let state = app.docs.get(&doc_id).expect("checked above");
                state.highlighting_enabled(&app.settings)
            };

            // Reset the highlight cache only when the mode changed (never
            // per frame — that would defeat the incremental cache).
            {
                let state = app.docs.get(&doc_id).expect("checked above");
                let doc_highlight = app.highlighters.entry(doc_id).or_default();
                if doc_highlight.enabled != highlighting_enabled {
                    doc_highlight.reset(state.doc.text().split('\n').count(), highlighting_enabled);
                }
            }

            let language = {
                let state = app.docs.get(&doc_id).expect("checked above");
                state.language
            };
            let syntax_name = app
                .docs
                .get(&doc_id)
                .map(|state| state.syntax_name.clone())
                .unwrap_or_else(|| "Plain Text".to_string());
            let read_only = app
                .docs
                .get(&doc_id)
                .is_some_and(|state| state.is_read_only());

            // Lend the mutable editor state pieces for this frame.
            let colors = app.colors();
            let font_size = app.settings.effective_font_size();
            let wrap = app.settings.editor.word_wrap;
            let line_numbers = app.settings.editor.line_numbers;
            let insert_spaces = app.settings.editor.insert_spaces;
            let tab_size = app.settings.editor.tab_size;
            let engine = &mut app.engine;
            let doc_highlight = app.highlighters.entry(doc_id).or_default();
            let syntect_theme = engine.theme(app.theme_id.dark);
            let job_cache = app.job_caches.entry(doc_id).or_insert_with(|| None);

            let assets = EditorAssets {
                font_id: egui::FontId::monospace(font_size),
                wrap,
                line_numbers,
                insert_spaces,
                tab_size,
                auto_indent: true,
                read_only,
                colors,
                engine: Some(engine),
                doc_highlight: Some(doc_highlight),
                language: Some(language),
                syntax_name,
                syntect_theme: Some(syntect_theme),
                job_cache: Some(job_cache),
            };

            let editor_id = egui::Id::new(("editor", pane_index, doc_id));
            let Some(state) = app.docs.get_mut(&doc_id) else {
                return;
            };
            let text = state.doc.text_mut();
            let result = show_code_editor(ui, editor_id, text, assets);
            output = Some((result, doc_id));
        });

    if let Some((editor_output, doc_id)) = output {
        app.panes[pane_index].cursor = (editor_output.cursor_line, editor_output.cursor_col);
        app.panes[pane_index].selection_chars = editor_output.selection_chars;
        if editor_output.changed {
            let state = app.docs.get_mut(&doc_id).expect("doc exists");
            state.modified = true;
            state.last_edit_at = std::time::Instant::now();
            state.rev += 1;
            app.settings_dirty = true;
        }
    }

    if focus_editor {
        let editor_id = egui::Id::new(("editor", pane_index, doc_id));
        ctx.memory_mut(|memory| memory.request_focus(editor_id));
    }
}

fn draw_text_preview(app: &mut RavenApp, ui: &mut egui::Ui, doc_id: u64) {
    let Some((kind, text)) = app.docs.get(&doc_id).and_then(|state| {
        let path = state.doc.path()?;
        Some((
            crate::app::preview_kind(path)?,
            state.doc.text().to_string(),
        ))
    }) else {
        app.preview_modes.remove(&doc_id);
        return;
    };
    let mut source = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(match kind {
                PreviewKind::Markdown => "Markdown Preview",
                PreviewKind::Json => "JSON Preview",
                PreviewKind::Delimited => "Table Preview",
            })
            .strong(),
        );
        ui.separator();
        if kind == PreviewKind::Json && ui.button("Format JSON").clicked() {
            app.active_pane = app
                .panes
                .iter()
                .position(|pane| pane.active == Some(doc_id))
                .unwrap_or(app.active_pane);
            app.format_json_active();
        }
        if ui.button("Source").clicked() {
            source = true;
        }
    });
    ui.separator();
    egui::ScrollArea::both()
        .auto_shrink(false)
        .show(ui, |ui| match kind {
            PreviewKind::Markdown => render_markdown(ui, &text),
            PreviewKind::Json => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(value) => render_json_value(ui, "root", &value, 0),
                Err(error) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Invalid JSON: {error}"),
                    );
                }
            },
            PreviewKind::Delimited => render_delimited(ui, &text, doc_id, app),
        });
    if source {
        app.preview_modes.remove(&doc_id);
    }
}

fn render_markdown(ui: &mut egui::Ui, text: &str) {
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            ui.label(egui::RichText::new(line).monospace());
        } else if trimmed.is_empty() {
            ui.add_space(6.0);
        } else if let Some(heading) = trimmed.strip_prefix("### ") {
            ui.heading(egui::RichText::new(heading).size(18.0));
        } else if let Some(heading) = trimmed.strip_prefix("## ") {
            ui.heading(egui::RichText::new(heading).size(22.0));
        } else if let Some(heading) = trimmed.strip_prefix("# ") {
            ui.heading(egui::RichText::new(heading).size(27.0));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            ui.label(format!("• {item}"));
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            ui.horizontal(|ui| {
                ui.separator();
                ui.label(quote);
            });
        } else {
            // Deliberately display raw HTML as text; never execute it in a preview.
            ui.label(line);
        }
    }
}

fn render_json_value(ui: &mut egui::Ui, name: &str, value: &serde_json::Value, depth: usize) {
    if depth >= 10 {
        ui.label(format!("{name}: … (depth limit)"));
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            egui::CollapsingHeader::new(format!("{name} {{}} ({} keys)", map.len()))
                .default_open(depth < 2)
                .show(ui, |ui| {
                    for (index, (key, value)) in map.iter().enumerate() {
                        if index >= 300 {
                            ui.label("… (item limit)");
                            break;
                        }
                        render_json_value(ui, key, value, depth + 1);
                    }
                });
        }
        serde_json::Value::Array(items) => {
            egui::CollapsingHeader::new(format!("{name} [] ({} items)", items.len()))
                .default_open(depth < 2)
                .show(ui, |ui| {
                    for (index, value) in items.iter().take(300).enumerate() {
                        render_json_value(ui, &format!("[{index}]"), value, depth + 1);
                    }
                    if items.len() > 300 {
                        ui.label("… (item limit)");
                    }
                });
        }
        _ => {
            ui.label(format!("{name}: {value}"));
        }
    }
}

fn render_delimited(ui: &mut egui::Ui, text: &str, doc_id: u64, app: &mut RavenApp) {
    let delimiter = app
        .docs
        .get(&doc_id)
        .and_then(|doc| doc.doc.path())
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
        .filter(|ext| ext.eq_ignore_ascii_case("tsv"))
        .map(|_| '\t')
        .unwrap_or(',');
    let rows = parse_delimited(text, delimiter, 5000);
    if rows.is_empty() {
        ui.label("No rows");
        return;
    }
    egui::Grid::new(("delimited-preview", doc_id))
        .striped(true)
        .show(ui, |ui| {
            for (row_index, row) in rows.iter().enumerate() {
                for cell in row {
                    ui.label(if row_index == 0 {
                        egui::RichText::new(cell).strong()
                    } else {
                        egui::RichText::new(cell)
                    });
                }
                ui.end_row();
            }
        });
    if rows.len() >= 5000 {
        ui.label("Showing first 5,000 rows.");
    }
}

fn parse_delimited(text: &str, delimiter: char, limit: usize) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' && chars.peek() == Some(&'"') {
                cell.push('"');
                chars.next();
            } else if ch == '"' {
                quoted = false;
            } else {
                cell.push(ch);
            }
        } else {
            match ch {
                '"' if cell.is_empty() => quoted = true,
                c if c == delimiter => row.push(std::mem::take(&mut cell)),
                '\n' => {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                    if rows.len() >= limit {
                        break;
                    }
                }
                '\r' => {}
                _ => cell.push(ch),
            }
        }
    }
    if rows.len() < limit && (!cell.is_empty() || !row.is_empty()) {
        row.push(cell);
        rows.push(row);
    }
    rows
}

fn draw_image_preview(app: &mut RavenApp, ui: &mut egui::Ui, doc_id: u64) {
    let ctx = ui.ctx().clone();
    let mut open_source = None;
    let Some(preview) = app.image_previews.get_mut(&doc_id) else {
        return;
    };

    if preview.texture.is_none() {
        if let Some(decoded) = preview.decoded.take() {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width as usize, decoded.height as usize],
                &decoded.rgba,
            );
            preview.texture = Some(ctx.load_texture(
                format!("image-preview-{doc_id}"),
                image,
                egui::TextureOptions::LINEAR,
            ));
            preview.dimensions = Some((decoded.width, decoded.height));
            preview.format = Some(decoded.format);
        }
    }

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Image Preview").strong());
        ui.separator();
        if ui.selectable_label(preview.fit, "Fit").clicked() {
            preview.fit = true;
        }
        if ui.selectable_label(!preview.fit, "Actual size").clicked() {
            preview.fit = false;
            preview.zoom = 1.0;
        }
        ui.separator();
        if ui.small_button("−").on_hover_text("Zoom out").clicked() {
            preview.fit = false;
            preview.zoom = (preview.zoom / 1.2).max(0.1);
        }
        ui.label(format!("{}%", (preview.zoom * 100.0).round() as u32));
        if ui.small_button("+").on_hover_text("Zoom in").clicked() {
            preview.fit = false;
            preview.zoom = (preview.zoom * 1.2).min(8.0);
        }
        if preview
            .format
            .as_deref()
            .is_some_and(|format| format.eq_ignore_ascii_case("svg"))
            && ui.button("Source").clicked()
        {
            open_source = Some(preview.path.clone());
        }
        ui.separator();
        let details = match (preview.dimensions, preview.format.as_deref()) {
            (Some((width, height)), Some(format)) => {
                format!(
                    "{width} × {height} · {format} · {} bytes",
                    preview.file_size
                )
            }
            _ => format!("{} bytes", preview.file_size),
        };
        ui.label(egui::RichText::new(details).small().weak());
    });
    ui.separator();

    if let Some(error) = &preview.error {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("Image preview unavailable");
            ui.label(error);
        });
        return;
    }
    let Some(texture) = preview.texture.clone() else {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.spinner();
            ui.label("Decoding image…");
        });
        return;
    };
    let Some((width, height)) = preview.dimensions else {
        return;
    };

    let viewport = ui.available_size_before_wrap().max(egui::vec2(1.0, 1.0));
    let image_size = if preview.fit {
        let scale = (viewport.x / width as f32)
            .min(viewport.y / height as f32)
            .min(1.0);
        egui::vec2(width as f32 * scale, height as f32 * scale)
    } else {
        egui::vec2(width as f32, height as f32) * preview.zoom
    };

    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let viewport = ui.available_size_before_wrap().max(egui::vec2(1.0, 1.0));
        let content_size = image_size.max(viewport);
        let (content_rect, _) = ui.allocate_exact_size(content_size, egui::Sense::hover());
        let image_rect = egui::Rect::from_center_size(content_rect.center(), image_size);
        draw_checkerboard(ui.painter(), image_rect);
        ui.painter().image(
            texture.id(),
            image_rect,
            egui::Rect::EVERYTHING,
            egui::Color32::WHITE,
        );
    });
    if let Some(path) = open_source {
        let panes: Vec<usize> = app
            .panes
            .iter()
            .enumerate()
            .filter_map(|(index, pane)| pane.tabs.contains(&doc_id).then_some(index))
            .collect();
        for pane in panes {
            app.do_close_tab(pane, doc_id);
        }
        app.open_file(&path);
    }
}

fn draw_checkerboard(painter: &egui::Painter, image_rect: egui::Rect) {
    let visible = painter.clip_rect().intersect(image_rect);
    if visible.is_negative() {
        return;
    }
    let tile = 18.0;
    let start_x = ((visible.left() - image_rect.left()) / tile).floor() as i32;
    let end_x = ((visible.right() - image_rect.left()) / tile).ceil() as i32;
    let start_y = ((visible.top() - image_rect.top()) / tile).floor() as i32;
    let end_y = ((visible.bottom() - image_rect.top()) / tile).ceil() as i32;
    painter.rect_filled(image_rect, 0.0, egui::Color32::from_gray(62));
    for y in start_y..end_y {
        for x in start_x..end_x {
            if (x + y) % 2 == 0 {
                let rect = egui::Rect::from_min_size(
                    image_rect.min + egui::vec2(x as f32 * tile, y as f32 * tile),
                    egui::vec2(tile, tile),
                );
                painter.rect_filled(rect, 0.0, egui::Color32::from_gray(78));
            }
        }
    }
}

fn draw_hex_preview(app: &mut RavenApp, ui: &mut egui::Ui, doc_id: u64) {
    let ctx = ui.ctx().clone();
    let mut save_requested = false;
    let mut bytes_changed = false;
    let Some(preview) = app.hex_previews.get_mut(&doc_id) else {
        return;
    };

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Hex Viewer").strong());
        ui.separator();
        let complete = preview.file_size <= crate::file_previews::HEX_WINDOW_BYTES
            && preview
                .bytes
                .as_ref()
                .is_some_and(|bytes| bytes.len() as u64 == preview.file_size);
        let mut editing = preview.edit_mode;
        if ui
            .add_enabled(
                complete,
                egui::Checkbox::new(&mut editing, "Enable editing"),
            )
            .on_hover_text("Editing is only enabled for fully loaded files under 2 MiB")
            .changed()
        {
            preview.edit_mode = editing;
        }
        if preview.modified && ui.button("Save bytes").clicked() {
            save_requested = true;
        }
        ui.separator();
        ui.label(format!(
            "{} bytes{}",
            preview.file_size,
            if preview.file_size > crate::file_previews::HEX_WINDOW_BYTES {
                " · showing the first 2 MiB"
            } else {
                ""
            }
        ));
    });
    ui.horizontal(|ui| {
        ui.label("Search");
        ui.add(
            egui::TextEdit::singleline(&mut preview.search)
                .hint_text("hex bytes or ASCII text")
                .desired_width(230.0),
        );
        if let Some(offset) = preview.selected_byte {
            ui.label(format!("Selected offset: 0x{offset:08X}"));
        }
    });
    ui.separator();

    if let Some(error) = &preview.error {
        ui.colored_label(egui::Color32::from_rgb(235, 103, 92), error);
    } else if preview.bytes.is_none() {
        ui.vertical_centered(|ui| {
            ui.add_space(28.0);
            ui.spinner();
            ui.label("Reading binary data…");
        });
    } else {
        let bytes = preview.bytes.as_mut().expect("checked above");
        let row_count = bytes.len().div_ceil(16);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            22.0,
            row_count,
            |ui, rows| {
                for row in rows {
                    let start = row * 16;
                    let end = (start + 16).min(bytes.len());
                    let row_bytes = &bytes[start..end];
                    let hex = row_bytes
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let ascii = row_bytes
                        .iter()
                        .map(|byte| {
                            if (32..=126).contains(byte) {
                                char::from(*byte)
                            } else {
                                '.'
                            }
                        })
                        .collect::<String>();
                    let query = preview.search.trim().to_ascii_lowercase();
                    let matches_search = !query.is_empty()
                        && (hex.to_ascii_lowercase().contains(&query)
                            || ascii.to_ascii_lowercase().contains(&query));
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [94.0, 18.0],
                            egui::Label::new(
                                egui::RichText::new(format!("{:08X}", start))
                                    .monospace()
                                    .small()
                                    .weak(),
                            ),
                        );
                        for (relative, byte) in row_bytes.iter().enumerate() {
                            let offset = start + relative;
                            let selected = preview.selected_byte == Some(offset);
                            let text = format!("{byte:02X}");
                            let label = if matches_search {
                                egui::RichText::new(text)
                                    .monospace()
                                    .small()
                                    .color(egui::Color32::from_rgb(245, 193, 87))
                            } else {
                                egui::RichText::new(text).monospace().small()
                            };
                            if ui.selectable_label(selected, label).clicked() {
                                preview.selected_byte = Some(offset);
                                preview.edit_value = format!("{byte:02X}");
                            }
                        }
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(ascii).monospace().small());
                    });
                }
            },
        );
        if preview.edit_mode {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Selected byte (hex)");
                ui.add_enabled(
                    preview.selected_byte.is_some(),
                    egui::TextEdit::singleline(&mut preview.edit_value)
                        .char_limit(2)
                        .desired_width(48.0),
                );
                if ui
                    .add_enabled(
                        preview.selected_byte.is_some(),
                        egui::Button::new("Apply byte"),
                    )
                    .clicked()
                {
                    if let (Some(offset), Ok(value)) = (
                        preview.selected_byte,
                        u8::from_str_radix(preview.edit_value.trim(), 16),
                    ) {
                        if bytes.get(offset).is_some_and(|old| *old != value) {
                            bytes[offset] = value;
                            preview.modified = true;
                            bytes_changed = true;
                        }
                    }
                }
                if let Some(offset) = preview.selected_byte {
                    let row_start = offset / 16 * 16;
                    let row_end = (row_start + 16).min(bytes.len());
                    if ui.button("Copy row as hex").clicked() {
                        ctx.copy_text(
                            bytes[row_start..row_end]
                                .iter()
                                .map(|byte| format!("{byte:02X}"))
                                .collect::<Vec<_>>()
                                .join(" "),
                        );
                    }
                    if ui.button("Copy row as text").clicked() {
                        ctx.copy_text(
                            bytes[row_start..row_end]
                                .iter()
                                .map(|byte| {
                                    if (32..=126).contains(byte) {
                                        char::from(*byte)
                                    } else {
                                        '.'
                                    }
                                })
                                .collect::<String>(),
                        );
                    }
                }
            });
        }
    }
    if bytes_changed {
        if let Some(state) = app.docs.get_mut(&doc_id) {
            state.modified = true;
            state.last_edit_at = std::time::Instant::now();
        }
    }
    if save_requested {
        app.save_doc(doc_id);
    }
}

fn draw_tab_strip(app: &mut RavenApp, ui: &mut egui::Ui, pane_index: usize) {
    let colors = app.colors();
    let fill = egui::Color32::from_rgba_unmultiplied(
        colors.panel_bg[0],
        colors.panel_bg[1],
        colors.panel_bg[2],
        255,
    );
    egui::Panel::top(format!("tabs-{pane_index}"))
        .resizable(false)
        .frame(egui::Frame::new().fill(fill))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                ui.spacing_mut().item_spacing.y = 0.0;

                let tabs: Vec<u64> = app.panes[pane_index].tabs.clone();
                let active = app.panes[pane_index].active;
                if tabs.is_empty() {
                    return;
                }
                egui::ScrollArea::horizontal()
                    .id_salt(("tab-scroll", pane_index))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for doc_id in tabs {
                                let Some(state) = app.docs.get(&doc_id) else {
                                    continue;
                                };
                                let is_active = active == Some(doc_id);
                                let name = state.display_name();
                                let dot = if state.modified { " ●" } else { "" };
                                let icon_path = state
                                    .doc
                                    .path()
                                    .unwrap_or(std::path::Path::new("untitled.rs"));

                                let label = egui::RichText::new(format!("{name}{dot}")).size(12.5);
                                let button = egui::Button::new(label)
                                    .fill(if is_active {
                                        active_tab_fill(app)
                                    } else {
                                        egui::Color32::from_rgba_unmultiplied(
                                            colors.tab_inactive_bg[0],
                                            colors.tab_inactive_bg[1],
                                            colors.tab_inactive_bg[2],
                                            255,
                                        )
                                    })
                                    .stroke(if is_active {
                                        egui::Stroke::new(1.5, accent(app))
                                    } else {
                                        egui::Stroke::NONE
                                    });
                                ui.add(
                                    egui::Image::new(&crate::icons::file_texture(
                                        ui.ctx(),
                                        icon_path,
                                    ))
                                    .fit_to_exact_size(egui::vec2(14.0, 14.0)),
                                );
                                if ui.add(button).clicked() {
                                    app.panes[pane_index].active = Some(doc_id);
                                    app.active_pane = pane_index;
                                }
                                if ui
                                    .small_button("✕")
                                    .on_hover_text("Close tab (Ctrl+W)")
                                    .clicked()
                                {
                                    app.request_close_tab(pane_index, doc_id);
                                }
                            }
                        });
                    });
            });
        });
}

fn accent(app: &RavenApp) -> egui::Color32 {
    let color = app.colors().accent;
    egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 255)
}

fn active_tab_fill(app: &RavenApp) -> egui::Color32 {
    let color = app.colors().tab_active_bg;
    egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], 255)
}

fn draw_find_bar(app: &mut RavenApp, ui: &mut egui::Ui, pane_index: usize) {
    if !app.find.open {
        return;
    }
    let colors = app.colors();
    let fill = egui::Color32::from_rgba_unmultiplied(
        colors.panel_bg[0],
        colors.panel_bg[1],
        colors.panel_bg[2],
        255,
    );
    egui::Panel::top(format!("find-bar-{pane_index}"))
        .frame(egui::Frame::new().fill(fill))
        .show(ui, |ui| {
            ui.add_space(2.0);
            let ctx = ui.ctx().clone();
            let find_input_id = egui::Id::new(("find-input", pane_index));

            egui::Grid::new(("find-grid", pane_index))
                .num_columns(2)
                .spacing([6.0, 3.0])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let response = egui::TextEdit::singleline(&mut app.find.query)
                            .hint_text(if app.find.regex {
                                "find (regex)"
                            } else {
                                "find"
                            })
                            .desired_width(220.0)
                            .font(egui::FontId::monospace(12.5))
                            .id(find_input_id)
                            .show(ui);
                        if app.find.focus_editor {
                            ctx.memory_mut(|memory| memory.request_focus(find_input_id));
                            app.find.focus_editor = false;
                        }
                        let enter = response.response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        let shift_enter = response.response.lost_focus()
                            && ui.input(|input| {
                                input.key_pressed(egui::Key::Enter) && input.modifiers.shift
                            });
                        if enter {
                            find_next(app, pane_index, &ctx);
                        }
                        if shift_enter {
                            find_previous(app, pane_index, &ctx);
                        }
                        if !app.find.query.is_empty() {
                            ui.label(
                                egui::RichText::new(format!("{}", app.find.match_count)).small(),
                            );
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .small_button("◀")
                            .on_hover_text("Previous match")
                            .clicked()
                        {
                            find_previous(app, pane_index, &ctx);
                        }
                        if ui.small_button("▶").on_hover_text("Next match").clicked() {
                            find_next(app, pane_index, &ctx);
                        }
                        toggle(ui, "Aa", app.find.case_sensitive, "Match case", |value| {
                            app.find.case_sensitive = value;
                        });
                        toggle(ui, "W", app.find.whole_word, "Whole word", |value| {
                            app.find.whole_word = value;
                        });
                        toggle(ui, ".*", app.find.regex, "Regular expression", |value| {
                            app.find.regex = value;
                        });
                        if ui.small_button("×").on_hover_text("Close (Esc)").clicked() {
                            app.find.open = false;
                        }
                    });
                    ui.end_row();

                    if app.find.show_replace {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut app.find.replacement)
                                    .hint_text("replace with")
                                    .desired_width(220.0)
                                    .font(egui::FontId::monospace(12.5)),
                            );
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Replace").clicked() {
                                replace_current(app, pane_index, &ctx);
                            }
                            if ui.button("Replace All").clicked() {
                                replace_all(app, pane_index);
                            }
                        });
                        ui.end_row();
                    }
                });
            if let Some(message) = &app.find.message {
                ui.label(egui::RichText::new(message).small().color(
                    egui::Color32::from_rgba_unmultiplied(
                        colors.warning[0],
                        colors.warning[1],
                        colors.warning[2],
                        255,
                    ),
                ));
            }
            ui.add_space(2.0);
        });

    // Escape closes the bar.
    if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.find.open = false;
    }
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

fn current_query(app: &RavenApp) -> raven_editor::search::SearchQuery {
    raven_editor::search::SearchQuery {
        text: app.find.query.clone(),
        case_sensitive: app.find.case_sensitive,
        whole_word: app.find.whole_word,
        regex: app.find.regex,
    }
}

fn refresh_match_count(app: &mut RavenApp) {
    if app.find.query.is_empty() {
        app.find.match_count = 0;
        app.find.message = None;
        return;
    }
    let query = current_query(app);
    let Some(state) = app.active_doc() else {
        app.find.match_count = 0;
        return;
    };
    match raven_editor::search::find_all(state.doc.text(), &query, 10_000) {
        Ok(ranges) => {
            app.find.match_count = ranges.len();
            app.find.message = None;
        }
        Err(error) => {
            app.find.message = Some(error);
            app.find.match_count = 0;
        }
    }
}

fn active_cursor_char(ctx: &egui::Context, pane_index: usize) -> Option<usize> {
    let doc_id = app_active_doc(ctx, pane_index)?;
    let editor_id = egui::Id::new(("editor", pane_index, doc_id));
    egui::TextEdit::load_state(ctx, editor_id)
        .and_then(|edit_state| edit_state.cursor.char_range())
        .map(|range| range.primary.index.0)
}

// The helper needs the app to resolve the active doc; egui does not carry it.
// Implemented via a thread-local set by draw_pane for the current frame.
thread_local! {
    static CURRENT_ACTIVE_DOC: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn app_active_doc(_ctx: &egui::Context, pane_index: usize) -> Option<u64> {
    let id = CURRENT_ACTIVE_DOC.get();
    let _ = pane_index;
    (id != 0).then_some(id)
}

fn set_cursor_range(
    ctx: &egui::Context,
    pane_index: usize,
    doc_id: u64,
    start_char: usize,
    end_char: usize,
) {
    let editor_id = egui::Id::new(("editor", pane_index, doc_id));
    if let Some(mut edit_state) = egui::TextEdit::load_state(ctx, editor_id) {
        edit_state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange {
                primary: egui::text::CCursor {
                    index: egui::text::CharIndex(end_char),
                    prefer_next_row: true,
                },
                secondary: egui::text::CCursor {
                    index: egui::text::CharIndex(start_char),
                    prefer_next_row: false,
                },
                h_pos: None,
            }));
        egui::TextEdit::store_state(ctx, editor_id, edit_state);
    }
}

fn find_next(app: &mut RavenApp, pane_index: usize, ctx: &egui::Context) {
    refresh_match_count(app);
    let Some(doc_id) = app.active_doc_id() else {
        return;
    };
    let _ = pane_index;
    let query = current_query(app);
    let Some(state) = app.docs.get(&doc_id) else {
        return;
    };
    let text = state.doc.text().to_string();
    let current_char = active_cursor_char(ctx, pane_index).unwrap_or(0);
    let current_byte = crate::highlight::byte_from_char_index(&text, current_char);

    match raven_editor::search::find_next(&text, &query, current_byte, true) {
        Ok(Some(range)) => {
            let start_char = crate::highlight::char_index_from_byte(&text, range.start);
            let end_char = crate::highlight::char_index_from_byte(&text, range.end);
            set_cursor_range(ctx, pane_index, doc_id, start_char, end_char);
        }
        Ok(None) => app.find.message = Some("no matches".to_string()),
        Err(error) => app.find.message = Some(error),
    }
}

fn find_previous(app: &mut RavenApp, pane_index: usize, ctx: &egui::Context) {
    refresh_match_count(app);
    let Some(doc_id) = app.active_doc_id() else {
        return;
    };
    let query = current_query(app);
    let Some(state) = app.docs.get(&doc_id) else {
        return;
    };
    let text = state.doc.text().to_string();
    let current_char = active_cursor_char(ctx, pane_index).unwrap_or(0);
    let current_byte = crate::highlight::byte_from_char_index(&text, current_char);

    match raven_editor::search::find_previous(&text, &query, current_byte, true) {
        Ok(Some(range)) => {
            let start_char = crate::highlight::char_index_from_byte(&text, range.start);
            let end_char = crate::highlight::char_index_from_byte(&text, range.end);
            set_cursor_range(ctx, pane_index, doc_id, start_char, end_char);
        }
        Ok(None) => app.find.message = Some("no matches".to_string()),
        Err(error) => app.find.message = Some(error),
    }
}

fn replace_current(app: &mut RavenApp, pane_index: usize, ctx: &egui::Context) {
    refresh_match_count(app);
    let Some(doc_id) = app.active_doc_id() else {
        return;
    };
    let query = current_query(app);
    let Some(state) = app.docs.get_mut(&doc_id) else {
        return;
    };
    let text = state.doc.text().to_string();
    let current_char = active_cursor_char(ctx, pane_index).unwrap_or(0);
    let current_byte = crate::highlight::byte_from_char_index(&text, current_char);

    if let Ok(Some(range)) = raven_editor::search::find_next(&text, &query, current_byte, true) {
        let replacement = app.find.replacement.clone();
        let new_text = format!(
            "{}{replacement}{}",
            &text[..range.start],
            &text[range.end..]
        );
        let cursor_byte = range.start + replacement.len();
        let start_char = crate::highlight::char_index_from_byte(&new_text, cursor_byte);
        state.doc.set_text(&new_text);
        state.modified = true;
        state.rev += 1;
        if let Some(slot) = app.job_caches.get_mut(&doc_id) {
            *slot = None;
        }
        app.settings_dirty = true;
        set_cursor_range(ctx, pane_index, doc_id, start_char, start_char);
    }
}

fn replace_all(app: &mut RavenApp, pane_index: usize) {
    refresh_match_count(app);
    let Some(doc_id) = app.active_doc_id() else {
        return;
    };
    let _ = pane_index;
    let query = current_query(app);
    let Some(state) = app.docs.get_mut(&doc_id) else {
        return;
    };
    match raven_editor::search::replace_all(
        state.doc.text(),
        &query,
        &app.find.replacement,
        1_000_000,
    ) {
        Ok((new_text, count)) => {
            state.doc.set_text(&new_text);
            state.modified = true;
            state.rev += 1;
            if let Some(slot) = app.job_caches.get_mut(&doc_id) {
                *slot = None;
            }
            app.set_notice(format!("replaced {count} occurrence(s)"), false);
            app.settings_dirty = true;
        }
        Err(error) => app.find.message = Some(error),
    }
}

fn draw_welcome(app: &mut RavenApp, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() / 4.5);
        let texture = crate::icons::brand_texture(ui.ctx());
        ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(
            texture.id(),
            [96.0, 96.0],
        )));
        ui.add_space(8.0);
        ui.heading(egui::RichText::new("Raven Code").size(24.0));
        ui.weak(format!(
            "v{} — the Raven ecosystem's lightweight editor",
            env!("CARGO_PKG_VERSION")
        ));
        ui.add_space(12.0);
        let new_file = ui.button("New File  (Ctrl+N)").clicked();
        let open_file = ui.button("Open File…  (Ctrl+O)").clicked();
        let open_folder = ui.button("Open Folder…  (Ctrl+Shift+O)").clicked();
        if new_file {
            app.run_action(Action::NewFile);
        }
        if open_file {
            app.run_action(Action::OpenFile);
        }
        if open_folder {
            app.run_action(Action::OpenFolder);
        }
        ui.weak("Ctrl+P quick open · Ctrl+Shift+P commands · Ctrl+B explorer");
    });
}
