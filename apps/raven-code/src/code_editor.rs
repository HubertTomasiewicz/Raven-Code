//! The code editor widget: egui `TextEdit` + syntax-colored layouter +
//! line-number gutter + editor behaviors (auto-indent, smart Tab).
//!
//! Why this shape:
//!
//! * egui 0.36's multiline `TextEdit` sizes itself to its full galley and
//!   scrolls via the *enclosing* `ScrollArea` — which lets a separate
//!   gutter column stay perfectly in sync with zero scroll plumbing,
//! * the layouter returns a **cached galley** keyed by (text, wrap width,
//!   font) so per-frame cost is an `Arc` clone; edits trigger one rebuild,
//!   and the rebuild itself invalidates the syntect cache exactly at the
//!   first changed line (see `highlight::DocHighlighter`),
//! * `Tab`/`Enter` are intercepted **before** the text edit sees them when
//!   custom behavior (spaces indentation, auto-indent) applies; otherwise
//!   egui's native handling (tab char, shift-tab dedent) is kept.

use eframe::egui;
use egui::text::{CCursor, CharIndex, LayoutJob, LayoutSection, TextFormat, TextWrapping};

use crate::highlight::{
    DocHighlighter, HighlightEngine, byte_from_char_index, char_index_from_byte,
};

/// Everything the editor needs to render one frame. Mutable pieces are
/// borrowed from `RavenApp` per field so the borrow checker stays happy.
pub struct EditorAssets<'a> {
    pub font_id: egui::FontId,
    pub wrap: bool,
    pub line_numbers: bool,
    pub insert_spaces: bool,
    pub tab_size: u8,
    pub auto_indent: bool,
    pub read_only: bool,
    pub colors: &'a raven_editor::theme::ThemeColors,

    pub engine: Option<&'a mut HighlightEngine>,
    pub doc_highlight: Option<&'a mut DocHighlighter>,
    pub language: Option<&'static raven_editor::language::LanguageDef>,
    pub syntax_name: String,
    pub syntect_theme: Option<std::sync::Arc<syntect::highlighting::Theme>>,
    pub job_cache: Option<&'a mut Option<JobCache>>,
}

/// Cached layout result for one (pane, document).
pub struct JobCache {
    pub text: String,
    pub wrap_width: f32,
    pub font_id: egui::FontId,
    pub galley: std::sync::Arc<egui::Galley>,
    /// y position of the first visual row of every logical line.
    pub line_y: Vec<f32>,
    pub line_height: f32,
}

/// What the editor reports back to the app after one frame. Some fields
/// are part of the widget's public surface for future features (e.g.
/// click-to-position in the gutter) and are not read yet.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CodeEditorOutput {
    pub changed: bool,
    pub focused: bool,
    pub editor_id: egui::Id,
    /// primary cursor, byte offset
    pub cursor_byte: usize,
    pub cursor_line: usize,
    pub cursor_col: usize,
    /// selection length in chars
    pub selection_chars: usize,
    /// the galley y position of the cursor line (status bar / paint)
    pub cursor_y: f32,
}

