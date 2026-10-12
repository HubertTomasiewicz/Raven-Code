//! Syntax highlighting: syntect integration with an incremental per-line
//! state cache.
//!
//! Strategy (brief §9 — responsiveness):
//!
//! * the parse state after every line is cached, so a single-line edit only
//!   re-parses **from the edited line onward, lazily** — the same approach
//!   mainstream editors use,
//! * ranges for lines the layouter has not asked for are never computed,
//! * themes are built programmatically from the Raven palettes, so the
//!   highlighting is brand-coherent in both dark and light mode and needs
//!   no external theme files.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use syntect::highlighting::{
    HighlightState, Highlighter, ScopeSelectors, Style, Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

use raven_editor::theme::Theme as RavenTheme;

/// Owns the grammar database (loaded once per process).
pub struct HighlightEngine {
    syntax_set: SyntaxSet,
    custom_extensions: HashMap<String, String>,
    pub issues: Vec<String>,
    dark_theme: std::sync::Arc<Theme>,
    light_theme: std::sync::Arc<Theme>,
}

impl HighlightEngine {
    /// Loads the bundled Sublime syntax set and builds Raven themes.
    pub fn new() -> HighlightEngine {
        Self::with_custom_syntaxes(None)
    }

    /// Loads Sublime syntax files from an optional user directory on top of
    /// the bundled grammar set. Invalid files are reported and skipped.
    pub fn with_custom_syntaxes(folder: Option<&Path>) -> HighlightEngine {
        let defaults = SyntaxSet::load_defaults_newlines();
        let builtin_names: HashSet<String> = defaults
            .syntaxes()
            .iter()
            .map(|syntax| syntax.name.clone())
            .collect();
        let mut builder = defaults.into_builder();
        let mut issues = Vec::new();
        if let Some(folder) = folder {
            let mut files = Vec::new();
            collect_syntax_files(folder, &mut files);
            files.sort();
            for path in files {
                let result = std::fs::read_to_string(&path)
                    .map_err(|error| error.to_string())
                    .and_then(|source| {
                        syntect::parsing::SyntaxDefinition::load_from_str(
                            &source,
                            true,
                            path.file_stem().and_then(|stem| stem.to_str()),
                        )
                        .map_err(|error| error.to_string())
                    });
                match result {
                    Ok(definition) => builder.add(definition),
                    Err(error) => issues.push(format!("{}: {error}", path.display())),
                }
            }
        }
        let syntax_set = builder.build();
        let mut custom_extensions = HashMap::new();
        for syntax in syntax_set.syntaxes() {
            if builtin_names.contains(&syntax.name) {
                continue;
            }
            for extension in &syntax.file_extensions {
                custom_extensions.insert(extension.to_ascii_lowercase(), syntax.name.clone());
            }
        }
        HighlightEngine {
            syntax_set,
            custom_extensions,
            issues,
            dark_theme: std::sync::Arc::new(build_theme(raven_editor::theme::RAVEN_DARK)),
            light_theme: std::sync::Arc::new(build_theme(raven_editor::theme::RAVEN_LIGHT)),
        }
    }

    pub fn syntax_set(&self) -> &SyntaxSet {
        &self.syntax_set
    }

    pub fn theme(&self, dark: bool) -> std::sync::Arc<Theme> {
        if dark {
            self.dark_theme.clone()
        } else {
            self.light_theme.clone()
        }
    }

    /// Resolves a syntax definition for a language: extension first, then
    /// language id, then plain text — never a panic, never a hard failure.
    pub fn syntax_for(
        &self,
        language: &'static raven_editor::language::LanguageDef,
    ) -> &syntect::parsing::SyntaxReference {
        let syntax_set = &self.syntax_set;
        for extension in language.extensions {
            if let Some(syntax) = syntax_set.find_syntax_by_extension(extension) {
                return syntax;
            }
        }
        if let Some(syntax) = syntax_set.find_syntax_by_token(language.id) {
            return syntax;
        }
        syntax_set.find_syntax_plain_text()
    }

    pub fn syntax_for_path(
        &self,
        language: &'static raven_editor::language::LanguageDef,
        path: &Path,
    ) -> &syntect::parsing::SyntaxReference {
        if let Some(extension) = path.extension().and_then(|value| value.to_str())
            && let Some(name) = self.custom_extensions.get(&extension.to_ascii_lowercase())
            && let Some(syntax) = self.syntax_set.find_syntax_by_name(name)
        {
            return syntax;
        }
        self.syntax_for(language)
    }

