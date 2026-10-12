//! The language registry.
//!
//! Per-language behaviour in Raven Code is *data*, not scattered code: an
//! extension maps to a [`LanguageDef`] carrying display name, comment
//! tokens and indentation defaults. The UI layer maps a [`LanguageDef`] to
//! a concrete syntax-highlighting grammar; nothing outside this registry
//! should switch on file extensions.

/// Declarative description of a supported language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageDef {
    /// Stable identifier (settings, LSP config keys): "rust", "python", ...
    pub id: &'static str,
    /// Human name ("Rust").
    pub name: &'static str,
    /// File extensions without the dot.
    pub extensions: &'static [&'static str],
    /// Line-comment token for auto-indent/comment actions.
    pub line_comment: Option<&'static str>,
    /// Block-comment delimiters, when the language has them.
    pub block_comment: Option<(&'static str, &'static str)>,
}

macro_rules! lang {
    ($id:literal, $name:literal, $exts:expr, $line:expr, $block:expr) => {
        LanguageDef {
            id: $id,
            name: $name,
            extensions: $exts,
            line_comment: $line,
            block_comment: $block,
        }
    };
}

/// All languages the editor knows about out of the box. The list favors
/// breadth of *file types people edit* over compiler-grade completeness;
/// syntax highlighting coverage is decided by the highlighter, not here.
pub const LANGUAGES: &[LanguageDef] = &[
    lang!("rust", "Rust", &["rs"], Some("//"), Some(("/*", "*/"))),
    lang!("python", "Python", &["py", "pyw", "pyi"], Some("#"), None),
    lang!(
        "javascript",
        "JavaScript",
        &["js", "mjs", "cjs", "jsx"],
        Some("//"),
        Some(("/*", "*/"))
    ),
    lang!(
        "typescript",
        "TypeScript",
        &["ts", "tsx", "mts", "cts"],
        Some("//"),
        Some(("/*", "*/"))
    ),
    lang!("c", "C", &["c", "h"], Some("//"), Some(("/*", "*/"))),
    lang!(
        "cpp",
        "C++",
        &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "ipp"],
        Some("//"),
        Some(("/*", "*/"))
    ),
    lang!("csharp", "C#", &["cs"], Some("//"), Some(("/*", "*/"))),
    lang!("java", "Java", &["java"], Some("//"), Some(("/*", "*/"))),
    lang!(
        "kotlin",
        "Kotlin",
        &["kt", "kts"],
        Some("//"),
        Some(("/*", "*/"))
    ),
    lang!("go", "Go", &["go"], Some("//"), Some(("/*", "*/"))),
    lang!("ruby", "Ruby", &["rb", "erb"], Some("#"), None),
    lang!("php", "PHP", &["php"], Some("//"), Some(("/*", "*/"))),
    lang!("swift", "Swift", &["swift"], Some("//"), Some(("/*", "*/"))),
    lang!("shell", "Shell", &["sh", "bash", "zsh"], Some("#"), None),
    lang!(
        "powershell",
        "PowerShell",
        &["ps1", "psm1"],
        Some("#"),
        None
    ),
    lang!("batch", "Batch", &["bat", "cmd"], Some("::"), None),
    lang!("lua", "Lua", &["lua"], Some("--"), Some(("--[[", "]]"))),
    lang!("gdscript", "GDScript", &["gd"], Some("#"), None),
    lang!(
        "glsl",
        "GLSL",
        &["glsl", "vert", "frag", "comp"],
        Some("//"),
        Some(("/*", "*/"))
    ),
    lang!("sql", "SQL", &["sql"], Some("--"), Some(("/*", "*/"))),
    lang!("toml", "TOML", &["toml"], Some("#"), None),
    lang!("yaml", "YAML", &["yaml", "yml"], Some("#"), None),
    lang!("json", "JSON", &["json", "jsonc"], None, None),
    lang!(
        "xml",
        "XML",
        &["xml", "svg", "xaml", "csproj", "vbproj", "props", "targets"],
        None,
        Some(("<!--", "-->"))
    ),
    lang!(
        "html",
        "HTML",
        &["html", "htm"],
        None,
        Some(("<!--", "-->"))
    ),
    lang!("css", "CSS", &["css"], None, Some(("/*", "*/"))),
    lang!("scss", "SCSS", &["scss"], Some("//"), Some(("/*", "*/"))),
    lang!("markdown", "Markdown", &["md", "markdown"], None, None),
    lang!(
        "ini",
        "INI",
        &["ini", "cfg", "conf", "properties", "editorconfig"],
        Some(";"),
        None
    ),
    lang!("make", "Makefile", &["mk"], Some("#"), None),
    lang!("cmake", "CMake", &["cmake"], Some("#"), None),
    lang!(
        "haskell",
        "Haskell",
        &["hs"],
        Some("--"),
        Some(("{-", "-}"))
    ),
    lang!("elixir", "Elixir", &["ex", "exs"], Some("#"), None),
    lang!("r", "R", &["r", "R"], Some("#"), None),
    lang!("perl", "Perl", &["pl", "pm"], Some("#"), None),
    lang!("dart", "Dart", &["dart"], Some("//"), Some(("/*", "*/"))),
    lang!("vue", "Vue", &["vue"], None, Some(("<!--", "-->"))),
    lang!("zig", "Zig", &["zig"], Some("//"), None),
    lang!("julia", "Julia", &["jl"], Some("#"), None),
];

/// Plain-text fallback for extensions without a definition.
pub const PLAIN_TEXT: LanguageDef = lang!("plaintext", "Plain Text", &["txt"], Some("#"), None);

/// Looks up a language by extension (case-insensitive, no dot).
pub fn for_extension(ext: &str) -> Option<&'static LanguageDef> {
    let needle = ext.trim_start_matches('.').to_ascii_lowercase();
    LANGUAGES.iter().find(|def| {
        def.extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&needle))
    })
}

/// Looks up a language by stable id.
pub fn for_id(id: &str) -> Option<&'static LanguageDef> {
    LANGUAGES.iter().find(|def| def.id == id)
}

/// Resolves the language for a path, falling back to [`PLAIN_TEXT`].
pub fn for_path(path: &std::path::Path) -> &'static LanguageDef {
    path.extension()
        .and_then(|ext| for_extension(&ext.to_string_lossy()))
        .unwrap_or(&PLAIN_TEXT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn common_extensions_resolve() {
        assert_eq!(for_extension("rs").unwrap().id, "rust");
        assert_eq!(for_extension(".PY").unwrap().id, "python");
        assert_eq!(for_extension("TSX").unwrap().id, "typescript");
        assert_eq!(for_extension("gd").unwrap().id, "gdscript");
        assert_eq!(for_path(Path::new("no.extension.here")), &PLAIN_TEXT);
    }

    #[test]
    fn ids_resolve_and_are_unique() {
        assert_eq!(for_id("rust").unwrap().name, "Rust");
        let mut ids: Vec<_> = LANGUAGES.iter().map(|def| def.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "language ids must be unique");
    }

    #[test]
    fn unknown_extensions_fall_back_to_plain_text() {
        assert!(for_extension("xyzzy").is_none());
        assert_eq!(for_path(Path::new("readme.xyzzy")), &PLAIN_TEXT);
        assert_eq!(for_path(Path::new("Makefile")), &PLAIN_TEXT);
    }
}