/// Renders the editor inside the current `ScrollArea`. Callers wrap this in
/// `egui::ScrollArea::both()`.
pub fn show_code_editor(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    mut assets: EditorAssets<'_>,
) -> CodeEditorOutput {
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&assets.font_id));
    let line_height = row_height + ui.spacing().extra_text_line_spacing;

    let line_count = text.split('\n').count().max(1);
    let digits = line_count.to_string().len().max(2);
    let digit_width = ui
        .fonts_mut(|fonts| fonts.glyph_width(&assets.font_id, '0'))
        .max(1.0);
    let gutter_width = if assets.line_numbers {
        digit_width * digits as f32 + 10.0
    } else {
        0.0
    };

    let was_focused = ui.memory(|memory| memory.has_focus(id));
    let mut gutter_x: Option<f32> = None;

    // Immutable copies used after the layouter closure borrows `assets`.
    let widget_font_id = assets.font_id.clone();
    let line_numbers_enabled = assets.line_numbers;
    let editor_fill = egui::Color32::from_rgba_unmultiplied(
        assets.colors.editor_bg[0],
        assets.colors.editor_bg[1],
        assets.colors.editor_bg[2],
        assets.colors.editor_bg[3],
    );
    let read_only = assets.read_only;

    // ------------------------------------------------------ key handling ----
    if was_focused && !assets.read_only {
        intercept_editor_keys(ui, id, text, &assets, line_height);
    }

    // -------------------------------------------------------- the widget ----
    let mut layouter = |layout_ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
        let content = buffer.as_str();

        // Cache hit: same text, same width, same font -> reuse the galley.
        if let Some(Some(cache)) = assets.job_cache.as_deref_mut() {
            if cache.text == content
                && (cache.wrap_width - wrap_width).abs() < 0.5
                && cache.font_id == assets.font_id
                && (cache.line_height - line_height).abs() < 0.1
            {
                return cache.galley.clone();
            }
        }

        // Text changed since the cache was built: invalidate the highlight
        // cache exactly from the first differing line.
        if let (Some(doc), Some(Some(cache))) = (
            assets.doc_highlight.as_deref_mut(),
            assets.job_cache.as_deref_mut(),
        ) && cache.text != content
        {
            let first_diff = first_diff_byte(&cache.text, content);
            doc.invalidate_from(first_diff, content);
        }

        let job = build_layout_job(content, &mut assets, wrap_width, line_height, layout_ui);
        let galley = layout_ui.fonts_mut(|fonts| fonts.layout_job(job));

        // Compute per-logical-line y positions (wrap makes them uneven).
        let line_y = if assets.wrap {
            let mut positions = Vec::with_capacity(line_count);
            let mut char_offset = 0usize;
            for line in content.split('\n') {
                let cursor = CCursor {
                    index: CharIndex(char_offset),
                    prefer_next_row: false,
                };
                positions.push(galley.pos_from_cursor(cursor).min.y);
                char_offset += line.chars().count() + 1;
            }
            positions
        } else {
            (0..line_count)
                .map(|index| index as f32 * line_height)
                .collect()
        };

        let cache = JobCache {
            text: content.to_string(),
            wrap_width,
            font_id: assets.font_id.clone(),
            galley: galley.clone(),
            line_y,
            line_height,
        };
        if let Some(slot) = assets.job_cache.as_deref_mut() {
            *slot = Some(cache);
        }
        galley
    };

    let frame = egui::Frame::new()
        .fill(editor_fill)
        .inner_margin(egui::Margin::symmetric(4, 2));

    let mut read_only_buffer: &str = text.as_str();
    let mut text_edit = if read_only {
        // Read-only documents keep selection but not editing: an immutable
        // buffer makes mutation impossible by construction.
        egui::TextEdit::multiline(&mut read_only_buffer)
    } else {
        egui::TextEdit::multiline(text)
    };
    text_edit = text_edit
        .id(id)
        .font(egui::FontSelection::FontId(widget_font_id.clone()))
        .desired_rows(1)
        .layouter(&mut layouter)
        .frame(frame)
        .cursor_at_end(false);

    // The gutter is a real horizontal slot beside the text edit, so the two
    // can never overlap. With word wrap on (the default) there is no
    // horizontal scrolling and the slot stays aligned; with wrap off and a
    // long line the gutter scrolls away horizontally (documented limitation).
    let mut output = None;
    ui.horizontal(|ui| {
        if line_numbers_enabled {
            let (gutter_slot, _) =
                ui.allocate_exact_size(egui::vec2(gutter_width, 4.0), egui::Sense::hover());
            gutter_x = Some(gutter_slot.min.x);
        }
        text_edit = text_edit.desired_width(ui.available_width());
        let out = text_edit.show(ui);
        output = Some(out);
    });
    let output = output.expect("text edit ran");

    let cursor_range = output.cursor_range;
    let (cursor_byte, cursor_line, cursor_col, selection_chars, cursor_line_index) =
        match &cursor_range {
            Some(range) => {
                let primary_char = range.primary.index.0;
                let byte = byte_from_char_index(text, primary_char);
                let before = &text[..byte];
                let line = before.bytes().filter(|byte| *byte == b'\n').count();
                let line_start = before.rfind('\n').map(|position| position + 1).unwrap_or(0);
                let col = text[line_start..byte].chars().count();
                let selection = range
                    .as_sorted_char_range()
                    .end
                    .0
                    .saturating_sub(range.as_sorted_char_range().start.0);
                let y = match assets.job_cache.as_deref() {
                    Some(Some(cache)) => cache
                        .line_y
                        .get(line)
                        .copied()
                        .unwrap_or(line as f32 * line_height),
                    _ => line as f32 * line_height,
                };
                (byte, line, col, selection, y)
            }
            None => (0, 0, 0, 0, 0.0),
        };

    // -------------------------------------------------------- the gutter ----
    if let (true, Some(gutter_x)) = (assets.line_numbers, gutter_x) {
        paint_gutter(
            ui,
            gutter_x,
            &output,
            &assets,
            digits,
            line_height,
            cursor_line,
        );
    }

    CodeEditorOutput {
        changed: output.response.changed(),
        focused: ui.memory(|memory| memory.has_focus(id)),
        editor_id: id,
        cursor_byte,
        cursor_line,
        cursor_col,
        selection_chars,
        cursor_y: cursor_line_index,
    }
}

