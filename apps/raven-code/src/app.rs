//! Central application state and the per-frame orchestration.
//!
//! `RavenApp` owns documents, panes, workspace, settings, jobs, the LSP
//! bridge and UI overlays; `update` draws panels in edge-to-center order
//! (menu → explorer → bottom → status → editor) and then overlays. All
//! background work streams through channels that are drained at the top of
//! every frame.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use eframe::egui;

use raven_editor::buffer::{DocError, Document, LoadMode, LoadPolicy};
use raven_editor::language::{self, LanguageDef};
use raven_editor::settings::Settings;
use raven_editor::theme;

use crate::actions::Action;
use crate::code_editor::JobCache;
use crate::config_paths;
use crate::file_previews::{self, HexPreview, ImagePreview};
use crate::highlight::{DocHighlighter, HighlightEngine};
use crate::jobs::{JobManager, JobStatus};
use crate::lsp_bridge::LspBridge;
use crate::panels;
use crate::workspace::{FileIndex, Workspace};

/// One open buffer.
pub struct DocumentState {
    pub id: u64,
    pub doc: Document,
    pub modified: bool,
    pub language: &'static LanguageDef,
    /// Bumped on every text change (drives LSP sync and caches).
    pub rev: u64,
    pub lsp_version: i64,
    pub lsp_open: bool,
    /// Syntax set lazily; recomputed on language change.
    pub syntax_name: String,
    pub last_edit_at: std::time::Instant,
    pub last_saved_at: Option<std::time::Instant>,
    pub last_save_was_auto: bool,
}

impl DocumentState {
    pub fn display_name(&self) -> String {
        self.doc
            .path()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("Untitled {}", self.id))
    }

    pub fn is_read_only(&self) -> bool {
        self.doc.mode == LoadMode::ReadOnly
    }

    pub fn highlighting_enabled(&self, settings: &Settings) -> bool {
        settings.editor.highlighting
            && self.doc.mode == LoadMode::Full
            && self.syntax_name != "Plain Text"
    }
}

/// One editor pane (the app has one or two when split).
#[derive(Default)]
pub struct PaneState {
    pub tabs: Vec<u64>,
    pub active: Option<u64>,
    /// Last reported cursor position (status bar).
    pub cursor: (usize, usize),
    pub selection_chars: usize,
}

/// Modal dialogs.
pub enum Dialog {
    /// Unsaved changes guard before some pending action.
    UnsavedConfirm {
        doc_ids: Vec<u64>,
        pending: PendingClose,
        error: Option<String>,
    },
    ConfirmDelete {
        path: PathBuf,
        is_dir: bool,
    },
    /// Generic text input (new file/folder, rename, go to line, run command).
    Input {
        kind: InputKind,
        title: String,
        value: String,
    },
    About,
    Shortcuts,
    /// Manual path entry (native file dialog unavailable).
    OpenPath {
        kind: config_paths::PickKind,
        value: String,
    },
}

#[derive(Clone, PartialEq)]
pub enum InputKind {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    GoToLine,
    RunCommand,
}

/// What to do after unsaved changes are confirmed.
pub enum PendingClose {
    CloseTab(usize, u64),
    CloseAll,
    Quit,
    CloseFolder,
}

#[derive(Clone, PartialEq)]
pub enum PaletteMode {
    Commands,
    QuickOpen,
}

pub struct PaletteState {
    pub open: bool,
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
}

impl Default for PaletteState {
    fn default() -> Self {
        PaletteState {
            open: false,
            mode: PaletteMode::Commands,
            query: String::new(),
            selected: 0,
        }
    }
}

/// Find & replace state for the active editor.
#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub show_replace: bool,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    pub match_count: usize,
    pub message: Option<String>,
    /// One-shot: focus the editor input when the find bar opened.
    pub focus_editor: bool,
}

/// "Find in files" background search.
pub struct FileSearch {
    pub query: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub running: bool,
    pub receiver: Option<Receiver<Vec<panels::bottom_panel::SearchHit>>>,
    pub results: Vec<panels::bottom_panel::SearchHit>,
    pub error: Option<String>,
}

/// Transient status-bar message.
pub struct StatusNotice {
    pub text: String,
    pub is_error: bool,
    pub at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub level: ToastLevel,
    pub created: std::time::Instant,
    pub persistent: bool,
    pub repeats: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Markdown,
    Json,
    Delimited,
}

const MAX_TEXT_PREVIEW_BYTES: usize = 1024 * 1024;

fn is_workspace_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rvenc"))
}

