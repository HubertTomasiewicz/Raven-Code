//! The versioned user settings file (`raven-code.toml`).
//!
//! Conventions mirror `raven-core::config`:
//!
//! * the file carries a `schema_version`; this build reads version 1 and
//!   refuses *newer* files with an upgrade notice instead of guessing,
//! * unknown keys are ignored (forward compatibility inside a version),
//! * loading is graceful: a missing file yields defaults, an invalid file
//!   yields defaults **plus a notice** the UI shows — the editor never
//!   refuses to start because of a broken settings file,
//! * values are clamped to sane ranges after load.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{SETTINGS_FILE_NAME, SETTINGS_SCHEMA_VERSION};

/// Largest number of entries kept in any recent list.
pub const RECENT_LIMIT: usize = 10;

/// Outcome of [`Settings::load_from`] — the settings plus an optional
/// human-readable notice (missing file produces no notice by design).
#[derive(Debug, Clone)]
pub struct LoadedSettings {
    pub settings: Settings,
    pub notice: Option<String>,
}

/// One configured language server (`[lsp.servers.<id>]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerEntry {
    /// Executable invoked directly (no shell).
    pub command: String,
    /// Arguments passed verbatim.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Language ids (from the language registry) this server handles.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
}

/// The whole user settings document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub appearance: Appearance,
    pub editor: EditorSettings,
    pub window: WindowSettings,
    pub workspace: WorkspaceSettings,
    pub notifications: NotificationSettings,
    pub keys: BTreeMap<String, String>,
    pub lsp: LspSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// "raven-dark" (default) or "raven-light".
    pub theme: String,
    /// Base editor font size in points.
    pub font_size: f32,
    /// Optional path to a .ttf/.otf used for the editor font.
    pub font_path: String,
    /// Scale factor applied by zoom commands (persisted).
    pub zoom: f32,
    /// Suppress nonessential UI motion and use immediate transitions.
    pub reduce_motion: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Width of a tab stop: 1..=16.
    pub tab_size: u8,
    /// Insert spaces instead of a literal tab character.
    pub insert_spaces: bool,
    /// Soft-wrap long lines.
    pub word_wrap: bool,
    /// Autosave delay after the last edit in milliseconds; 0 disables it.
    pub autosave_delay_ms: u64,
    /// Render the line-number gutter.
    pub line_numbers: bool,
    /// Master switch for syntax highlighting.
    pub highlighting: bool,
    /// Files larger than this (bytes) open without highlighting.
    pub highlight_off_above_bytes: u64,
    /// Files larger than this (bytes) open read-only.
    pub readonly_above_bytes: u64,
    /// Show whitespace markers? Reserved; unused in 0.3.
    pub show_whitespace: bool,
    /// External terminal command override ("gnome-terminal --working-directory" style).
    /// Empty means the platform default is used.
    pub external_terminal: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    pub inner_width: f32,
    pub inner_height: f32,
    pub maximized: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceSettings {
    pub sidebar_visible: bool,
    pub bottom_visible: bool,
    /// Reopen the last folder/file on launch.
    pub restore_last_session: bool,
    /// Bounded recent lists (most recent first).
    pub recent_files: Vec<String>,
    pub recent_folders: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub duration_ms: u64,
    pub max_visible: u8,
    pub save_success: bool,
    pub build_results: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        NotificationSettings {
            enabled: true,
            duration_ms: 5_000,
            max_visible: 4,
            save_success: true,
            build_results: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LspSettings {
    /// Language servers by stable id; started lazily per language.
    pub servers: BTreeMap<String, ServerEntry>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            appearance: Appearance::default(),
            editor: EditorSettings::default(),
            window: WindowSettings::default(),
            workspace: WorkspaceSettings::default(),
            notifications: NotificationSettings::default(),
            keys: BTreeMap::new(),
            lsp: LspSettings::default(),
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            theme: "raven-dark".to_string(),
            font_size: 14.0,
            font_path: String::new(),
            zoom: 1.0,
            reduce_motion: false,
        }
    }
}