/// Paints line numbers and the current-line highlight into the reserved
/// gutter slot. Line positions come from the galley, so they stay glued to
/// the text however far the ScrollArea is scrolled.
fn paint_gutter(
    ui: &mut egui::Ui,
    gutter_x: f32,
    output: &egui::widgets::text_edit::TextEditOutput,
    assets: &EditorAssets<'_>,
    digits: usize,
    line_height: f32,
    active_line: usize,
) {
    let colors = assets.colors;
    let viewport = ui.clip_rect();
    let number_width = ui
        .painter()
        .layout_no_wrap(
            "0".repeat(digits.max(3)),
            assets.font_id.clone(),
            egui::Color32::WHITE,
        )
        .size()
        .x;
    let gutter_rect = egui::Rect::from_min_size(
        egui::pos2(gutter_x, viewport.min.y),
        egui::vec2(number_width + 8.0, viewport.height()),
    );

    // Current-line highlight across the editor width (subtle, over text).
    let cache_y = |line: usize| -> f32 {
        match assets.job_cache.as_deref() {
            Some(Some(cache)) => cache
                .line_y
                .get(line)
                .copied()
                .unwrap_or(line as f32 * line_height),
            _ => line as f32 * line_height,
        }
    };
    let highlight_color = egui::Color32::from_rgba_unmultiplied(
        colors.line_highlight[0],
        colors.line_highlight[1],
        colors.line_highlight[2],
        40,
    );
    let highlight_y = output.galley_pos.y + cache_y(active_line);
    if highlight_y >= viewport.min.y - line_height && highlight_y <= viewport.max.y {
        let rect = egui::Rect::from_min_size(
            egui::pos2(viewport.min.x, highlight_y),
            egui::vec2(viewport.width(), line_height),
        );
        ui.painter().rect_filled(rect, 0.0, highlight_color);
    }

    let painter = ui.painter_at(gutter_rect);
    let font_id = assets.font_id.clone();
    let number_x = gutter_rect.min.x + 4.0;
    let text_origin_y = output.galley_pos.y;

    let line_count = cache_total_lines(assets);
    for index in 0..line_count {
        let y = text_origin_y + cache_y(index);
        if y > viewport.max.y {
            break;
        }
        if y + line_height >= viewport.min.y {
            let active = index == active_line;
            let color = if active {
                colors.gutter_active_text
            } else {
                colors.gutter_text
            };
            painter.text(
                egui::pos2(number_x, y),
                egui::Align2::LEFT_TOP,
                format!("{:>width$}", index + 1, width = digits),
                font_id.clone(),
                egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
            );
        }
    }
}

fn cache_total_lines(assets: &EditorAssets<'_>) -> usize {
    match assets.job_cache.as_deref() {
        Some(Some(cache)) => cache.line_y.len(),
        _ => 0,
    }
}

