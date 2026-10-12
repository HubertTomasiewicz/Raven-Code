//! Editor actions and keyboard shortcuts.
//!
//! Every user-invokable command is an [`Action`]. Default chords live here;
//! the settings file's `[keys]` table can override any of them by action id
//! (`"save" = "Ctrl+S"`), parsed by [`parse_chord`].

use eframe::egui;

/// A parsed keyboard chord: modifiers plus a logical key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}

impl Chord {
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: egui::Key) -> Chord {
        Chord {
            ctrl,
            shift,
            alt,
            key,
        }
    }

    /// True when this chord's key was pressed with these modifiers this frame.
    pub fn pressed(&self, input: &egui::InputState) -> bool {
        if !input.key_pressed(self.key) {
            return false;
        }
        let modifiers = input.modifiers;
        // `command` equals ctrl on Windows/Linux; use it for ctrl so mac
        // users get the same muscle memory.
        let command = modifiers.command;
        let ctrl_match = self.ctrl == command;
        let shift_match = self.shift == modifiers.shift;
        let alt_match = self.alt == modifiers.alt;
        ctrl_match && shift_match && alt_match
    }

    /// "Ctrl+Shift+P" style label.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(key_label(self.key));
        out
    }
}

fn key_label(key: egui::Key) -> &'static str {
    use egui::Key as K;
    match key {
        K::ArrowDown => "Down",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::ArrowUp => "Up",
        K::Escape => "Esc",
        K::Tab => "Tab",
        K::Backspace => "Backspace",
        K::Enter => "Enter",
        K::Space => "Space",
        K::Insert => "Insert",
        K::Delete => "Delete",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        K::F1 => "F1",
        K::F2 => "F2",
        K::F3 => "F3",
        K::F4 => "F4",
        K::F5 => "F5",
        K::F6 => "F6",
        K::F7 => "F7",
        K::F8 => "F8",
        K::F9 => "F9",
        K::F10 => "F10",
        K::F11 => "F11",
        K::F12 => "F12",
        K::Minus => "-",
        K::Equals => "=",
        K::Plus => "+",
        K::Slash => "/",
        K::Backtick => "`",
        // Letters and digits.
        K::A => "A",
        K::B => "B",
        K::C => "C",
        K::D => "D",
        K::E => "E",
        K::F => "F",
        K::G => "G",
        K::H => "H",
        K::I => "I",
        K::J => "J",
        K::K => "K",
        K::L => "L",
        K::M => "M",
        K::N => "N",
        K::O => "O",
        K::P => "P",
        K::Q => "Q",
        K::R => "R",
        K::S => "S",
        K::T => "T",
        K::U => "U",
        K::V => "V",
        K::W => "W",
        K::X => "X",
        K::Y => "Y",
        K::Z => "Z",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        other => {
            let _ = other;
            "?"
        }
    }
}

/// Parses "Ctrl+Shift+P", "F5", "Alt+F4" ... into a chord; `None` on
/// anything unparseable (the default then stays in force).
pub fn parse_chord(text: &str) -> Option<Chord> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key: Option<egui::Key> = None;

    for part in text.split('+') {
        let part = part.trim();
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" | "cmd" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" => alt = true,
            _ => {
                key = parse_key(part);
            }
        }
    }
    Some(Chord::new(ctrl, shift, alt, key?))
}

fn parse_key(text: &str) -> Option<egui::Key> {
    use egui::Key as K;
    let lower = text.to_ascii_lowercase();
    Some(match lower.as_str() {
        "down" | "arrowdown" => K::ArrowDown,
        "left" | "arrowleft" => K::ArrowLeft,
        "right" | "arrowright" => K::ArrowRight,
        "up" | "arrowup" => K::ArrowUp,
        "esc" | "escape" => K::Escape,
        "tab" => K::Tab,
        "backspace" => K::Backspace,
        "enter" | "return" => K::Enter,
        "space" => K::Space,
        "insert" => K::Insert,
        "delete" | "del" => K::Delete,
        "home" => K::Home,
        "end" => K::End,
        "pageup" => K::PageUp,
        "pagedown" => K::PageDown,
        "f1" => K::F1,
        "f2" => K::F2,
        "f3" => K::F3,
        "f4" => K::F4,
        "f5" => K::F5,
        "f6" => K::F6,
        "f7" => K::F7,
        "f8" => K::F8,
        "f9" => K::F9,
        "f10" => K::F10,
        "f11" => K::F11,
        "f12" => K::F12,
        "-" | "minus" => K::Minus,
        "=" | "equals" => K::Equals,
        "+" | "plus" => K::Plus,
        "/" | "slash" => K::Slash,
        "`" | "backtick" => K::Backtick,
        _ => {
            // Single letters map by name.
            if text.len() == 1 && text.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                return letter_key(&lower);
            }
            return None;
        }
    })
}