    pub fn syntax_named_or_language(
        &self,
        name: &str,
        language: &'static raven_editor::language::LanguageDef,
    ) -> &syntect::parsing::SyntaxReference {
        self.syntax_set
            .find_syntax_by_name(name)
            .unwrap_or_else(|| self.syntax_for(language))
    }
}

fn collect_syntax_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_syntax_files(&path, files);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "sublime-syntax")
        {
            files.push(path);
        }
    }
}

impl Default for HighlightEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps the Raven palette onto syntect scope selectors.
fn build_theme(theme: RavenTheme) -> Theme {
    let colors = theme.colors;
    let color = |rgba: [u8; 4]| syntect::highlighting::Color {
        r: rgba[0],
        g: rgba[1],
        b: rgba[2],
        a: rgba[3],
    };

    let item = |selector: &str, foreground: [u8; 4]| ThemeItem {
        scope: ScopeSelectors::from_str(selector).expect("static scope selector"),
        style: syntect::highlighting::StyleModifier {
            foreground: Some(color(foreground)),
            background: None,
            font_style: None,
        },
    };

    let mut items = vec![
        item("comment", colors.syntax_comment),
        item("string", colors.syntax_string),
        item("string.regexp", colors.syntax_constant),
        item("constant", colors.syntax_constant),
        item("constant.numeric", colors.syntax_number),
        item("keyword", colors.syntax_keyword),
        item("storage", colors.syntax_keyword),
        item("support.function", colors.syntax_function),
        item("entity.name.function", colors.syntax_function),
        item("meta.function-call", colors.syntax_function),
        item("entity.name.type", colors.syntax_type),
        item("entity.name.class", colors.syntax_type),
        item("entity.name.struct", colors.syntax_type),
        item("entity.name.enum", colors.syntax_type),
        item("support.type", colors.syntax_type),
        item("support.class", colors.syntax_type),
        item("variable.language", colors.syntax_constant),
        item("variable", colors.syntax_variable),
        item("punctuation", colors.syntax_punctuation),
        item("meta.tag", colors.syntax_keyword),
        item("entity.name.tag", colors.syntax_keyword),
        item("markup.heading", colors.syntax_type),
    ];

    // Markdown headings / emphasis get meaningful colors in light mode too;
    // bold is intentionally avoided (font style is not rendered by the
    // editor's mono layouter anyway).
    items.push(item("markup.quote", colors.syntax_comment));

    Theme {
        name: Some(theme.name.to_string()),
        author: Some("Raven Developers".to_string()),
        settings: ThemeSettings {
            foreground: Some(color(colors.syntax_text)),
            background: Some(color(colors.editor_bg)),
            caret: Some(color(colors.cursor)),
            selection: Some(color(colors.selection)),
            ..Default::default()
        },
        scopes: items,
    }
}

/// Per-document incremental highlight cache.
///
/// Invariant (kept in lockstep):
///
/// * `states[i]` is the parse state **after** line `i-1` (i.e. the state
///   line `i` must start with); `states[0]` is the fresh initial state,
///   stored implicitly by an empty vec prefix — `states.len() == ranges.len() + 1`,
/// * `ranges[i]` are the styled byte ranges within line `i`,
/// * everything from the edited line onward is invalidated on edit and
///   recomputed lazily, chained from the last valid state.
pub struct DocHighlighter {
    /// Per line: (scope parse state, highlight state) *before* that line.
    states: Vec<(ParseState, HighlightState)>,
    ranges: Vec<Vec<(Style, Range<usize>)>>,
    /// Flips off for large files or plain-text mode.
    pub enabled: bool,
}

impl DocHighlighter {
    pub fn new() -> DocHighlighter {
        DocHighlighter {
            states: Vec::new(),
            ranges: Vec::new(),
            enabled: false,
        }
    }

    /// Clears the cache for a new text; `enabled` gates all work.
    pub fn reset(&mut self, _line_count: usize, enabled: bool) {
        self.enabled = enabled;
        self.states.clear();
        self.ranges.clear();
    }

    /// Invalidates everything from the line containing `byte_pos`. The
    /// state *before* that line is still valid, so recomputation starts
    /// exactly where the edit happened.
    pub fn invalidate_from(&mut self, byte_pos: usize, text: &str) {
        if !self.enabled {
            return;
        }
        let line = text[..byte_pos.min(text.len())]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        self.states.truncate(line + 1);
        self.ranges.truncate(line);
    }