/// Intercepts Tab / Enter before the TextEdit processes them, implementing
/// spaces indentation and auto-indent. Consumed events never reach the
/// widget, so no double handling is possible.
fn intercept_editor_keys(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    assets: &EditorAssets<'_>,
    _line_height: f32,
) {
    let indent_unit = if assets.insert_spaces {
        " ".repeat(assets.tab_size as usize)
    } else {
        "\t".to_string()
    };

    // ---- Tab (indent / insert spaces) --------------------------------
    let tab_pressed = take_key_event(ui, |event| {
        matches!(
            event,
            egui::Event::Key {
                key: egui::Key::Tab,
                pressed: true,
                modifiers: egui::Modifiers {
                    shift: false,
                    alt: false,
                    ..
                },
                ..
            }
        )
    });
    let shift_tab_pressed = take_key_event(ui, |event| {
        matches!(
            event,
            egui::Event::Key {
                key: egui::Key::Tab,
                pressed: true,
                modifiers: egui::Modifiers { shift: true, .. },
                ..
            }
        )
    });

    if let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) {
        if let Some(range) = state.cursor.char_range() {
            let a = byte_from_char_index(text, range.primary.index.0.min(char_len(text)));
            let b = byte_from_char_index(text, range.secondary.index.0.min(char_len(text)));
            let (lo, hi) = (a.min(b), a.max(b));
            let has_selection = lo != hi;

            if tab_pressed && assets.insert_spaces {
                if has_selection {
                    let (new_text, new_lo, new_hi) =
                        transform_selected_lines(text, lo, hi, |line| {
                            indent_line(line, &indent_unit)
                        });
                    *text = new_text;
                    set_cursor_chars(
                        ui,
                        id,
                        text,
                        char_index_from_byte(text, new_lo),
                        char_index_from_byte(text, new_hi),
                    );
                } else {
                    text.insert_str(lo, &indent_unit);
                    let cursor = lo + indent_unit.len();
                    set_cursor_bytes(ui, id, text, cursor);
                }
            } else if shift_tab_pressed {
                let (new_text, new_lo, new_hi) = transform_selected_lines(text, lo, hi, |line| {
                    dedent_line(line, assets.tab_size)
                });
                *text = new_text;
                set_cursor_chars(
                    ui,
                    id,
                    text,
                    char_index_from_byte(text, new_lo),
                    char_index_from_byte(text, new_hi),
                );
            }
            // Native tab handling (insert '\t') remains for insert_spaces=false.
        }
    }

    // ---- Enter with auto-indent --------------------------------------
    if assets.auto_indent {
        let enter_pressed = take_key_event(ui, |event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter,
                    pressed: true,
                    modifiers: egui::Modifiers {
                        ctrl: false,
                        shift: false,
                        alt: false,
                        mac_cmd: false,
                        command: false
                    },
                    ..
                }
            )
        });
        if enter_pressed {
            if let Some(state) = egui::TextEdit::load_state(ui.ctx(), id) {
                if let Some(range) = state.cursor.char_range() {
                    let a = byte_from_char_index(text, range.primary.index.0.min(char_len(text)));
                    let b = byte_from_char_index(text, range.secondary.index.0.min(char_len(text)));
                    let (lo, hi) = (a.min(b), a.max(b));
                    let indent = current_indent(text, lo);
                    let continuation = if line_opens_block(text, lo) {
                        &indent_unit
                    } else {
                        ""
                    };
                    let insertion = format!("\n{indent}{continuation}");
                    text.replace_range(lo..hi, &insertion);
                    let new_cursor = lo + insertion.len();
                    set_cursor_bytes(ui, id, text, new_cursor);
                }
            }
        }
    }
}