fn letter_key(lower: &str) -> Option<egui::Key> {
    use egui::Key as K;
    Some(match lower {
        "a" => K::A,
        "b" => K::B,
        "c" => K::C,
        "d" => K::D,
        "e" => K::E,
        "f" => K::F,
        "g" => K::G,
        "h" => K::H,
        "i" => K::I,
        "j" => K::J,
        "k" => K::K,
        "l" => K::L,
        "m" => K::M,
        "n" => K::N,
        "o" => K::O,
        "p" => K::P,
        "q" => K::Q,
        "r" => K::R,
        "s" => K::S,
        "t" => K::T,
        "u" => K::U,
        "v" => K::V,
        "w" => K::W,
        "x" => K::X,
        "y" => K::Y,
        "z" => K::Z,
        "0" | "num0" => K::Num0,
        "1" | "num1" => K::Num1,
        "2" | "num2" => K::Num2,
        "3" | "num3" => K::Num3,
        "4" | "num4" => K::Num4,
        "5" | "num5" => K::Num5,
        "6" | "num6" => K::Num6,
        "7" | "num7" => K::Num7,
        "8" | "num8" => K::Num8,
        "9" | "num9" => K::Num9,
        _ => return None,
    })
}

/// Every command the editor exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // File
    NewFile,
    OpenFile,
    OpenFolder,
    OpenWorkspace,
    SaveWorkspaceAs,
    Save,
    SaveAs,
    SaveAll,
    CloseTab,
    CloseAllTabs,
    CloseFolder,
    Quit,

    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,
    Replace,
    GoToLine,

    // View
    CommandPalette,
    ToggleSidebar,
    ToggleBottomPanel,
    ToggleWordWrap,
    ZoomIn,
    ZoomOut,
    ResetZoom,
    SplitEditor,
    TogglePreview,
    FormatJson,
    ThemeToggle,
    RefreshExplorer,

    // Go
    QuickOpen,
    NextTab,
    PreviousTab,

    // Run
    BuildProject,
    TestProject,
    RunProject,
    CancelJob,
    ClearOutput,

    // Help
    Shortcuts,
    About,

    // Language intelligence (optional, needs a configured server)
    CompletionTrigger,
    GotoDefinition,
    HoverInfo,
    FormatDocument,

    // Tools
    RunCommand,
    OpenTerminal,
    OpenPath,
}