impl Default for EditorSettings {
    fn default() -> Self {
        EditorSettings {
            tab_size: 4,
            insert_spaces: true,
            word_wrap: true,
            autosave_delay_ms: 900,
            line_numbers: true,
            highlighting: true,
            highlight_off_above_bytes: 2 * 1024 * 1024,
            readonly_above_bytes: 16 * 1024 * 1024,
            show_whitespace: false,
            external_terminal: String::new(),
        }
    }
}

impl Default for WindowSettings {
    fn default() -> Self {
        WindowSettings {
            inner_width: 1280.0,
            inner_height: 800.0,
            maximized: false,
        }
    }
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        WorkspaceSettings {
            sidebar_visible: true,
            bottom_visible: false,
            restore_last_session: true,
            recent_files: Vec::new(),
            recent_folders: Vec::new(),
        }
    }
}

impl Settings {
    /// Loads settings from the given directory (`raven-code.toml` inside),
    /// falling back to defaults with a notice on any problem.
    pub fn load_from(dir: &Path) -> LoadedSettings {
        let path = dir.join(SETTINGS_FILE_NAME);
        Self::load(&path)
    }

    /// Loads settings from an explicit file path.
    pub fn load(path: &Path) -> LoadedSettings {
        match fs::read_to_string(path) {
            Err(_) => LoadedSettings {
                settings: Settings::default(),
                notice: None, // first launch: silent defaults
            },
            Ok(raw) => match toml::from_str::<Settings>(&raw) {
                Ok(mut settings) => {
                    if settings.schema_version > SETTINGS_SCHEMA_VERSION {
                        return LoadedSettings {
                            settings: Settings::default(),
                            notice: Some(format!(
                                "{} requests schema version {} but this build supports \
                                 version {}; defaults are in use. Please update Raven Code.",
                                SETTINGS_FILE_NAME,
                                settings.schema_version,
                                SETTINGS_SCHEMA_VERSION
                            )),
                        };
                    }
                    settings.clamp();
                    LoadedSettings {
                        settings,
                        notice: None,
                    }
                }
                Err(error) => LoadedSettings {
                    settings: Settings::default(),
                    notice: Some(format!(
                        "{} could not be parsed and was ignored ({error}); defaults are in use",
                        SETTINGS_FILE_NAME
                    )),
                },
            },
        }
    }

    /// Serializes the settings to the given directory.
    pub fn save_to(&self, dir: &Path) -> Result<PathBuf, String> {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        let path = dir.join(SETTINGS_FILE_NAME);
        let body = toml::to_string_pretty(&self).map_err(|error| error.to_string())?;
        let document = format!(
            "# Raven Code user settings (schema version {})\n\
             # Unknown keys are ignored; delete this file to reset everything to defaults.\n\n{body}",
            SETTINGS_SCHEMA_VERSION
        );
        fs::write(&path, document).map_err(|error| error.to_string())?;
        Ok(path)
    }

    /// Adds a path to a recent list (deduplicated, most-recent-first,
    /// bounded by [`RECENT_LIMIT`]).
    pub fn push_recent(list: &mut Vec<String>, entry: String) {
        list.retain(|item| item != &entry);
        list.insert(0, entry);
        list.truncate(RECENT_LIMIT);
    }

    /// Applies range clamps so a hand-edited file cannot produce absurd UI.
    fn clamp(&mut self) {
        self.appearance.font_size = self.appearance.font_size.clamp(7.0, 72.0);
        self.appearance.zoom = self.appearance.zoom.clamp(0.4, 3.0);
        self.editor.tab_size = self.editor.tab_size.clamp(1, 16);
        if self.editor.autosave_delay_ms > 0 {
            self.editor.autosave_delay_ms = self.editor.autosave_delay_ms.clamp(250, 10_000);
        }
        if self.editor.readonly_above_bytes < self.editor.highlight_off_above_bytes {
            self.editor.readonly_above_bytes = self.editor.highlight_off_above_bytes;
        }
        self.workspace.recent_files.truncate(RECENT_LIMIT);
        self.workspace.recent_folders.truncate(RECENT_LIMIT);
        self.notifications.duration_ms = self.notifications.duration_ms.clamp(1_500, 30_000);
        self.notifications.max_visible = self.notifications.max_visible.clamp(1, 8);
    }