fn resolve_workspace_root(path: &Path, contents: &str) -> Result<PathBuf, String> {
    let parsed: serde_json::Value = serde_json::from_str(contents)
        .map_err(|error| format!("Invalid .rvenc workspace file: {error}"))?;
    if parsed.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("Unsupported .rvenc workspace version.".to_string());
    }
    let root = parsed
        .get("root")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "Workspace file is missing its root folder.".to_string())?;
    let root = PathBuf::from(root);
    let resolved = if root.is_absolute() {
        root
    } else {
        path.parent().unwrap_or_else(|| Path::new(".")).join(root)
    };
    if !resolved.is_dir() {
        return Err(format!(
            "Workspace folder does not exist: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn encode_workspace_file(root: &Path, target: &Path) -> Result<String, String> {
    let root_value = target
        .parent()
        .and_then(|parent| root.strip_prefix(parent).ok())
        .map(|relative| relative.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let contents = serde_json::json!({"version": 1, "root": root_value});
    serde_json::to_string_pretty(&contents)
        .map(|serialized| format!("{serialized}\n"))
        .map_err(|error| error.to_string())
}

pub struct RavenApp {
    pub settings: Settings,
    pub settings_dir: PathBuf,
    pub settings_dirty: bool,
    pub last_settings_save: std::time::Instant,

    pub theme_id: &'static theme::Theme,
    pub engine: HighlightEngine,

    pub docs: HashMap<u64, DocumentState>,
    pub preview_modes: HashSet<u64>,
    pub image_previews: HashMap<u64, ImagePreview>,
    pub hex_previews: HashMap<u64, HexPreview>,
    pub next_doc_id: u64,
    pub highlighters: HashMap<u64, DocHighlighter>,
    pub job_caches: HashMap<u64, Option<JobCache>>,

    pub panes: [PaneState; 2],
    pub split: bool,
    pub active_pane: usize,

    pub workspace: Option<Workspace>,
    /// Optional `.rvenc` file used to reopen this workspace later.
    pub workspace_file: Option<PathBuf>,
    pub file_index: FileIndex,
    pub project_info: Option<(String, PathBuf)>,
    pub project_detect_receiver: Option<Receiver<Option<(String, PathBuf)>>>,

    pub jobs: JobManager,
    notified_jobs: HashSet<u64>,
    pub lsp: LspBridge,

    pub palette: PaletteState,
    pub dialog: Option<Dialog>,
    pub find: FindState,
    pub file_search: FileSearch,

    pub notice: Option<StatusNotice>,
    pub notifications: VecDeque<Toast>,
    pub notification_history: VecDeque<Toast>,
    pub notifications_open: bool,
    next_notification_id: u64,
    pub last_notification_frame: std::time::Instant,
    pub bottom_tab: usize,
    /// Native close is cancelled until the user resolves the unsaved guard.
    close_when_ready: bool,

    /// Completion popup state (LSP).
    pub completion_open: bool,
    pub completion_selected: usize,
    /// Hover window visible until this instant (LSP).
    pub hover_until: Option<std::time::Instant>,
}

impl eframe::App for RavenApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Runs before each UI pass (and while hidden): drain background
        // work, apply theme, handle global shortcuts. No painting here.
        self.pump_background(ctx);
        self.autosave_due();
        if ctx.input(|input| input.viewport().close_requested()) {
            let modified = self.modified_doc_ids();
            if modified.is_empty() {
                self.close_when_ready = false;
            } else {
                let already_confirming_quit = matches!(
                    &self.dialog,
                    Some(Dialog::UnsavedConfirm {
                        pending: PendingClose::Quit,
                        ..
                    })
                );
                if !already_confirming_quit {
                    self.dialog = Some(Dialog::UnsavedConfirm {
                        doc_ids: modified,
                        pending: PendingClose::Quit,
                        error: None,
                    });
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        if std::mem::take(&mut self.close_when_ready) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        panels::apply_theme(ctx, self.theme_id, self.settings.appearance.reduce_motion);

        let palette_open = self.palette.open;
        if !palette_open {
            self.handle_shortcuts(ctx);
        } else {
            panels::palette::handle_palette_keys(self, ctx);
        }

        // Completion popup intercepts navigation keys.
        if self.completion_open {
            self.handle_completion_keys(ctx);
        }

        self.maybe_save_settings(ctx);
        self.update_window_title(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // 1. Panel layout, edge to center.
        panels::menu_bar::show(self, ui);
        panels::activity_bar::show(self, ui);
        if self.settings.workspace.sidebar_visible {
            panels::explorer::show(self, ui);
        }
        panels::bottom_panel::show(self, ui);
        panels::status_bar::show(self, ui);
        panels::editor_area::show(self, ui, &ctx);

        // 2. Overlays (context-based windows).
        panels::palette::show(self, &ctx);
        panels::dialogs::show(self, &ctx);
        panels::notifications::show(self, &ctx);

        // 3. Window drop targets.
        self.handle_dropped_files(&ctx);

        // Keep repainting only when something is running (no busy polling).
        let needs_repaint = self.jobs.any_running()
            || self
                .image_previews
                .values()
                .any(|preview| preview.receiver.is_some())
            || self
                .hex_previews
                .values()
                .any(|preview| preview.receiver.is_some())
            || self
                .hex_previews
                .values()
                .any(|preview| preview.receiver.is_some())
            || self.file_search.running
            || (self.settings.editor.autosave_delay_ms > 0
                && self
                    .docs
                    .values()
                    .any(|doc| doc.modified && doc.doc.path().is_some()))
            || self.file_index.status() == crate::workspace::IndexStatus::Scanning
            || self
                .notice
                .as_ref()
                .is_some_and(|notice| notice.at.elapsed() < std::time::Duration::from_secs(4));
        let notifications_need_repaint =
            self.notifications.iter().any(|toast| !toast.persistent) || self.notifications_open;
        if needs_repaint || notifications_need_repaint {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Save settings and shut down language servers cleanly.
        let _ = self.settings.save_to(&self.settings_dir);
        self.lsp.shutdown();
    }
}

impl RavenApp {
    pub fn new(
        settings: Settings,
        settings_dir: PathBuf,
        startup_folder: Option<PathBuf>,
        startup_files: Vec<PathBuf>,
        startup_notice: Option<String>,
    ) -> RavenApp {
        let theme_id = theme::resolve(&settings.appearance.theme);
        let lsp = LspBridge::new(&settings);
        let syntax_folder = settings_dir.join("syntaxes");
        let engine = HighlightEngine::with_custom_syntaxes(Some(&syntax_folder));
        let startup_notice = startup_notice.or_else(|| {
            engine
                .issues
                .first()
                .map(|issue| format!("Could not load a custom syntax: {issue}"))
        });

        let mut app = RavenApp {
            settings,
            settings_dir,
            settings_dirty: false,
            last_settings_save: std::time::Instant::now(),
            theme_id,
            engine,
            docs: HashMap::new(),
            preview_modes: HashSet::new(),
            image_previews: HashMap::new(),
            hex_previews: HashMap::new(),
            next_doc_id: 1,
            highlighters: HashMap::new(),
            job_caches: HashMap::new(),
            panes: [PaneState::default(), PaneState::default()],
            split: false,
            active_pane: 0,
            workspace: None,
            workspace_file: None,
            file_index: FileIndex::default(),
            project_info: None,
            project_detect_receiver: None,
            jobs: JobManager::new(),
            notified_jobs: HashSet::new(),
            lsp,
            palette: PaletteState::default(),
            dialog: None,
            find: FindState::default(),
            file_search: FileSearch {
                query: String::new(),
                case_sensitive: false,
                regex: false,
                running: false,
                receiver: None,
                results: Vec::new(),
                error: None,
            },
            notice: startup_notice.map(|text| StatusNotice {
                text,
                is_error: false,
                at: std::time::Instant::now(),
            }),
            notifications: VecDeque::new(),
            notification_history: VecDeque::new(),
            notifications_open: false,
            next_notification_id: 1,
            last_notification_frame: std::time::Instant::now(),
            bottom_tab: 0,
            close_when_ready: false,
            completion_open: false,
            completion_selected: 0,
            hover_until: None,
        };

        if let Some(folder) = startup_folder {
            app.open_folder(&folder);
        }
        for file in startup_files {
            if file.is_file() {
                app.open_file(&file);
            } else if file.is_dir() && app.workspace.is_none() {
                app.open_folder(&file);
            }
        }
        app
    }

    // ------------------------------------------------------------ helpers ----

    pub fn colors(&self) -> &'static theme::ThemeColors {
        &self.theme_id.colors
    }

    pub fn policy(&self) -> LoadPolicy {
        LoadPolicy {
            highlight_off_bytes: self.settings.editor.highlight_off_above_bytes,
            readonly_bytes: self.settings.editor.readonly_above_bytes,
        }
    }

    pub fn set_notice(&mut self, text: impl Into<String>, is_error: bool) {
        let text = text.into();
        self.notice = Some(StatusNotice {
            text: text.clone(),
            is_error,
            at: std::time::Instant::now(),
        });
        let is_save = text.starts_with("saved ");
        let is_action_success = text.starts_with("created ")
            || text.starts_with("deleted ")
            || text.starts_with("renamed to ");
        if is_error || is_save || is_action_success {
            if is_save && !self.settings.notifications.save_success {
                return;
            }
            self.push_notification(
                text,
                if is_error {
                    ToastLevel::Error
                } else if is_save || is_action_success {
                    ToastLevel::Success
                } else {
                    ToastLevel::Info
                },
                is_error,
            );
        }
    }

    pub fn push_notification(&mut self, text: String, level: ToastLevel, persistent: bool) {
        if !self.settings.notifications.enabled {
            return;
        }
        let now = std::time::Instant::now();
        if let Some(existing) = self.notification_history.back_mut().filter(|item| {
            item.text == text && item.level == level && item.created.elapsed().as_secs_f32() < 3.0
        }) {
            existing.repeats += 1;
            existing.created = now;
            let id = existing.id;
            if let Some(active) = self.notifications.iter_mut().find(|item| item.id == id) {
                active.repeats = existing.repeats;
                active.created = now;
            } else {
                self.notifications.push_back(existing.clone());
            }
            return;
        }
        let item = Toast {
            id: self.next_notification_id,
            text,
            level,
            created: now,
            persistent,
            repeats: 1,
        };
        self.next_notification_id = self.next_notification_id.wrapping_add(1).max(1);
        self.notification_history.push_back(item.clone());
        while self.notification_history.len() > 100 {
            self.notification_history.pop_front();
        }
        self.notifications.push_back(item);
        while self.notifications.len() > 16 {
            self.notifications.pop_front();
        }
    }

    pub fn toggle_file_preview(&mut self) {
        let Some(id) = self.active_doc_id() else {
            self.set_notice("Open a Markdown, JSON, CSV, or TSV file first.", true);
            return;
        };
        let Some(path) = self
            .docs
            .get(&id)
            .and_then(|doc| doc.doc.path())
            .map(Path::to_path_buf)
        else {
            self.set_notice(
                "Preview is available for saved Markdown, JSON, CSV, and TSV files.",
                true,
            );
            return;
        };
        if preview_kind(&path).is_none() {
            self.set_notice("This file type has no built-in preview.", true);
            return;
        }
        let size = self
            .docs
            .get(&id)
            .map(|doc| doc.doc.text().len())
            .unwrap_or_default();
        if size > MAX_TEXT_PREVIEW_BYTES {
            self.set_notice(
                "Text previews are limited to 1 MiB to keep the UI responsive.",
                true,
            );
            return;
        }
        if !self.preview_modes.remove(&id) {
            self.preview_modes.insert(id);
        }
    }

    pub fn format_json_active(&mut self) {
        let Some(id) = self.active_doc_id() else {
            self.set_notice("Open a JSON file first.", true);
            return;
        };
        let Some((path, source)) = self
            .docs
            .get(&id)
            .and_then(|doc| Some((doc.doc.path()?.to_path_buf(), doc.doc.text().to_string())))
        else {
            self.set_notice("Save the JSON file before formatting it.", true);
            return;
        };
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        {
            self.set_notice("Format JSON works on .json files.", true);
            return;
        }
        let formatted = match serde_json::from_str::<serde_json::Value>(&source)
            .and_then(|value| serde_json::to_string_pretty(&value))
        {
            Ok(formatted) => formatted,
            Err(error) => {
                self.set_notice(format!("JSON is invalid: {error}"), true);
                return;
            }
        };
        if formatted == source {
            self.set_notice("JSON is already formatted.", false);
            return;
        }
        if let Some(state) = self.docs.get_mut(&id) {
            state.doc.set_text(&formatted);
            state.modified = true;
            state.rev = state.rev.saturating_add(1);
            state.last_edit_at = std::time::Instant::now();
        }
        if let Some(highlighter) = self.highlighters.get_mut(&id) {
            highlighter.reset(formatted.split('\n').count(), true);
        }
        self.job_caches.insert(id, None);
        self.set_notice(format!("formatted {}", path.display()), false);
    }

    pub fn active_pane(&self) -> &PaneState {
        &self.panes[self.active_pane]
    }

    pub fn active_pane_mut(&mut self) -> &mut PaneState {
        &mut self.panes[self.active_pane]
    }

    pub fn active_doc_id(&self) -> Option<u64> {
        self.active_pane().active
    }

    pub fn active_doc(&self) -> Option<&DocumentState> {
        let id = self.active_doc_id()?;
        self.docs.get(&id)
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut DocumentState> {
        let id = self.active_doc_id()?;
        self.docs.get_mut(&id)
    }

    pub fn modified_doc_ids(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .docs
            .iter()
            .filter(|(_, doc)| doc.modified)
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    // ----------------------------------------------------------- opening ----

    /// Opens a file (or focuses it when already open). Returns doc id.
    pub fn open_file(&mut self, path: &Path) -> Option<u64> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        if is_workspace_file(&canonical) {
            self.open_workspace_file(&canonical);
            return None;
        }

        // Already open? Focus it.
        for (id, doc) in &self.docs {
            if doc.doc.path() == Some(canonical.as_path()) {
                let id = *id;
                self.focus_doc(id);
                return Some(id);
            }
        }

        if file_previews::is_image_path(&canonical) {
            return self.open_image_preview(&canonical);
        }

        if file_previews::is_known_binary_path(&canonical) {
            return self.open_hex_preview(&canonical);
        }

        match Document::load_with_policy(&canonical, &self.policy()) {
            Ok(doc) => {
                let language = language::for_path(&canonical);
                let id = self.next_doc_id;
                self.next_doc_id += 1;

                let highlighting = doc.mode == LoadMode::Full
                    && self.settings.editor.highlighting
                    && language.id != "plaintext";
                let mut highlighter = DocHighlighter::new();
                highlighter.reset(doc.text().split('\n').count(), highlighting);
                self.highlighters.insert(id, highlighter);
                self.job_caches.insert(id, None);

                let mut state = DocumentState {
                    id,
                    doc,
                    modified: false,
                    language,
                    rev: 1,
                    lsp_version: 1,
                    lsp_open: false,
                    syntax_name: String::new(),
                    last_edit_at: std::time::Instant::now(),
                    last_saved_at: None,
                    last_save_was_auto: false,
                };
                state.syntax_name = self
                    .engine
                    .syntax_for_path(language, &canonical)
                    .name
                    .clone();

                // LSP: start/configure lazily; opening never installs.
                let text = state.doc.text().to_string();
                self.lsp.document_opened(language, &canonical, &text);
                state.lsp_open = true;

                self.docs.insert(id, state);
                self.push_recent_file(&canonical);
                self.focus_doc(id);
                self.request_symbols(id);
                Some(id)
            }
            Err(error @ DocError::UnsupportedEncoding { .. }) => {
                self.open_hex_preview(&canonical).or_else(|| {
                    self.set_notice(error.to_string(), true);
                    None
                })
            }
            Err(error) => {
                self.set_notice(error.to_string(), true);
                None
            }
        }
    }

    fn open_hex_preview(&mut self, path: &Path) -> Option<u64> {
        let preview = match HexPreview::start(path) {
            Ok(preview) => preview,
            Err(error) => {
                self.set_notice(format!("Could not open binary file: {error}"), true);
                return None;
            }
        };
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        let mut doc = Document::new_empty();
        doc.set_path(Some(path.to_path_buf()));
        doc.mode = LoadMode::ReadOnly;
        self.docs.insert(
            id,
            DocumentState {
                id,
                doc,
                modified: false,
                language: &language::PLAIN_TEXT,
                rev: 1,
                lsp_version: 1,
                lsp_open: false,
                syntax_name: "Hex Viewer".to_string(),
                last_edit_at: std::time::Instant::now(),
                last_saved_at: None,
                last_save_was_auto: false,
            },
        );
        self.hex_previews.insert(id, preview);
        self.push_recent_file(path);
        self.focus_doc(id);
        Some(id)
    }

    fn open_image_preview(&mut self, path: &Path) -> Option<u64> {
        let preview = match ImagePreview::start(path) {
            Ok(preview) => preview,
            Err(error) => {
                self.set_notice(
                    format!("Could not preview {}: {error}", path.display()),
                    true,
                );
                return None;
            }
        };
        let id = self.next_doc_id;
        self.next_doc_id += 1;
        let mut doc = Document::new_empty();
        doc.set_path(Some(path.to_path_buf()));
        doc.mode = LoadMode::ReadOnly;
        self.docs.insert(
            id,
            DocumentState {
                id,
                doc,
                modified: false,
                language: &language::PLAIN_TEXT,
                rev: 1,
                lsp_version: 1,
                lsp_open: false,
                syntax_name: "Image Preview".to_string(),
                last_edit_at: std::time::Instant::now(),
                last_saved_at: None,
                last_save_was_auto: false,
            },
        );
        self.image_previews.insert(id, preview);
        self.push_recent_file(path);
        self.focus_doc(id);
        Some(id)
    }

    pub fn open_folder(&mut self, path: &Path) {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !canonical.is_dir() {
            self.set_notice(format!("not a folder: {}", canonical.display()), true);
            return;
        }
        self.workspace = Some(Workspace::open(canonical.clone()));
        self.workspace_file = None;
        self.file_index.scan(canonical.clone());
        self.push_recent_folder(&canonical);

        // Project detection in the background (raven-projects SDK).
        let (sender, receiver) = std::sync::mpsc::channel();
        self.project_detect_receiver = Some(receiver);
        std::thread::Builder::new()
            .name("raven-code-detect".into())
            .spawn(move || {
                let detection =
                    raven_projects::detect(&canonical, &raven_projects::DetectOptions::default())
                        .ok()
                        .and_then(|detection| {
                            raven_projects::select(&detection, None)
                                .ok()
                                .map(|(kind, root)| (kind.display_name().to_string(), root))
                        });
                let _ = sender.send(detection);
            })
            .ok();
    }

    pub fn close_folder(&mut self) {
        self.workspace = None;
        self.workspace_file = None;
        self.project_info = None;
        self.file_search.results.clear();
    }

    fn focus_doc(&mut self, id: u64) {
        let pane = self.active_pane_mut();
        if !pane.tabs.contains(&id) {
            pane.tabs.push(id);
        }
        pane.active = Some(id);
    }

    pub fn open_in_pane(&mut self, pane_index: usize, id: u64) {
        let pane = &mut self.panes[pane_index];
        if !pane.tabs.contains(&id) {
            pane.tabs.push(id);
        }
        pane.active = Some(id);
        self.active_pane = pane_index;
    }

    pub fn push_recent_file(&mut self, path: &Path) {
        Settings::push_recent(
            &mut self.settings.workspace.recent_files,
            path.to_string_lossy().into_owned(),
        );
        self.settings_dirty = true;
    }

    pub fn push_recent_folder(&mut self, path: &Path) {
        Settings::push_recent(
            &mut self.settings.workspace.recent_folders,
            path.to_string_lossy().into_owned(),
        );
        self.settings_dirty = true;
    }

    // ------------------------------------------------------------ saving ----

    pub fn save_doc(&mut self, id: u64) -> bool {
        self.save_doc_with_kind(id, false)
    }

    fn save_doc_with_kind(&mut self, id: u64, automatic: bool) -> bool {
        if self.hex_previews.contains_key(&id) {
            return self.save_hex_to(id, None);
        }
        if self.docs.get(&id).is_some_and(DocumentState::is_read_only) {
            self.set_notice("This preview is read-only.", true);
            return false;
        }
        let has_path = self
            .docs
            .get(&id)
            .is_some_and(|state| state.doc.path().is_some());
        if !has_path {
            return self.save_doc_as_native(id);
        }
        let Some(state) = self.docs.get_mut(&id) else {
            return false;
        };
        let result = state.doc.save();
        match result {
            Ok(()) => {
                state.modified = false;
                state.last_saved_at = Some(std::time::Instant::now());
                state.last_save_was_auto = automatic;
                let language = state.language;
                let path = state.doc.path().unwrap().to_path_buf();
                let text = state.doc.text().to_string();
                self.lsp.document_saved(language, &path, &text);
                if !automatic {
                    self.set_notice(format!("saved {}", path.display()), false);
                }
                true
            }
            Err(error) => {
                self.set_notice(error.to_string(), true);
                false
            }
        }
    }

    fn save_doc_as_native(&mut self, id: u64) -> bool {
        let (start, suggested) = match self.docs.get(&id) {
            Some(state) => (
                state
                    .doc
                    .path()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf),
                state.display_name(),
            ),
            None => return false,
        };
        let Some(target) = config_paths::native_save_file(start.as_deref(), &suggested, None)
        else {
            return false;
        };
        self.save_doc_as(id, &target)
    }

    pub fn open_workspace_file(&mut self, path: &Path) {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) => {
                self.set_notice(format!("Could not read workspace file: {error}"), true);
                return;
            }
        };
        let root = match resolve_workspace_root(path, &contents) {
            Ok(root) => root,
            Err(error) => {
                self.set_notice(error, true);
                return;
            }
        };
        self.open_folder(&root);
        self.workspace_file = Some(path.to_path_buf());
        self.push_recent_file(path);
        self.set_notice(format!("Opened workspace {}", path.display()), false);
    }

    pub fn save_workspace_file_as(&mut self) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.clone())
        else {
            self.set_notice("Open a folder before saving a workspace file.", true);
            return;
        };
        let suggested = format!(
            "{}.rvenc",
            root.file_name().unwrap_or_default().to_string_lossy()
        );
        let Some(target) = config_paths::native_save_file(
            Some(&root),
            &suggested,
            Some(("Raven Workspace", &["rvenc"])),
        ) else {
            return;
        };
        if !is_workspace_file(&target) {
            self.set_notice("Workspace files must use the .rvenc extension.", true);
            return;
        }
        self.write_workspace_file(&root, &target);
    }

    fn write_workspace_file(&mut self, root: &Path, target: &Path) {
        let contents = match encode_workspace_file(root, target) {
            Ok(serialized) => serialized,
            Err(error) => {
                self.set_notice(format!("Could not encode workspace file: {error}"), true);
                return;
            }
        };
        match std::fs::write(target, contents) {
            Ok(()) => {
                self.workspace_file = Some(target.to_path_buf());
                self.push_recent_file(target);
                self.set_notice(format!("Saved workspace {}", target.display()), false);
            }
            Err(error) => self.set_notice(format!("Could not save workspace file: {error}"), true),
        }
    }

    pub fn save_workspace_file(&mut self) {
        let (Some(root), Some(path)) = (
            self.workspace
                .as_ref()
                .map(|workspace| workspace.root.clone()),
            self.workspace_file.clone(),
        ) else {
            self.save_workspace_file_as();
            return;
        };
        self.write_workspace_file(&root, &path);
    }

    pub fn save_doc_as(&mut self, id: u64, target: &Path) -> bool {
        if self.hex_previews.contains_key(&id) {
            return self.save_hex_to(id, Some(target));
        }
        if self.docs.get(&id).is_some_and(DocumentState::is_read_only) {
            self.set_notice("This preview is read-only.", true);
            return false;
        }
        let Some(state) = self.docs.get_mut(&id) else {
            return false;
        };
        match state.doc.save_to(target) {
            Ok(()) => {
                state.modified = false;
                state.last_saved_at = Some(std::time::Instant::now());
                state.last_save_was_auto = false;
                let language = language::for_path(target);
                state.language = language;
                state.syntax_name = self.engine.syntax_for_path(language, target).name.clone();
                let highlighting = state.doc.mode == LoadMode::Full
                    && self.settings.editor.highlighting
                    && language.id != "plaintext";
                self.highlighters.entry(id).and_modify(|doc| {
                    doc.reset(state.doc.text().split('\n').count(), highlighting);
                });
                let text = state.doc.text().to_string();
                self.lsp.document_saved(language, target, &text);
                self.push_recent_file(target);
                self.set_notice(format!("saved {}", target.display()), false);
                true
            }
            Err(error) => {
                self.set_notice(error.to_string(), true);
                false
            }
        }
    }

    fn save_hex_to(&mut self, id: u64, target: Option<&Path>) -> bool {
        let Some(preview) = self.hex_previews.get(&id) else {
            return false;
        };
        let Some(bytes) = preview.bytes.clone() else {
            self.set_notice("Binary data is still loading.", true);
            return false;
        };
        let original_path = preview.path.clone();
        let original_size = preview.file_size;
        let original_modified = preview.original_modified;
        let is_modified = preview.modified;
        let path = target.unwrap_or(&original_path).to_path_buf();
        if target.is_none() && !is_modified {
            return true;
        }
        if bytes.len() as u64 != original_size && target.is_some() {
            self.set_notice("Save As is unavailable for a partial binary preview.", true);
            return false;
        }
        if target.is_none() && bytes.len() as u64 != original_size {
            self.set_notice(
                "The binary preview is incomplete; it cannot be saved safely.",
                true,
            );
            return false;
        }
        if path == original_path {
            match std::fs::metadata(&path) {
                Ok(metadata)
                    if metadata.len() == original_size
                        && metadata.modified().ok() == original_modified => {}
                Ok(_) => {
                    self.set_notice(
                        format!(
                            "{} changed on disk since it was opened; reload before saving",
                            path.display()
                        ),
                        true,
                    );
                    return false;
                }
                Err(error) => {
                    self.set_notice(format!("Could not check {}: {error}", path.display()), true);
                    return false;
                }
            }
        }
        match Document::write_bytes_to(&path, &bytes) {
            Ok(()) => {
                let new_meta = std::fs::metadata(&path).ok();
                if let Some(preview) = self.hex_previews.get_mut(&id) {
                    preview.path = path.clone();
                    preview.file_size = new_meta
                        .as_ref()
                        .map_or(bytes.len() as u64, |meta| meta.len());
                    preview.original_modified = new_meta.and_then(|meta| meta.modified().ok());
                    preview.modified = false;
                }
                if let Some(state) = self.docs.get_mut(&id) {
                    state.doc.set_path(Some(path.clone()));
                    state.modified = false;
                    state.last_saved_at = Some(std::time::Instant::now());
                    state.last_save_was_auto = false;
                }
                self.set_notice(format!("saved {}", path.display()), false);
                true
            }
            Err(error) => {
                self.set_notice(error.to_string(), true);
                false
            }
        }
    }

    pub fn save_all(&mut self) {
        let ids: Vec<u64> = self.docs.keys().copied().collect();
        for id in ids {
            if self.docs.get(&id).is_some_and(|doc| doc.modified) {
                self.save_doc(id);
            }
        }
    }

    fn autosave_due(&mut self) {
        let delay_ms = self.settings.editor.autosave_delay_ms;
        if delay_ms == 0 {
            return;
        }
        let delay = std::time::Duration::from_millis(delay_ms);
        let now = std::time::Instant::now();
        let due: Vec<u64> = self
            .docs
            .values_mut()
            .filter(|doc| {
                doc.modified
                    && doc.doc.path().is_some()
                    && now.duration_since(doc.last_edit_at) >= delay
            })
            .map(|doc| {
                // Back off after a failed save so an unwritable file does not
                // get hammered every frame; the dirty indicator stays visible.
                doc.last_edit_at = now;
                doc.id
            })
            .collect();
        for id in due {
            let _ = self.save_doc_with_kind(id, true);
        }
    }

    // ----------------------------------------------------------- closing ----

    /// Initiates closing a tab with the unsaved-changes guard.
    pub fn request_close_tab(&mut self, pane_index: usize, doc_id: u64) {
        let modified = self.docs.get(&doc_id).is_some_and(|doc| doc.modified);
        if modified {
            self.dialog = Some(Dialog::UnsavedConfirm {
                doc_ids: vec![doc_id],
                pending: PendingClose::CloseTab(pane_index, doc_id),
                error: None,
            });
        } else {
            self.do_close_tab(pane_index, doc_id);
        }
    }

    pub fn do_close_tab(&mut self, pane_index: usize, doc_id: u64) {
        let language_path = self.docs.get(&doc_id).and_then(|doc| {
            doc.doc
                .path()
                .map(|path| (doc.language, path.to_path_buf()))
        });
        if let Some((language, path)) = language_path {
            self.lsp.document_closed(language, &path);
            self.lsp.diagnostics.remove(&path);
        }

        if let Some(pane) = self.panes.get_mut(pane_index) {
            pane.tabs.retain(|tab| *tab != doc_id);
            if pane.active == Some(doc_id) {
                pane.active = pane.tabs.last().copied();
            }
        }
        // Is the doc still visible in the other pane?
        let still_visible = self.panes.iter().any(|pane| pane.tabs.contains(&doc_id));
        if !still_visible {
            self.docs.remove(&doc_id);
            self.preview_modes.remove(&doc_id);
            self.image_previews.remove(&doc_id);
            self.hex_previews.remove(&doc_id);
            self.hex_previews.remove(&doc_id);
            self.highlighters.remove(&doc_id);
            self.job_caches.remove(&doc_id);
        }
    }

    pub fn request_close_all(&mut self) {
        let modified = self.modified_doc_ids();
        if !modified.is_empty() {
            self.dialog = Some(Dialog::UnsavedConfirm {
                doc_ids: modified,
                pending: PendingClose::CloseAll,
                error: None,
            });
        } else {
            self.do_close_all();
        }
    }

    pub fn do_close_all(&mut self) {
        let ids: Vec<u64> = self.docs.keys().copied().collect();
        for id in ids {
            let language_path = self.docs.get(&id).and_then(|doc| {
                doc.doc
                    .path()
                    .map(|path| (doc.language, path.to_path_buf()))
            });
            if let Some((language, path)) = language_path {
                self.lsp.document_closed(language, &path);
            }
        }
        self.docs.clear();
        self.preview_modes.clear();
        self.image_previews.clear();
        self.hex_previews.clear();
        self.hex_previews.clear();
        self.highlighters.clear();
        self.job_caches.clear();
        for pane in &mut self.panes {
            pane.tabs.clear();
            pane.active = None;
        }
    }

    pub fn request_quit(&mut self) {
        let modified = self.modified_doc_ids();
        if !modified.is_empty() {
            self.dialog = Some(Dialog::UnsavedConfirm {
                doc_ids: modified,
                pending: PendingClose::Quit,
                error: None,
            });
        } else {
            self.close_when_ready = true;
        }
    }

    pub fn finish_pending_close(&mut self, pending: PendingClose) {
        match pending {
            PendingClose::CloseTab(pane, doc_id) => self.do_close_tab(pane, doc_id),
            PendingClose::CloseAll => self.do_close_all(),
            PendingClose::Quit => self.close_when_ready = true,
            PendingClose::CloseFolder => self.close_folder(),
        }
    }

    pub fn save_for_pending_close(&mut self, ids: &[u64], pending: PendingClose) {
        let mut failed = Vec::new();
        for id in ids.iter().copied() {
            if !self.docs.get(&id).is_some_and(|doc| doc.modified) {
                continue;
            }
            let Some(has_path) = self.docs.get(&id).map(|doc| doc.doc.path().is_some()) else {
                continue;
            };
            if !has_path {
                if self.save_doc_as_native(id) {
                    continue;
                }
                self.resume_pending_close(pending);
                return;
            }
            if !self.save_doc(id) {
                failed.push(id);
            }
        }
        if !failed.is_empty() {
            let names = failed
                .iter()
                .filter_map(|id| self.docs.get(id).map(DocumentState::display_name))
                .collect::<Vec<_>>();
            let detail = self
                .notice
                .as_ref()
                .map(|notice| notice.text.clone())
                .unwrap_or_else(|| "The document could not be saved.".to_string());
            self.dialog = Some(Dialog::UnsavedConfirm {
                doc_ids: failed,
                pending,
                error: Some(format!("{detail} ({})", names.join(", "))),
            });
            return;
        }
        self.finish_pending_close(pending);
    }

    pub fn resume_pending_close(&mut self, pending: PendingClose) {
        let remaining = self.modified_doc_ids();
        if remaining.is_empty() {
            self.finish_pending_close(pending);
        } else {
            self.dialog = Some(Dialog::UnsavedConfirm {
                doc_ids: remaining,
                pending,
                error: None,
            });
        }
    }

    // ------------------------------------------------------------ editing ----

    pub fn request_symbols(&mut self, doc_id: u64) {
        let Some(state) = self.docs.get(&doc_id) else {
            return;
        };
        let Some(path) = state.doc.path() else {
            return;
        };
        let language = state.language;
        if self.lsp.server_running(language) {
            self.lsp.request_symbols(language, path);
        }
    }

    pub fn sync_lsp_changes(&mut self) {
        // Called once per frame after panels ran: push full-text changes.
        let states: Vec<(u64, &'static LanguageDef, PathBuf, String, i64)> = self
            .docs
            .values()
            .filter(|doc| doc.lsp_open && doc.rev > doc.lsp_version.unsigned_abs())
            .map(|doc| {
                (
                    doc.id,
                    doc.language,
                    doc.doc.path().map(Path::to_path_buf).unwrap_or_default(),
                    doc.doc.text().to_string(),
                    doc.rev as i64,
                )
            })
            .collect();
        for (_id, language, path, text, version) in states {
            if path.as_os_str().is_empty() {
                continue;
            }
            self.lsp.document_changed(language, &path, &text, version);
            if let Some(doc) = self
                .docs
                .values_mut()
                .find(|doc| doc.doc.path() == Some(path.as_path()))
            {
                doc.lsp_version = version;
            }
        }
    }

    // --------------------------------------------------------- shortcuts ----

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let chord_for = |action: Action| action.chord(&self.settings);
        let mut to_run: Vec<Action> = Vec::new();
        for action in Action::all() {
            if let Some(chord) = chord_for(*action) {
                if ctx.input(|input| chord.pressed(input)) {
                    to_run.push(*action);
                }
            }
        }
        for action in to_run {
            self.run_action(action);
        }
    }

    /// Executes an action — the single dispatch point used by menus,
    /// shortcuts and the command palette.
    pub fn run_action(&mut self, action: Action) {
        use Action::*;
        match action {
            NewFile => {
                let doc = Document::new_empty();
                let id = self.next_doc_id;
                self.next_doc_id += 1;
                let highlighting = self.settings.editor.highlighting;
                let mut highlighter = DocHighlighter::new();
                highlighter.reset(1, highlighting);
                self.highlighters.insert(id, highlighter);
                self.job_caches.insert(id, None);
                self.docs.insert(
                    id,
                    DocumentState {
                        id,
                        doc,
                        modified: false,
                        language: &language::PLAIN_TEXT,
                        rev: 1,
                        lsp_version: 1,
                        lsp_open: false,
                        syntax_name: "Plain Text".to_string(),
                        last_edit_at: std::time::Instant::now(),
                        last_saved_at: None,
                        last_save_was_auto: false,
                    },
                );
                self.focus_doc(id);
            }
            OpenFile => {
                let start = self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone());
                match config_paths::native_pick(config_paths::PickKind::File, start.as_deref()) {
                    Some(path) => {
                        self.open_file(&path);
                    }
                    None => {
                        // Portal/native dialog may be unavailable; fall back.
                        self.dialog = Some(Dialog::OpenPath {
                            kind: config_paths::PickKind::File,
                            value: String::new(),
                        });
                    }
                }
            }
            OpenFolder => {
                let start = self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone());
                match config_paths::native_pick(config_paths::PickKind::Folder, start.as_deref()) {
                    Some(path) => self.open_folder(&path),
                    None => {
                        self.dialog = Some(Dialog::OpenPath {
                            kind: config_paths::PickKind::Folder,
                            value: String::new(),
                        });
                    }
                }
            }
            Save => {
                if let Some(id) = self.active_doc_id() {
                    self.save_doc(id);
                } else if self.workspace.is_some() {
                    self.save_workspace_file();
                }
            }
            SaveAs => {
                if let Some(id) = self.active_doc_id() {
                    let _ = self.save_doc_as_native(id);
                }
            }
            OpenWorkspace => {
                let start = self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone());
                if let Some(path) = config_paths::native_pick_filtered(
                    start.as_deref(),
                    "Raven Workspace",
                    &["rvenc"],
                ) {
                    self.open_workspace_file(&path);
                }
            }
            SaveWorkspaceAs => self.save_workspace_file_as(),
            SaveAll => self.save_all(),
            CloseTab => {
                let pane = self.active_pane;
                if let Some(id) = self.active_doc_id() {
                    self.request_close_tab(pane, id);
                }
            }
            CloseAllTabs => self.request_close_all(),
            CloseFolder => {
                if self.workspace.is_some() {
                    let modified = self.modified_doc_ids();
                    if modified.is_empty() {
                        self.close_folder();
                    } else {
                        self.dialog = Some(Dialog::UnsavedConfirm {
                            doc_ids: modified,
                            pending: PendingClose::CloseFolder,
                            error: None,
                        });
                    }
                }
            }
            Quit => self.request_quit(),
            Undo | Redo | Cut | Copy | Paste | SelectAll => {
                // Native widget behavior; action exists for palette docs.
                self.set_notice(
                    "use the editor's native shortcuts (Ctrl+Z / Ctrl+Y / Ctrl+X/C/V / Ctrl+A)",
                    false,
                );
            }
            Find => {
                self.find.open = true;
                self.find.show_replace = false;
                self.find.focus_editor = true;
            }
            Replace => {
                self.find.open = true;
                self.find.show_replace = true;
                self.find.focus_editor = true;
            }
            GoToLine => {
                self.dialog = Some(Dialog::Input {
                    kind: InputKind::GoToLine,
                    title: "Go to line".to_string(),
                    value: String::new(),
                });
            }
            CommandPalette => {
                self.palette.open = true;
                self.palette.mode = PaletteMode::Commands;
                self.palette.query.clear();
                self.palette.selected = 0;
            }
            QuickOpen => {
                self.palette.open = true;
                self.palette.mode = PaletteMode::QuickOpen;
                self.palette.query.clear();
                self.palette.selected = 0;
            }
            ToggleSidebar => {
                self.settings.workspace.sidebar_visible = !self.settings.workspace.sidebar_visible;
                self.settings_dirty = true;
            }
            ToggleBottomPanel => {
                self.settings.workspace.bottom_visible = !self.settings.workspace.bottom_visible;
                self.settings_dirty = true;
            }
            ToggleWordWrap => {
                self.settings.editor.word_wrap = !self.settings.editor.word_wrap;
                self.settings_dirty = true;
                // Reset caches: wrap width changes layout.
                for slot in self.job_caches.values_mut() {
                    *slot = None;
                }
            }
            ZoomIn => {
                self.settings.appearance.zoom = (self.settings.appearance.zoom + 0.1).min(3.0);
                self.settings_dirty = true;
                self.rebuild_caches();
            }
            ZoomOut => {
                self.settings.appearance.zoom = (self.settings.appearance.zoom - 0.1).max(0.4);
                self.settings_dirty = true;
                self.rebuild_caches();
            }
            ResetZoom => {
                self.settings.appearance.zoom = 1.0;
                self.settings_dirty = true;
                self.rebuild_caches();
            }
            SplitEditor => {
                self.split = !self.split;
                if self.split {
                    // Mirror the active tab into the second pane.
                    let active = self.active_doc_id();
                    if let Some(id) = active {
                        self.open_in_pane(1, id);
                        self.active_pane = 0;
                    }
                }
            }
            TogglePreview => self.toggle_file_preview(),
            FormatJson => self.format_json_active(),
            ThemeToggle => {
                let next = if self.theme_id.dark {
                    "raven-light"
                } else {
                    "raven-dark"
                };
                self.apply_theme_by_id(next);
            }
            RefreshExplorer => {
                if let Some(workspace) = &mut self.workspace {
                    workspace.refresh_all();
                }
                if let Some(root) = self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone())
                {
                    self.file_index.scan(root);
                }
            }
            NextTab => self.cycle_tab(1),
            PreviousTab => self.cycle_tab(-1),
            BuildProject => self.run_pico(raven_build::Op::Build),
            TestProject => self.run_pico(raven_build::Op::Test),
            RunProject => self.run_pico(raven_build::Op::Run),
            CancelJob => {
                self.jobs.cancel_all();
                self.set_notice("cancel requested for running jobs", false);
            }
            ClearOutput => self.jobs.buffer.clear(),
            Shortcuts => {
                self.dialog = Some(Dialog::Shortcuts);
            }
            About => {
                self.dialog = Some(Dialog::About);
            }
            CompletionTrigger => self.lsp_completion(),
            GotoDefinition => self.lsp_definition(),
            HoverInfo => self.lsp_hover(),
            FormatDocument => self.lsp_format(),
            RunCommand => {
                self.dialog = Some(Dialog::Input {
                    kind: InputKind::RunCommand,
                    title: "Run command (program + args, executed without a shell)".to_string(),
                    value: String::new(),
                });
            }
            OpenTerminal => {
                let dir = self
                    .workspace
                    .as_ref()
                    .map(|workspace| workspace.root.clone())
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                let custom = {
                    let trimmed = self.settings.editor.external_terminal.trim().to_string();
                    (!trimmed.is_empty()).then_some(trimmed)
                };
                match crate::config_paths::open_external_terminal(&dir, custom.as_deref()) {
                    Ok(()) => {
                        self.set_notice(format!("terminal opened at {}", dir.display()), false)
                    }
                    Err(error) => self.set_notice(error, true),
                }
            }
            OpenPath => {
                self.dialog = Some(Dialog::OpenPath {
                    kind: crate::config_paths::PickKind::File,
                    value: String::new(),
                });
            }
        }
    }

    // ------------------------------------------------- language intelligence ----

    fn lsp_completion(&mut self) {
        let Some(state) = self.active_doc() else {
            return;
        };
        let (line, col) = self.active_pane().cursor;
        let (language, path) = (state.language, state.doc.path().map(Path::to_path_buf));
        let Some(path) = path else {
            return;
        };
        if self.lsp.server_running(language) {
            self.lsp
                .request_completion(language, &path, line as u32, col as u32);
            self.completion_open = true;
            self.completion_selected = 0;
        } else if self.lsp.configured_for(language) {
            self.set_notice(
                format!("{} has not finished starting yet", language.name),
                false,
            );
        }
    }

    fn lsp_definition(&mut self) {
        let Some(state) = self.active_doc() else {
            return;
        };
        let (line, col) = self.active_pane().cursor;
        let (language, path) = (state.language, state.doc.path().map(Path::to_path_buf));
        let Some(path) = path else {
            return;
        };
        if self.lsp.server_running(language) {
            self.lsp
                .request_definition(language, &path, line as u32, col as u32);
        }
    }

    fn lsp_hover(&mut self) {
        let Some(state) = self.active_doc() else {
            return;
        };
        let (line, col) = self.active_pane().cursor;
        let (language, path) = (state.language, state.doc.path().map(Path::to_path_buf));
        let Some(path) = path else {
            return;
        };
        if self.lsp.server_running(language) {
            self.lsp
                .request_hover(language, &path, line as u32, col as u32);
            self.hover_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(8));
        }
    }

    fn lsp_format(&mut self) {
        let Some(state) = self.active_doc() else {
            return;
        };
        let (language, path) = (state.language, state.doc.path().map(Path::to_path_buf));
        let Some(path) = path else {
            return;
        };
        if self.lsp.server_running(language) {
            self.lsp.request_formatting(
                language,
                &path,
                self.settings.editor.tab_size as u32,
                self.settings.editor.insert_spaces,
            );
        } else {
            self.set_notice(
                "formatting needs a configured language server for this file type",
                false,
            );
        }
    }

    /// Inserts the chosen completion at the cursor.
    pub fn accept_completion(&mut self, item_index: usize) {
        let Some((uri, items)) = self.lsp.completions.clone() else {
            return;
        };
        let Some(item) = items.get(item_index) else {
            return;
        };
        let Some(doc_path) = self
            .active_doc()
            .and_then(|state| state.doc.path().map(Path::to_path_buf))
        else {
            return;
        };
        if raven_lsp::client::uri_to_path(&uri).as_ref() != Some(&doc_path) {
            return;
        }
        let Some(doc_id) = self.active_doc_id() else {
            return;
        };
        let text = {
            let state = self.docs.get(&doc_id).expect("checked above");
            state.doc.text().to_string()
        };
        let (line, col) = self.active_pane().cursor;
        let byte = byte_of_line_char(&text, line, col);
        let insert = item.insert.clone();
        let mut new_text = String::with_capacity(text.len() + insert.len());
        new_text.push_str(&text[..byte]);
        new_text.push_str(&insert);
        new_text.push_str(&text[byte..]);
        let state = self.docs.get_mut(&doc_id).expect("checked above");
        state.doc.set_text(&new_text);
        state.modified = true;
        state.rev += 1;
        if let Some(slot) = self.job_caches.get_mut(&doc_id) {
            *slot = None;
        }
        self.completion_open = false;
        self.settings_dirty = true;
    }

    pub fn apply_theme_by_id(&mut self, theme_id: &str) {
        self.theme_id = theme::resolve(theme_id);
        self.settings.appearance.theme = self.theme_id.id.to_string();
        self.settings_dirty = true;
        for slot in self.job_caches.values_mut() {
            *slot = None;
        }
    }

    pub fn rebuild_caches(&mut self) {
        for slot in self.job_caches.values_mut() {
            *slot = None;
        }
    }

    fn cycle_tab(&mut self, direction: i32) {
        let pane = &self.panes[self.active_pane];
        if pane.tabs.is_empty() {
            return;
        }
        let current = pane
            .active
            .and_then(|id| pane.tabs.iter().position(|tab| *tab == id))
            .unwrap_or(0);
        let count = pane.tabs.len() as i32;
        let next = (current as i32 + direction).rem_euclid(count) as usize;
        let id = pane.tabs[next];
        self.panes[self.active_pane].active = Some(id);
    }

    /// Navigation keys for the completion popup.
    fn handle_completion_keys(&mut self, ctx: &egui::Context) {
        let Some((_, items)) = &self.lsp.completions else {
            self.completion_open = false;
            return;
        };
        let count = items.len();
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let down = ctx.input(|input| input.key_pressed(egui::Key::ArrowDown));
        let up = ctx.input(|input| input.key_pressed(egui::Key::ArrowUp));
        let enter = ctx.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Enter)
        });
        if escape {
            self.completion_open = false;
            return;
        }
        if count == 0 {
            self.completion_open = false;
            return;
        }
        if down {
            self.completion_selected = (self.completion_selected + 1).min(count - 1);
        }
        if up {
            self.completion_selected = self.completion_selected.saturating_sub(1);
        }
        if enter {
            let selected = self.completion_selected;
            self.accept_completion(selected);
        }
    }

    pub fn run_pico(&mut self, op: raven_build::Op) {
        let Some(root) = self
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.clone())
        else {
            self.set_notice(
                "open a folder first — Run works on the project detected in it",
                true,
            );
            return;
        };
        self.settings.workspace.bottom_visible = true;
        self.settings_dirty = true;
        self.bottom_tab = 0;
        self.jobs.start_pico_operation(root, op, false, Vec::new());
    }

    // ------------------------------------------------------------ pumping ----

    fn pump_background(&mut self, ctx: &egui::Context) {
        self.jobs.drain_messages();
        if self.settings.notifications.build_results {
            let (_, jobs) = self.jobs.buffer.snapshot();
            for job in jobs {
                if job.status == JobStatus::Running || !self.notified_jobs.insert(job.id) {
                    continue;
                }
                let (text, level) = match job.status {
                    JobStatus::Succeeded => (
                        format!("{} completed successfully", job.title),
                        ToastLevel::Success,
                    ),
                    JobStatus::Failed => (
                        format!(
                            "{} failed{}",
                            job.title,
                            job.exit_code
                                .map(|code| format!(" (exit code {code})"))
                                .unwrap_or_default()
                        ),
                        ToastLevel::Error,
                    ),
                    JobStatus::Cancelled => {
                        (format!("{} was cancelled", job.title), ToastLevel::Warning)
                    }
                    JobStatus::LaunchFailed => (
                        format!("{} could not be started", job.title),
                        ToastLevel::Error,
                    ),
                    JobStatus::Running => continue,
                };
                self.push_notification(text, level, matches!(level, ToastLevel::Error));
            }
        }

        let mut completed_images = Vec::new();
        for (id, preview) in &mut self.image_previews {
            let Some(receiver) = &preview.receiver else {
                continue;
            };
            match receiver.try_recv() {
                Ok(result) => {
                    preview.receiver = None;
                    completed_images.push((*id, result));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    preview.receiver = None;
                    completed_images.push((*id, Err("Image decoder stopped unexpectedly".into())));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        for (id, result) in completed_images {
            match result {
                Ok(image) => {
                    if let Some(preview) = self.image_previews.get_mut(&id) {
                        preview.decoded = Some(image);
                    }
                }
                Err(error) => {
                    if let Some(preview) = self.image_previews.get_mut(&id) {
                        preview.error = Some(error.clone());
                    }
                    self.set_notice(format!("Could not preview image: {error}"), true);
                }
            }
        }
        let mut completed_hex = Vec::new();
        for (id, preview) in &mut self.hex_previews {
            let Some(receiver) = &preview.receiver else {
                continue;
            };
            match receiver.try_recv() {
                Ok(result) => {
                    preview.receiver = None;
                    completed_hex.push((*id, result));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    preview.receiver = None;
                    completed_hex.push((*id, Err("Binary reader stopped unexpectedly".into())));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        for (id, result) in completed_hex {
            match result {
                Ok(bytes) => {
                    if let Some(preview) = self.hex_previews.get_mut(&id) {
                        preview.bytes = Some(bytes);
                    }
                }
                Err(error) => {
                    if let Some(preview) = self.hex_previews.get_mut(&id) {
                        preview.error = Some(error.clone());
                    }
                    self.set_notice(format!("Could not read binary file: {error}"), true);
                }
            }
        }

        if self.file_index.poll() {
            // index refreshed
        }

        if let Some(receiver) = &self.project_detect_receiver {
            match receiver.try_recv() {
                Ok(info) => {
                    self.project_info = info;
                    self.project_detect_receiver = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.project_detect_receiver = None;
                }
            }
        }

        if self.file_search.running {
            if let Some(receiver) = &self.file_search.receiver {
                match receiver.try_recv() {
                    Ok(hits) => {
                        self.file_search.results = hits;
                        self.file_search.running = false;
                        self.file_search.receiver = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.file_search.running = false;
                        self.file_search.receiver = None;
                    }
                }
            }
        }

        self.lsp.poll();

        // Route LSP "go to definition" results.
        if let Some(location) = self.lsp.take_pending_jump() {
            if let Some(path) = raven_lsp::client::uri_to_path(&location.uri) {
                if let Some(id) = self.open_file(&path) {
                    self.jump_to_position(
                        ctx,
                        id,
                        location.range.start.line,
                        location.range.start.character,
                    );
                }
            }
        }

        // Route formatting edits.
        if let Some((uri, edits)) = self.lsp.take_pending_format() {
            if let Some(path) = raven_lsp::client::uri_to_path(&uri) {
                if let Some(id) = self
                    .docs
                    .values()
                    .find(|doc| doc.doc.path() == Some(path.as_path()))
                    .map(|doc| doc.id)
                {
                    self.apply_format_edits(id, &edits);
                }
            }
        }

        self.sync_lsp_changes();
    }

    pub fn jump_to_position(
        &mut self,
        ctx: &egui::Context,
        doc_id: u64,
        line: u32,
        character: u32,
    ) {
        let Some(state) = self.docs.get(&doc_id) else {
            return;
        };
        let text = state.doc.text().to_string();
        let byte = byte_of_line_char(&text, line as usize, character as usize);
        let char_index = text[..byte].chars().count();

        let id = egui::Id::new(("editor", self.active_pane, doc_id));
        if let Some(mut edit_state) = egui::text_edit::TextEdit::load_state(ctx, id) {
            edit_state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor {
                    index: egui::text::CharIndex(char_index),
                    prefer_next_row: true,
                })));
            egui::text_edit::TextEdit::store_state(ctx, id, edit_state);
        }
    }

    /// Alias used by the diagnostics panel (keeps call sites readable).
    pub fn jump_to_position_at(
        &mut self,
        ctx: &egui::Context,
        doc_id: u64,
        line: u32,
        character: u32,
    ) {
        self.jump_to_position(ctx, doc_id, line, character);
    }

    /// Jumps to the start of a 0-based line (go-to-line, search hits).
    pub fn jump_to_line(&mut self, ctx: &egui::Context, doc_id: u64, line: usize) {
        self.jump_to_position(ctx, doc_id, line as u32, 0);
    }

    fn apply_format_edits(&mut self, doc_id: u64, edits: &[raven_lsp::messages::TextEdit]) {
        let Some(state) = self.docs.get_mut(&doc_id) else {
            return;
        };
        let text = state.doc.text().to_string();
        let formatted = apply_text_edits(&text, edits);
        if let Some(formatted) = formatted {
            state.doc.set_text(&formatted);
            state.modified = true;
            state.rev += 1;
            if let Some(doc) = self.highlighters.get_mut(&doc_id) {
                doc.reset(0, doc.enabled);
                doc.invalidate_from(0, &formatted);
            }
            if let Some(slot) = self.job_caches.get_mut(&doc_id) {
                *slot = None;
            }
            self.set_notice("document formatted by language server", false);
        }
    }

    // -------------------------------------------------------- drag & drop ----

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        for path in dropped {
            if path.is_dir() {
                self.open_folder(&path);
            } else {
                self.open_file(&path);
            }
        }
    }

    // ---------------------------------------------------------- settings ----

    fn maybe_save_settings(&mut self, ctx: &egui::Context) {
        // Persist window geometry continuously (cheap, debounced).
        let (width, height) = ctx.input(|input| {
            (
                input
                    .viewport()
                    .inner_rect
                    .map(|rect| rect.width())
                    .unwrap_or(0.0),
                input
                    .viewport()
                    .inner_rect
                    .map(|rect| rect.height())
                    .unwrap_or(0.0),
            )
        });
        if width > 100.0 && height > 100.0 {
            self.settings.window.inner_width = width;
            self.settings.window.inner_height = height;
        }

        let elapsed = self.last_settings_save.elapsed();
        if self.settings_dirty && elapsed > std::time::Duration::from_secs(1) {
            let _ = self.settings.save_to(&self.settings_dir);
            self.settings_dirty = false;
            self.last_settings_save = std::time::Instant::now();
        }
    }

    fn update_window_title(&mut self, ctx: &egui::Context) {
        let title = match (self.active_doc(), self.workspace.as_ref()) {
            (Some(doc), _) => {
                let modified = if doc.modified { "• " } else { "" };
                format!("{modified}{} — Raven Code", doc.display_name())
            }
            (None, Some(workspace)) => format!("{} — Raven Code", workspace.display_name()),
            _ => format!("Raven Code {}", env!("CARGO_PKG_VERSION")),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

/// Byte offset for an LSP position (line, UTF-16 column).
pub fn preview_kind(path: &Path) -> Option<PreviewKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "md" | "markdown" | "mdown" => PreviewKind::Markdown,
        "json" => PreviewKind::Json,
        "csv" | "tsv" => PreviewKind::Delimited,
        _ => return None,
    })
}

pub fn byte_of_line_char(text: &str, line: usize, utf16_col: usize) -> usize {
    let mut current_line = 0usize;
    let mut utf16_seen = 0usize;
    let mut byte = 0usize;
    for (index, character) in text.char_indices() {
        if current_line == line {
            if utf16_seen >= utf16_col {
                return index;
            }
            utf16_seen += character.len_utf16();
            byte = index + character.len_utf8();
        } else if character == '\n' {
            current_line += 1;
            byte = index + 1;
            if current_line == line {
                utf16_seen = 0;
            }
        }
    }
    if current_line == line {
        return text.len();
    }
    byte.min(text.len())
}

/// Applies LSP text edits (sorted, non-overlapping in practice) to a string.
pub fn apply_text_edits(text: &str, edits: &[raven_lsp::messages::TextEdit]) -> Option<String> {
    if edits.is_empty() {
        return None;
    }
    let mut edits: Vec<&raven_lsp::messages::TextEdit> = edits.iter().collect();
    edits.sort_by_key(|edit| {
        (
            edit.range.start.line,
            std::cmp::Reverse(edit.range.start.character),
        )
    });
    let mut out = String::with_capacity(text.len() + 64);
    let mut last_byte = 0usize;
    for edit in edits {
        let start = byte_of_line_char(
            text,
            edit.range.start.line as usize,
            edit.range.start.character as usize,
        );
        let end = byte_of_line_char(
            text,
            edit.range.end.line as usize,
            edit.range.end.character as usize,
        );
        if start < last_byte || end < start {
            continue; // overlapping/malformed: skip defensively
        }
        out.push_str(&text[last_byte..start]);
        out.push_str(&edit.new_text);
        last_byte = end;
    }
    out.push_str(&text[last_byte..]);
    Some(out)
}

#[cfg(test)]
mod close_workflow_tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "raven-close-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn test_app(root: &Path) -> RavenApp {
        RavenApp::new(
            Settings::default(),
            root.join("settings"),
            None,
            Vec::new(),
            None,
        )
    }

    fn add_dirty_doc(app: &mut RavenApp, id: u64, path: PathBuf, text: &str) {
        let mut doc = Document::new_empty();
        doc.set_path(Some(path));
        doc.set_text(text);
        app.docs.insert(
            id,
            DocumentState {
                id,
                doc,
                modified: true,
                language: &language::PLAIN_TEXT,
                rev: 1,
                lsp_version: 1,
                lsp_open: false,
                syntax_name: "Plain Text".to_string(),
                last_edit_at: std::time::Instant::now(),
                last_saved_at: None,
                last_save_was_auto: false,
            },
        );
    }

    #[test]
    fn rvenc_workspace_round_trips_relative_root() {
        let root = temp_root("workspace");
        let file = root.join("project.RVENC");
        assert!(is_workspace_file(&file));
        let encoded = encode_workspace_file(&root, &file).unwrap();
        assert_eq!(resolve_workspace_root(&file, &encoded).unwrap(), root);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rvenc_rejects_unknown_format_versions() {
        let root = temp_root("workspace-version");
        let file = root.join("project.rvenc");
        let error = resolve_workspace_root(&file, r#"{"version":2,"root":"."}"#).unwrap_err();
        assert!(error.contains("Unsupported"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn quit_guard_reports_the_actual_dirty_document_count() {
        let root = temp_root("count");
        let mut app = test_app(&root);
        add_dirty_doc(&mut app, 1, root.join("one.rs"), "one");
        add_dirty_doc(&mut app, 2, root.join("two.rs"), "two");
        app.request_quit();

        let Some(Dialog::UnsavedConfirm {
            doc_ids,
            pending: PendingClose::Quit,
            ..
        }) = app.dialog
        else {
            panic!("quit must be held behind the unsaved confirmation");
        };
        assert_eq!(doc_ids.len(), 2);
        assert!(!app.close_when_ready);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_save_keeps_dirty_document_and_does_not_close() {
        let root = temp_root("failure");
        let mut app = test_app(&root);
        let missing_parent = root.join("missing").join("file.rs");
        add_dirty_doc(&mut app, 1, missing_parent, "unsaved");

        app.save_for_pending_close(&[1], PendingClose::Quit);

        assert!(app.docs.get(&1).unwrap().modified);
        assert!(!app.close_when_ready);
        assert!(matches!(
            app.dialog,
            Some(Dialog::UnsavedConfirm {
                pending: PendingClose::Quit,
                error: Some(_),
                ..
            })
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn save_all_closes_only_after_every_file_is_written() {
        let root = temp_root("success");
        let mut app = test_app(&root);
        add_dirty_doc(&mut app, 1, root.join("one.rs"), "one");
        add_dirty_doc(&mut app, 2, root.join("two.rs"), "two");

        app.save_for_pending_close(&[1, 2], PendingClose::Quit);

        assert_eq!(std::fs::read_to_string(root.join("one.rs")).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(root.join("two.rs")).unwrap(), "two");
        assert!(app.docs.values().all(|doc| !doc.modified));
        assert!(app.close_when_ready);
        let _ = std::fs::remove_dir_all(root);
    }
}