impl Action {
    /// Stable id for the settings file and command palette.
    pub fn id(self) -> &'static str {
        use Action::*;
        match self {
            NewFile => "new_file",
            OpenFile => "open_file",
            OpenFolder => "open_folder",
            OpenWorkspace => "open_workspace",
            SaveWorkspaceAs => "save_workspace_as",
            Save => "save",
            SaveAs => "save_as",
            SaveAll => "save_all",
            CloseTab => "close_tab",
            CloseAllTabs => "close_all_tabs",
            CloseFolder => "close_folder",
            Quit => "quit",
            Undo => "undo",
            Redo => "redo",
            Cut => "cut",
            Copy => "copy",
            Paste => "paste",
            SelectAll => "select_all",
            Find => "find",
            Replace => "replace",
            GoToLine => "go_to_line",
            CommandPalette => "command_palette",
            ToggleSidebar => "toggle_sidebar",
            ToggleBottomPanel => "toggle_bottom_panel",
            ToggleWordWrap => "toggle_word_wrap",
            ZoomIn => "zoom_in",
            ZoomOut => "zoom_out",
            ResetZoom => "reset_zoom",
            SplitEditor => "split_editor",
            TogglePreview => "toggle_preview",
            FormatJson => "format_json",
            ThemeToggle => "toggle_theme",
            RefreshExplorer => "refresh_explorer",
            QuickOpen => "quick_open",
            NextTab => "next_tab",
            PreviousTab => "previous_tab",
            BuildProject => "build_project",
            TestProject => "test_project",
            RunProject => "run_project",
            CancelJob => "cancel_job",
            ClearOutput => "clear_output",
            Shortcuts => "help.shortcuts",
            About => "help.about",
            CompletionTrigger => "lsp.completion",
            GotoDefinition => "lsp.goto_definition",
            HoverInfo => "lsp.hover",
            FormatDocument => "lsp.format",
            RunCommand => "run.command",
            OpenTerminal => "run.terminal",
            OpenPath => "file.open_path",
        }
    }

    /// Human label used in the palette and menus.
    pub fn label(self) -> &'static str {
        use Action::*;
        match self {
            NewFile => "File: New File",
            OpenFile => "File: Open File…",
            OpenFolder => "File: Open Folder…",
            OpenWorkspace => "File: Open Workspace…",
            SaveWorkspaceAs => "File: Save Workspace As…",
            Save => "File: Save",
            SaveAs => "File: Save As…",
            SaveAll => "File: Save All",
            CloseTab => "File: Close Tab",
            CloseAllTabs => "File: Close All Tabs",
            CloseFolder => "File: Close Folder",
            Quit => "File: Exit",
            Undo => "Edit: Undo",
            Redo => "Edit: Redo",
            Cut => "Edit: Cut",
            Copy => "Edit: Copy",
            Paste => "Edit: Paste",
            SelectAll => "Edit: Select All",
            Find => "Edit: Find",
            Replace => "Edit: Replace",
            GoToLine => "Go: Go to Line…",
            CommandPalette => "View: Command Palette",
            ToggleSidebar => "View: Toggle Explorer",
            ToggleBottomPanel => "View: Toggle Bottom Panel",
            ToggleWordWrap => "View: Toggle Word Wrap",
            ZoomIn => "View: Zoom In",
            ZoomOut => "View: Zoom Out",
            ResetZoom => "View: Reset Zoom",
            SplitEditor => "View: Toggle Split Editor",
            TogglePreview => "View: Toggle File Preview",
            FormatJson => "Edit: Format JSON",
            ThemeToggle => "View: Toggle Dark/Light Theme",
            RefreshExplorer => "View: Refresh Explorer",
            QuickOpen => "Go: Quick Open File…",
            NextTab => "Go: Next Tab",
            PreviousTab => "Go: Previous Tab",
            BuildProject => "Run: Build Project",
            TestProject => "Run: Test Project",
            RunProject => "Run: Run Project",
            CancelJob => "Run: Cancel Running Job",
            ClearOutput => "Run: Clear Output",
            Shortcuts => "Help: Keyboard Shortcuts",
            About => "Help: About Raven Code",
            CompletionTrigger => "LSP: Trigger Completion (Ctrl+Space)",
            GotoDefinition => "LSP: Go to Definition (F12)",
            HoverInfo => "LSP: Hover Info",
            FormatDocument => "LSP: Format Document",
            RunCommand => "Run: Run Command…",
            OpenTerminal => "Run: Open External Terminal",
            OpenPath => "File: Open Path…",
        }
    }

    /// The default chord, when the action has one. Undo/Redo/Cut/Copy/
    /// Paste/SelectAll are handled natively by the text widget and listed
    /// for documentation only.
    pub fn default_chord(self) -> Option<Chord> {
        use Action::*;
        use egui::Key as K;
        Some(match self {
            NewFile => Chord::new(true, false, false, K::N),
            OpenFile => Chord::new(true, false, false, K::O),
            OpenFolder => Chord::new(true, true, false, K::O),
            Save => Chord::new(true, false, false, K::S),
            SaveAs => Chord::new(true, true, false, K::S),
            SaveAll => Chord::new(true, false, true, K::S),
            CloseTab => Chord::new(true, false, false, K::W),
            Find => Chord::new(true, false, false, K::F),
            Replace => Chord::new(true, false, false, K::H),
            GoToLine => Chord::new(true, false, false, K::G),
            CommandPalette => Chord::new(true, true, false, K::P),
            ToggleSidebar => Chord::new(true, false, false, K::B),
            ToggleBottomPanel => Chord::new(true, false, false, K::J),
            ToggleWordWrap => Chord::new(true, true, false, K::W),
            ZoomIn => Chord::new(true, false, false, K::Equals),
            ZoomOut => Chord::new(true, false, false, K::Minus),
            ResetZoom => Chord::new(true, false, false, K::Num0),
            SplitEditor => Chord::new(true, false, false, K::Backslash),
            QuickOpen => Chord::new(true, false, false, K::P),
            NextTab => Chord::new(true, false, false, K::PageDown),
            PreviousTab => Chord::new(true, false, false, K::PageUp),
            BuildProject => Chord::new(true, true, false, K::B),
            TestProject => Chord::new(true, true, false, K::T),
            RunProject => Chord::new(false, false, true, K::R),
            CancelJob => Chord::new(true, true, false, K::C),
            ClearOutput => Chord::new(true, false, true, K::K),
            RefreshExplorer => Chord::new(false, false, false, K::F5),
            ThemeToggle => Chord::new(true, true, false, K::D),
            CompletionTrigger => Chord::new(true, false, false, K::Space),
            GotoDefinition => Chord::new(false, false, false, K::F12),
            HoverInfo => Chord::new(false, false, true, K::H),
            FormatDocument => Chord::new(false, true, true, K::F),
            _ => return None,
        })
    }

    /// The effective chord: settings `[keys]` override the default.
    pub fn chord(self, settings: &raven_editor::settings::Settings) -> Option<Chord> {
        if let Some(raw) = settings.keys.get(self.id()) {
            return parse_chord(raw);
        }
        self.default_chord()
    }

    /// All actions with default chords, for the shortcuts help dialog.
    pub fn all() -> &'static [Action] {
        ALL
    }
}

