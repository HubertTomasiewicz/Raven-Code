//! Raven Code — the Raven ecosystem's lightweight, fast code and text editor.
//!
//! Entry point: parses command-line arguments, loads user settings, builds
//! the font stack and starts the eframe window. All application state lives
//! in [`app::RavenApp`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod code_editor;
mod config_paths;
mod file_previews;
mod highlight;
mod icons;
mod jobs;
mod lsp_bridge;
mod panels;
mod workspace;

use std::path::PathBuf;

use eframe::egui;

use app::RavenApp;
use config_paths::config_dir;
use raven_editor::settings::Settings;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Minimal argument surface: --help/--version, everything else is a
    // file or folder to open. Editors should start instantly; we do not
    // want a CLI framework here.
    for arg in &args {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "Raven Code {} — the Raven ecosystem's lightweight code editor\n\n\
                     USAGE:\n    raven-code [OPTIONS] [FILE|FOLDER]...\n\n\
                     OPTIONS:\n    -h, --help       Print this help\n    -V, --version    Print version\n\n\
                     Any other argument is opened as a file (in tabs) or as a folder (workspace).",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            "--version" | "-V" => {
                println!("Raven Code {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => {}
        }
    }

    let settings_dir = config_dir();
    let loaded = Settings::load_from(&settings_dir);
    let startup_notice = loaded.notice;
    let settings = loaded.settings;

    // Command-line paths become the initial session.
    let mut startup_files: Vec<PathBuf> = Vec::new();
    let mut startup_folder: Option<PathBuf> = None;
    for arg in &args {
        let path = PathBuf::from(arg);
        if startup_folder.is_none() && path.is_dir() {
            startup_folder = Some(path);
        } else {
            startup_files.push(path);
        }
    }
    if startup_folder.is_none()
        && startup_files.is_empty()
        && settings.workspace.restore_last_session
    {
        if let Some(folder) = settings.workspace.recent_folders.first() {
            let path = PathBuf::from(folder);
            if path.is_dir() {
                startup_folder = Some(path);
            }
        }
    }

    let window_title = format!("Raven Code {}", env!("CARGO_PKG_VERSION"));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&window_title)
            .with_inner_size([settings.window.inner_width, settings.window.inner_height])
            .with_min_inner_size([640.0, 400.0])
            .with_icon(icons::window_icon()),
        ..Default::default()
    };

    eframe::run_native(
        &window_title,
        options,
        Box::new(move |creation_context| {
            let fonts = build_fonts(&settings);
            creation_context.egui_ctx.set_fonts(fonts);
            Ok(Box::new(RavenApp::new(
                settings,
                settings_dir,
                startup_folder,
                startup_files,
                startup_notice,
            )))
        }),
    )
}

/// Builds the font stack: optional user font, egui defaults, and a best
/// effort system CJK fallback so non-Latin file names and content render.
fn build_fonts(settings: &Settings) -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();

    let insert = |fonts: &mut egui::FontDefinitions, name: &str, data: &[u8], first: bool| {
        if fonts.font_data.contains_key(name) {
            return;
        }
        fonts.font_data.insert(
            name.to_string(),
            egui::FontData::from_owned(data.to_vec()).into(),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let list = fonts.families.entry(family).or_default();
            if first {
                list.insert(0, name.to_string());
            } else {
                list.push(name.to_string());
            }
        }
    };

    // 1. User-configured font (settings.appearance.font_path).
    let font_path = settings.appearance.font_path.trim().to_string();
    if !font_path.is_empty() {
        match std::fs::read(&font_path) {
            Ok(data) => insert(&mut fonts, "raven-user-font", &data, true),
            Err(error) => eprintln!(
                "Raven Code: configured font '{}' could not be loaded ({}); using defaults",
                font_path, error
            ),
        }
    }

    // 2. System CJK fallback (optional; the app works fine without it).
    const CJK_CANDIDATES: &[&str] = &[
        "/usr/share/fonts/truetype/chinese/NotoSansSC-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
    ];
    for candidate in CJK_CANDIDATES {
        if let Ok(data) = std::fs::read(candidate) {
            insert(&mut fonts, "raven-cjk-fallback", &data, false);
            break;
        }
    }

    fonts
}