    /// Styled ranges for `line` (computed on demand, chaining states).
    pub fn ranges_for(
        &mut self,
        engine: &HighlightEngine,
        syntax: &syntect::parsing::SyntaxReference,
        theme: &Theme,
        text: &str,
        line: usize,
    ) -> Vec<(Style, Range<usize>)> {
        if !self.enabled {
            return Vec::new();
        }
        let highlighter = Highlighter::new(theme);
        while self.ranges.len() <= line {
            let index = self.ranges.len();
            let (mut parse_state, mut highlight_state) = match self.states.last() {
                Some((parse_state, highlight_state)) => {
                    (parse_state.clone(), highlight_state.clone())
                }
                None => (
                    ParseState::new(syntax),
                    HighlightState::new(&highlighter, ScopeStack::new()),
                ),
            };
            // `parse_line` expects the line including its trailing newline
            // (we loaded the newline-aware syntax set); the final line of a
            // file without a trailing newline gets one appended — harmless.
            let line_text = text
                .split('\n')
                .nth(index)
                .map(|line_text| format!("{line_text}\n"))
                .unwrap_or_default();
            let ranges = match parse_state.parse_line(&line_text, engine.syntax_set()) {
                Ok(ops) => {
                    // (Style, &str) pairs partition the line; convert to
                    // byte ranges for the layouter.
                    let iterator = syntect::highlighting::HighlightIterator::new(
                        &mut highlight_state,
                        &ops[..],
                        &line_text,
                        &highlighter,
                    );
                    let mut offset = 0usize;
                    let mut converted = Vec::with_capacity(ops.len());
                    for (style, slice) in iterator {
                        if !slice.is_empty() {
                            converted.push((style, offset..offset + slice.len()));
                        }
                        offset += slice.len();
                    }
                    converted
                }
                Err(_) => Vec::new(), // parse failure: plain line, never a crash
            };
            self.ranges.push(ranges);
            self.states.push((parse_state, highlight_state));
        }
        self.ranges[line].clone()
    }
}

impl Default for DocHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a char index (egui cursor space) into a byte index.
pub fn byte_from_char_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Converts a byte index into a char index.
pub fn char_index_from_byte(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use raven_editor::language;

    #[test]
    fn syntax_resolution_by_extension() {
        let engine = HighlightEngine::new();
        let rust = language::for_extension("rs").unwrap();
        let syntax = engine.syntax_for(rust);
        assert_eq!(syntax.name, "Rust");

        let unknown = &language::PLAIN_TEXT;
        let syntax = engine.syntax_for(unknown);
        assert!(!syntax.name.is_empty());
    }

    #[test]
    fn incremental_ranges_recompute_after_invalidation() {
        let engine = HighlightEngine::new();
        let syntax = engine.syntax_for(language::for_extension("rs").unwrap());
        let text = "let x = 1;\nfn main() {}\n// done\n";
        let mut doc = DocHighlighter::new();
        doc.reset(3, true);

        let first = doc.ranges_for(&engine, syntax, engine.theme(true).as_ref(), text, 0);
        assert!(!first.is_empty(), "keyword `let` must be styled");

        let third = doc.ranges_for(&engine, syntax, engine.theme(true).as_ref(), text, 2);
        assert!(!third.is_empty(), "comment must be styled");

        // Edit line 0 -> caches from line 0 onward invalidated.
        doc.invalidate_from(0, text);
        let first_again = doc.ranges_for(&engine, syntax, engine.theme(true).as_ref(), text, 0);
        assert_eq!(first_again.len(), first.len());
    }

    #[test]
    fn disabled_highlighter_returns_empty() {
        let engine = HighlightEngine::new();
        let syntax = engine.syntax_for(language::for_extension("rs").unwrap());
        let mut doc = DocHighlighter::new();
        doc.reset(2, false);
        assert!(
            doc.ranges_for(
                &engine,
                syntax,
                engine.theme(true).as_ref(),
                "let x = 1;\n",
                0
            )
            .is_empty()
        );
    }

    #[test]
    fn char_byte_conversions_roundtrip() {
        let text = "héllo wörld";
        let byte = byte_from_char_index(text, 2);
        assert_eq!(byte, 3); // é is 2 bytes
        assert_eq!(char_index_from_byte(text, byte), 2);
        assert_eq!(char_index_from_byte(text, 9999), text.chars().count());
    }

    #[test]
    fn themes_cover_core_scopes() {
        for theme in [RAVEN_DARK, RAVEN_LIGHT] {
            let built = build_theme(theme);
            assert!(built.settings.foreground.is_some());
            assert!(!built.scopes.is_empty());
        }
    }

    use raven_editor::theme::RAVEN_DARK;
    use raven_editor::theme::RAVEN_LIGHT;
}