    /// Convenience accessor resolving the effective editor font size.
    pub fn effective_font_size(&self) -> f32 {
        (self.appearance.font_size * self.appearance.zoom).clamp(6.0, 96.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("raven-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_is_silent_defaults() {
        let loaded = Settings::load(Path::new("/nonexistent/raven-code.toml"));
        assert!(loaded.notice.is_none());
        assert_eq!(loaded.settings.editor.tab_size, 4);
        assert_eq!(loaded.settings.appearance.theme, "raven-dark");
    }

    #[test]
    fn roundtrip_preserves_all_sections() {
        let dir = tmp_dir("roundtrip");
        let mut settings = Settings::default();
        settings.appearance.theme = "raven-light".to_string();
        settings.editor.tab_size = 2;
        settings.editor.insert_spaces = false;
        settings.workspace.recent_files.push("/tmp/a.rs".into());
        settings
            .keys
            .insert("open_command_palette".into(), "Ctrl+K".into());
        settings.lsp.servers.insert(
            "rust-analyzer".into(),
            ServerEntry {
                command: "rust-analyzer".into(),
                args: vec!["--quiet".into()],
                languages: vec!["rust".into()],
            },
        );
        let path = settings.save_to(&dir).unwrap();

        let loaded = Settings::load(&path);
        assert!(loaded.notice.is_none());
        assert_eq!(loaded.settings.appearance.theme, "raven-light");
        assert_eq!(loaded.settings.editor.tab_size, 2);
        assert!(!loaded.settings.editor.insert_spaces);
        assert_eq!(loaded.settings.workspace.recent_files, vec!["/tmp/a.rs"]);
        assert_eq!(
            loaded
                .settings
                .keys
                .get("open_command_palette")
                .map(String::as_str),
            Some("Ctrl+K")
        );
        assert_eq!(
            loaded
                .settings
                .lsp
                .servers
                .get("rust-analyzer")
                .unwrap()
                .command,
            "rust-analyzer"
        );
    }

    #[test]
    fn invalid_file_falls_back_with_notice() {
        let dir = tmp_dir("invalid");
        let path = dir.join(SETTINGS_FILE_NAME);
        fs::write(&path, "totally = not [toml").unwrap();
        let loaded = Settings::load(&path);
        assert!(loaded.notice.is_some());
        assert_eq!(loaded.settings, Settings::default());
    }

    #[test]
    fn future_schema_version_is_refused_with_notice() {
        let dir = tmp_dir("future");
        let path = dir.join(SETTINGS_FILE_NAME);
        fs::write(&path, "schema_version = 99\n[editor]\ntab_size = 8\n").unwrap();
        let loaded = Settings::load(&path);
        assert!(
            loaded
                .notice
                .as_deref()
                .unwrap_or_default()
                .contains("schema version 99")
        );
        assert_eq!(loaded.settings, Settings::default());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let dir = tmp_dir("unknown");
        let path = dir.join(SETTINGS_FILE_NAME);
        fs::write(
            &path,
            "schema_version = 1\nsome_future_key = true\n[editor]\ntab_size = 2\n",
        )
        .unwrap();
        let loaded = Settings::load(&path);
        assert!(loaded.notice.is_none());
        assert_eq!(loaded.settings.editor.tab_size, 2);
    }

    #[test]
    fn values_are_clamped() {
        let dir = tmp_dir("clamp");
        let path = dir.join(SETTINGS_FILE_NAME);
        fs::write(
            &path,
            "schema_version = 1\n[appearance]\nfont_size = 500.0\nzoom = 99.0\n[editor]\ntab_size = 64\n",
        )
        .unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.settings.appearance.font_size, 72.0);
        assert_eq!(loaded.settings.appearance.zoom, 3.0);
        assert_eq!(loaded.settings.editor.tab_size, 16);
    }

    #[test]
    fn recent_lists_stay_bounded_and_deduped() {
        let mut list: Vec<String> = Vec::new();
        for index in 0..(RECENT_LIMIT as i32 + 5) {
            Settings::push_recent(&mut list, format!("file{index}"));
        }
        assert_eq!(list.len(), RECENT_LIMIT);
        assert_eq!(list[0], "file14");
        Settings::push_recent(&mut list, "file7".to_string());
        assert_eq!(list[0], "file7");
        assert_eq!(list.iter().filter(|item| **item == "file7").count(), 1);
    }
}
