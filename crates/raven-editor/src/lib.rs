//! # raven-editor
//!
//! UI-free editor core for Raven Code, the lightweight editor of the Raven
//! ecosystem. Everything in this crate works without a display, a GUI
//! framework or any async runtime, which keeps it unit-testable and lets
//! future Raven applications (Raven Studio) reuse the exact same logic.
//!
//! Modules:
//!
//! * [`buffer`] — document loading/saving with encoding detection (UTF-8,
//!   UTF-8 BOM, UTF-16 LE/BE), line-ending preservation and large-file policy.
//! * [`search`] — literal and regular-expression search & replace with byte
//!   ranges suitable for caret movement.
//! * [`language`] — the language registry: file extensions map to language
//!   definitions (comments, indentation). Nothing in the editor hardcodes
//!   per-language behaviour; new languages are data, not code.
//! * [`settings`] — the versioned user settings file (`raven-code.toml`),
//!   loaded gracefully: missing or invalid files fall back to defaults and
//!   report a notice instead of failing the launch.
//! * [`theme`] — theme palettes (Raven Dark default, Raven Light) as plain
//!   RGBA data, converted to concrete UI colors by the application layer.
//!
//! Stability contract: the settings schema is versioned (`schema_version = 1`)
//! and evolves additively, mirroring the `raven-core` configuration and
//! report conventions.

pub mod buffer;
pub mod language;
pub mod search;
pub mod settings;
pub mod theme;

/// Human-readable product identity (window titles, dialogs, logs).
pub const APP_DISPLAY_NAME: &str = "Raven Code";

/// Version of the settings schema written and accepted by this crate.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

/// The default settings file name used in the user configuration directory.
pub const SETTINGS_FILE_NAME: &str = "raven-code.toml";
