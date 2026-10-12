//! Theme palettes as plain data.
//!
//! Colors are stored as 8-bit RGBA tuples so this crate stays UI-free; the
//! application layer converts them to its concrete color type. The two
//! built-in themes ship with the editor:
//!
//! * **Raven Dark** (default) — near-black slate surfaces with the Raven
//!   Code cyan accent,
//! * **Raven Light** — a coherent light counterpart using a darker cyan for
//!   contrast on white.
//!
//! Custom themes: any settings `theme` value that does not resolve to a
//! built-in falls back to the dark default (documented behavior).

/// A full editor theme. Every field is used by the UI layer; adding fields
/// is additive and does not break stored settings (themes are code-defined).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: bool,
    pub colors: ThemeColors,
}

/// All named colors used across the interface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeColors {
    // Surfaces
    pub window_bg: [u8; 4],
    pub panel_bg: [u8; 4],
    pub panel_header_bg: [u8; 4],
    pub editor_bg: [u8; 4],
    pub status_bg: [u8; 4],
    pub input_bg: [u8; 4],
    pub overlay_bg: [u8; 4],

    // Text
    pub text: [u8; 4],
    pub text_dim: [u8; 4],
    pub text_bright: [u8; 4],

    // Structure
    pub border: [u8; 4],
    pub accent: [u8; 4],
    pub accent_dim: [u8; 4],
    pub selection: [u8; 4],
    pub cursor: [u8; 4],
    pub line_highlight: [u8; 4],
    pub gutter_text: [u8; 4],
    pub gutter_active_text: [u8; 4],
    pub tab_active_bg: [u8; 4],
    pub tab_inactive_bg: [u8; 4],
    pub hover: [u8; 4],

    // Status / diagnostics
    pub error: [u8; 4],
    pub warning: [u8; 4],
    pub info: [u8; 4],
    pub success: [u8; 4],

    // Syntax (coherent with the accent family)
    pub syntax_text: [u8; 4],
    pub syntax_keyword: [u8; 4],
    pub syntax_string: [u8; 4],
    pub syntax_number: [u8; 4],
    pub syntax_comment: [u8; 4],
    pub syntax_function: [u8; 4],
    pub syntax_type: [u8; 4],
    pub syntax_constant: [u8; 4],
    pub syntax_punctuation: [u8; 4],
    pub syntax_variable: [u8; 4],
}

/// The default dark theme (also the fallback for unknown theme ids).
pub const RAVEN_DARK: Theme = Theme {
    id: "raven-dark",
    name: "Raven Dark",
    dark: true,
    colors: ThemeColors {
        window_bg: [0x30, 0x30, 0x30, 0xFF], // matches crow_icon.svg
        panel_bg: [0x27, 0x27, 0x27, 0xFF],
        panel_header_bg: [0x2B, 0x2B, 0x2B, 0xFF],
        editor_bg: [0x30, 0x30, 0x30, 0xFF],
        status_bg: [0x25, 0x25, 0x25, 0xFF],
        input_bg: [0x1D, 0x1D, 0x1D, 0xFF],
        overlay_bg: [0x2A, 0x2A, 0x2A, 0xFF],

        text: [0xD6, 0xDB, 0xE1, 0xFF],
        text_dim: [0x8B, 0x94, 0x9E, 0xFF],
        text_bright: [0xF0, 0xF3, 0xF6, 0xFF],

        border: [0x2A, 0x30, 0x38, 0xFF],
        accent: [0x2F, 0xC4, 0xDE, 0xFF], // Raven Code cyan
        accent_dim: [0x1F, 0x8A, 0x9E, 0xFF],
        selection: [0x14, 0x42, 0x55, 0x88],
        cursor: [0x2F, 0xC4, 0xDE, 0xFF],
        line_highlight: [0x38, 0x38, 0x38, 0xFF],
        gutter_text: [0x56, 0x60, 0x6C, 0xFF],
        gutter_active_text: [0xC4, 0xCB, 0xD3, 0xFF],
        tab_active_bg: [0x16, 0x19, 0x1E, 0xFF],
        tab_inactive_bg: [0x1B, 0x1F, 0x24, 0xFF],
        hover: [0x24, 0x2A, 0x31, 0xFF],

        error: [0xE5, 0x53, 0x4B, 0xFF],
        warning: [0xD4, 0xA7, 0x2C, 0xFF],
        info: [0x53, 0x9B, 0xF5, 0xFF],
        success: [0x57, 0xAB, 0x5A, 0xFF],

        syntax_text: [0xCE, 0xD6, 0xDF, 0xFF],
        syntax_keyword: [0x4E, 0xC5, 0xE0, 0xFF], // cyan family
        syntax_string: [0x8D, 0xCC, 0x6E, 0xFF],  // brand green nod
        syntax_number: [0xE2, 0xA8, 0xFF, 0xFF],
        syntax_comment: [0x63, 0x6E, 0x7B, 0xFF],
        syntax_function: [0x6C, 0xB6, 0xFF, 0xFF],
        syntax_type: [0x7E, 0xE7, 0x87, 0xFF],
        syntax_constant: [0xE8, 0xB3, 0x39, 0xFF],
        syntax_punctuation: [0x9A, 0xA4, 0xB0, 0xFF],
        syntax_variable: [0xCE, 0xD6, 0xDF, 0xFF],
    },
};