fn take_key_event(ui: &mut egui::Ui, matches: impl Fn(&egui::Event) -> bool) -> bool {
    ui.input_mut(|input| {
        let found = input.events.iter().any(&matches);
        if found {
            input.events.retain(|event| !matches(event));
        }
        found
    })
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

fn current_indent(text: &str, byte: usize) -> String {
    let line_start = text[..byte]
        .rfind('\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    text[line_start..byte]
        .chars()
        .take_while(|character| *character == ' ' || *character == '\t')
        .collect()
}

fn line_opens_block(text: &str, byte: usize) -> bool {
    let line_start = text[..byte]
        .rfind('\n')
        .map(|position| position + 1)
        .unwrap_or(0);
    let line_end = text[byte..]
        .find('\n')
        .map(|position| byte + position)
        .unwrap_or(text.len());
    let trimmed = text[line_start..line_end].trim_end();
    trimmed.ends_with('{')
        || trimmed.ends_with('[')
        || trimmed.ends_with('(')
        || trimmed.ends_with(':')
}

/// Applies a per-line transform to every line intersecting `[lo, hi)` and
/// returns the new text plus cursor positions adjusted line-by-line
/// (selection endpoints stay at the same *position within their line*).
fn transform_selected_lines(
    text: &str,
    lo: usize,
    hi: usize,
    transform: impl Fn(&str) -> String,
) -> (String, usize, usize) {
    let mut out = String::with_capacity(text.len() + 16);
    let mut byte = 0usize; // old-text byte of current line start
    let mut new_line_start = 0usize; // new-text byte of current line start
    let mut new_lo = lo;
    let mut new_hi = hi;

    for line in text.split_inclusive('\n') {
        let line_len = line.len();
        let line_end = byte + line_len; // exclusive, includes '\n'
        let intersects = byte < hi && line_end > lo && !line.is_empty() && line != "\n";

        let transformed = if intersects {
            transform(line)
        } else {
            line.to_string()
        };

        // Map a cursor position inside this line into the new text.
        let map_cursor = |old_cursor: usize, out: &mut usize| {
            if old_cursor >= byte && old_cursor <= line_end && intersects {
                let within = (old_cursor - byte).min(line_len);
                let mut mapped = new_line_start + within.min(transformed.len());
                if line.ends_with('\n') {
                    mapped = mapped.min(new_line_start + transformed.len() - 1);
                }
                *out = mapped;
            } else if old_cursor >= byte && old_cursor <= line_end {
                *out = new_line_start + (old_cursor - byte).min(line_len);
            }
        };
        map_cursor(lo, &mut new_lo);
        map_cursor(hi, &mut new_hi);

        out.push_str(&transformed);
        new_line_start += transformed.len();
        byte = line_end;
    }
    // Positions at the very end of the text.
    if lo >= byte {
        new_lo = out.len();
    }
    if hi >= byte {
        new_hi = out.len();
    }
    (out, new_lo, new_hi)
}

fn indent_line(line: &str, indent: &str) -> String {
    if line == "\n" || line.is_empty() {
        return line.to_string();
    }
    format!("{indent}{line}")
}

/// Removes up to one indent level (tab char or `tab_size` leading spaces).
fn dedent_line(line: &str, tab_size: u8) -> String {
    if let Some(rest) = line.strip_prefix('\t') {
        return rest.to_string();
    }
    let spaces = line.len() - line.trim_start_matches(' ').len();
    let remove = spaces.min(tab_size.max(1) as usize);
    line[remove..].to_string()
}

fn set_cursor_bytes(ui: &egui::Ui, id: egui::Id, text: &str, byte: usize) {
    set_cursor_chars(
        ui,
        id,
        text,
        char_index_from_byte(text, byte),
        char_index_from_byte(text, byte),
    );
}

fn set_cursor_chars(ui: &egui::Ui, id: egui::Id, _text: &str, primary: usize, secondary: usize) {
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
        state.cursor.set_char_range(Some(egui::text::CCursorRange {
            primary: CCursor {
                index: CharIndex(primary),
                prefer_next_row: true,
            },
            secondary: CCursor {
                index: CharIndex(secondary),
                prefer_next_row: false,
            },
            h_pos: None,
        }));
        state.store(ui.ctx(), id);
    }
}

/// Byte offset of the first difference between two strings (the shorter
/// length when one is a prefix of the other).
fn first_diff_byte(old: &str, new: &str) -> usize {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();
    let mut index = 0usize;
    while index < old_bytes.len() && index < new_bytes.len() {
        if old_bytes[index] != new_bytes[index] {
            return index;
        }
        index += 1;
    }
    index
}

/// Builds the full layout job: one section per styled span, gaps filled
/// with the base style, newlines included so the galley matches the buffer.
fn build_layout_job(
    text: &str,
    assets: &mut EditorAssets<'_>,
    wrap_width: f32,
    line_height: f32,
    ui: &egui::Ui,
) -> LayoutJob {
    let base_color = egui::Color32::from_rgba_unmultiplied(
        assets.colors.syntax_text[0],
        assets.colors.syntax_text[1],
        assets.colors.syntax_text[2],
        assets.colors.syntax_text[3],
    );
    let mut base_format = TextFormat::simple(assets.font_id.clone(), base_color);
    base_format.line_height = Some(line_height);

    let mut job = LayoutJob {
        text: text.to_string(),
        wrap: TextWrapping {
            max_width: if assets.wrap {
                wrap_width
            } else {
                f32::INFINITY
            },
            max_rows: usize::MAX,
            break_anywhere: false,
            overflow_character: None,
        },
        ..LayoutJob::default()
    };
    job.keep_trailing_whitespace = true;

    // Local span builder: sections must be contiguous and cover all bytes.
    let mut sections: Vec<LayoutSection> = Vec::new();
    struct SpanBuilder {
        cursor: usize,
    }
    impl SpanBuilder {
        fn push(&mut self, sections: &mut Vec<LayoutSection>, end: usize, format: TextFormat) {
            if end > self.cursor {
                sections.push(LayoutSection {
                    leading_space: 0.0,
                    byte_range: egui::text::ByteIndex(self.cursor)..egui::text::ByteIndex(end),
                    format,
                });
                self.cursor = end;
            }
        }
    }
    let mut builder = SpanBuilder { cursor: 0usize };
    let push_span = |builder: &mut SpanBuilder,
                     sections: &mut Vec<LayoutSection>,
                     end: usize,
                     format: TextFormat| {
        builder.push(sections, end, format);
    };

    let highlight_enabled = assets.engine.is_some()
        && assets.doc_highlight.is_some()
        && assets.language.is_some()
        && assets.syntect_theme.is_some()
        && assets
            .doc_highlight
            .as_ref()
            .map(|doc| doc.enabled)
            .unwrap_or(false);

    let line_count = text.split('\n').count();
    for (line_index, line) in text.split('\n').enumerate() {
        let line_start = builder.cursor;
        let line_len = line.len();

        if highlight_enabled {
            // The engine is re-borrowed immutably for the syntax/theme
            // lookup; the doc cache is the only mutated piece.
            let engine = assets.engine.as_deref_mut().expect("checked");
            let language = assets.language.expect("checked");
            let theme = assets.syntect_theme.as_deref().expect("checked");
            let syntax = engine.syntax_named_or_language(&assets.syntax_name, language);
            let doc = assets.doc_highlight.as_deref_mut().expect("checked");
            let ranges =
                doc.ranges_for(engine, syntax, theme, text, line_index.min(line_count - 1));
            for (style, range) in ranges {
                let start = range.start.min(line_len);
                let end = range.end.min(line_len);
                if start >= end {
                    continue;
                }
                let color = egui::Color32::from_rgba_unmultiplied(
                    style.foreground.r,
                    style.foreground.g,
                    style.foreground.b,
                    255,
                );
                let mut format = base_format.clone();
                format.color = color;
                push_span(&mut builder, &mut sections, line_start + end, format);
            }
        }
        // Cover the rest of the line plus its newline with the base style.
        let inclusive_end = (line_start + line_len + 1)
            .min(text.len())
            .max(line_start + line_len);
        push_span(
            &mut builder,
            &mut sections,
            inclusive_end.min(text.len()),
            base_format.clone(),
        );
        if line_start + line_len >= text.len() {
            break;
        }
    }
    push_span(&mut builder, &mut sections, text.len(), base_format.clone());

    let _ = ui;
    job.sections = sections;
    job
}