const ALL: &[Action] = &[
    Action::NewFile,
    Action::OpenFile,
    Action::OpenFolder,
    Action::OpenWorkspace,
    Action::SaveWorkspaceAs,
    Action::Save,
    Action::SaveAs,
    Action::SaveAll,
    Action::CloseTab,
    Action::CloseAllTabs,
    Action::CloseFolder,
    Action::Quit,
    Action::Undo,
    Action::Redo,
    Action::Cut,
    Action::Copy,
    Action::Paste,
    Action::SelectAll,
    Action::Find,
    Action::Replace,
    Action::GoToLine,
    Action::CommandPalette,
    Action::QuickOpen,
    Action::ToggleSidebar,
    Action::ToggleBottomPanel,
    Action::ToggleWordWrap,
    Action::ZoomIn,
    Action::ZoomOut,
    Action::ResetZoom,
    Action::SplitEditor,
    Action::TogglePreview,
    Action::FormatJson,
    Action::ThemeToggle,
    Action::RefreshExplorer,
    Action::NextTab,
    Action::PreviousTab,
    Action::BuildProject,
    Action::TestProject,
    Action::RunProject,
    Action::CancelJob,
    Action::ClearOutput,
    Action::Shortcuts,
    Action::About,
    Action::CompletionTrigger,
    Action::GotoDefinition,
    Action::HoverInfo,
    Action::FormatDocument,
    Action::RunCommand,
    Action::OpenTerminal,
    Action::OpenPath,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_roundtrip() {
        let chord = parse_chord("Ctrl+Shift+P").unwrap();
        assert!(chord.ctrl && chord.shift && !chord.alt);
        assert_eq!(chord.key, egui::Key::P);
        assert_eq!(chord.label(), "Ctrl+Shift+P");

        let chord = parse_chord("F5").unwrap();
        assert!(!chord.ctrl && !chord.shift && !chord.alt);
        assert_eq!(chord.key, egui::Key::F5);

        assert!(parse_chord("Banana+Split").is_none());
    }

    #[test]
    fn ids_are_unique_and_labels_exist() {
        let mut ids: Vec<&str> = ALL.iter().map(|action| action.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len());
        for action in ALL {
            assert!(!action.label().is_empty());
        }
    }

    #[test]
    fn settings_override_wins() {
        let mut settings = raven_editor::settings::Settings::default();
        settings
            .keys
            .insert("save".to_string(), "Alt+Q".to_string());
        let chord = Action::Save.chord(&settings).unwrap();
        assert_eq!(chord.key, egui::Key::Q);
        assert!(chord.alt);
    }
}