/// The coherent light theme.
pub const RAVEN_LIGHT: Theme = Theme {
    id: "raven-light",
    name: "Raven Light",
    dark: false,
    colors: ThemeColors {
        window_bg: [0xF6, 0xF8, 0xFA, 0xFF],
        panel_bg: [0xEC, 0xF0, 0xF4, 0xFF],
        panel_header_bg: [0xE3, 0xE8, 0xEE, 0xFF],
        editor_bg: [0xFF, 0xFF, 0xFF, 0xFF],
        status_bg: [0xE8, 0xEC, 0xF1, 0xFF],
        input_bg: [0xFF, 0xFF, 0xFF, 0xFF],
        overlay_bg: [0xF9, 0xFB, 0xFC, 0xFF],

        text: [0x1F, 0x23, 0x28, 0xFF],
        text_dim: [0x63, 0x6D, 0x76, 0xFF],
        text_bright: [0x10, 0x14, 0x18, 0xFF],

        border: [0xD0, 0xD7, 0xDE, 0xFF],
        accent: [0x00, 0x7F, 0x9E, 0xFF], // darker cyan for contrast
        accent_dim: [0x33, 0x9D, 0xB8, 0xFF],
        selection: [0xB6, 0xE0, 0xEA, 0x88],
        cursor: [0x00, 0x7F, 0x9E, 0xFF],
        line_highlight: [0xF2, 0xF5, 0xF8, 0xFF],
        gutter_text: [0x9E, 0xA7, 0xB1, 0xFF],
        gutter_active_text: [0x3C, 0x44, 0x4D, 0xFF],
        tab_active_bg: [0xFF, 0xFF, 0xFF, 0xFF],
        tab_inactive_bg: [0xEC, 0xF0, 0xF4, 0xFF],
        hover: [0xE4, 0xE9, 0xEE, 0xFF],

        error: [0xCF, 0x22, 0x22, 0xFF],
        warning: [0x9A, 0x67, 0x00, 0xFF],
        info: [0x05, 0x50, 0xAE, 0xFF],
        success: [0x1A, 0x7F, 0x37, 0xFF],

        syntax_text: [0x24, 0x29, 0x2F, 0xFF],
        syntax_keyword: [0x05, 0x50, 0xAE, 0xFF],
        syntax_string: [0x0A, 0x63, 0x06, 0xFF],
        syntax_number: [0x05, 0x50, 0xAE, 0xFF],
        syntax_comment: [0x6E, 0x77, 0x81, 0xFF],
        syntax_function: [0x82, 0x5D, 0xF2, 0xFF],
        syntax_type: [0x11, 0x63, 0xC9, 0xFF],
        syntax_constant: [0x95, 0x38, 0x00, 0xFF],
        syntax_punctuation: [0x45, 0x4C, 0x54, 0xFF],
        syntax_variable: [0x24, 0x29, 0x2F, 0xFF],
    },
};

/// All built-in themes.
pub const BUILT_INS: &[Theme] = &[RAVEN_DARK, RAVEN_LIGHT];

/// Resolves a theme id (settings value) to a theme; unknown ids fall back
/// to the dark default.
pub fn resolve(theme_id: &str) -> &'static Theme {
    BUILT_INS
        .iter()
        .find(|theme| theme.id.eq_ignore_ascii_case(theme_id))
        .unwrap_or(&RAVEN_DARK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_builtins_and_falls_back() {
        assert_eq!(resolve("raven-dark").id, "raven-dark");
        assert_eq!(resolve("RAVEN-LIGHT").id, "raven-light");
        assert_eq!(resolve("nonexistent").id, "raven-dark");
    }

    #[test]
    fn themes_are_distinct_and_complete() {
        assert_ne!(RAVEN_DARK.colors.window_bg, RAVEN_LIGHT.colors.window_bg);
        for theme in BUILT_INS {
            // Every alpha must be opaque or a deliberate translucent overlay.
            for (name, color) in [
                ("accent", theme.colors.accent),
                ("text", theme.colors.text),
                ("error", theme.colors.error),
            ] {
                assert_eq!(color[3], 0xFF, "{}.{} must be opaque", theme.id, name);
            }
        }
    }
}
