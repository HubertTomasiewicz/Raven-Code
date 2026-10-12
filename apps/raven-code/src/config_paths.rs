//! Platform-isolated configuration paths.
//!
//! Windows is the primary Raven Code target (`%APPDATA%\RavenCode`);
//! Unix-like systems follow the XDG base directory spec. Keeping this in
//! one place makes future platform support auditable.

use std::ffi::OsStr;
#[cfg(not(windows))]
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The per-user configuration directory for Raven Code.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_default();
                home.join("AppData").join("Roaming")
            });
        base.join("RavenCode")
    }

    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        base.join("raven-code")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    File,
    Folder,
}

/// Opens a system file or folder picker through the desktop portal (Linux)
/// or the native dialog (Windows). Returns `None` when the user cancels or
/// no dialog backend is available.
pub fn native_pick(kind: PickKind, start: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    match kind {
        PickKind::File => dialog.pick_file(),
        PickKind::Folder => dialog.pick_folder(),
    }
}

pub fn native_pick_filtered(
    start: Option<&Path>,
    filter_name: &str,
    extensions: &[&str],
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().add_filter(filter_name, extensions);
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_file()
}

/// Opens the operating system's Save As dialog. `rfd` uses Windows' native
/// IFileDialog on Windows and the platform dialog/portal elsewhere.
pub fn native_save_file(
    start: Option<&Path>,
    suggested_name: &str,
    filter: Option<(&str, &[&str])>,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_file_name(suggested_name);
    if let Some(start) = start {
        dialog = dialog.set_directory(start);
    }
    if let Some((name, extensions)) = filter {
        dialog = dialog.add_filter(name, extensions);
    }
    dialog.save_file()
}

/// Opens a directory in the platform's file manager.
pub fn open_in_file_manager(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open {}: {error}", path.display()))
}

/// Opens an external terminal at a directory without blocking the editor.
///
/// This is the honest alternative to a fake integrated terminal (see
/// docs/raven-code/limitations.md): the user gets a real shell.
pub fn open_external_terminal(dir: &Path, custom: Option<&str>) -> Result<(), String> {
    if let Some(custom) = custom {
        let (program, args) = split_command_line(custom)?;
        return spawn_detached(&program, &args, dir);
    }

    #[cfg(windows)]
    {
        // `start` opens a new console window that inherits our working directory.
        spawn_detached("cmd.exe", &["/C", "start", "cmd.exe", "/K"], dir)
    }

    #[cfg(not(windows))]
    {
        // Terminals that are happy with just the child's working directory
        // (set via `current_dir`) get no extra arguments. The others are
        // single-instance servers or take an explicit flag, so we pass the
        // directory to them directly as well.
        const CANDIDATES: &[&str] = &[
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "alacritty",
            "kitty",
            "xterm",
        ];

        for candidate in CANDIDATES {
            if which(candidate).is_none() {
                continue;
            }
            let args: Vec<OsString> = match *candidate {
                "gnome-terminal" => vec![flag_with_value("--working-directory=", dir)],
                "konsole" => vec!["--workdir".into(), dir.as_os_str().to_owned()],
                "xfce4-terminal" => vec![flag_with_value("--default-working-directory=", dir)],
                "alacritty" => vec!["--working-directory".into(), dir.as_os_str().to_owned()],
                "kitty" => vec!["--directory".into(), dir.as_os_str().to_owned()],
                _ => Vec::new(),
            };
            return spawn_detached(candidate, &args, dir);
        }
        Err("no terminal emulator found; set [terminal] external_command in settings".into())
    }
}

/// Builds a `--flag=value` style argument without lossy UTF-8 conversion.
#[cfg(not(windows))]
fn flag_with_value(prefix: &str, value: &Path) -> OsString {
    let mut arg = OsString::from(prefix);
    arg.push(value.as_os_str());
    arg
}

fn spawn_detached<S: AsRef<OsStr>>(program: &str, args: &[S], dir: &Path) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(dir);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // Detach: the terminal outlives the editor window.
        command.process_group(0);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // The launcher `cmd.exe` is only a trampoline for `start`; hide its
        // own console so it doesn't flash. The terminal `start` opens is unaffected.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("failed to launch '{program}': {error}"))
}

/// Splits a command line into a program and its arguments.
///
/// Whitespace separates tokens; single or double quotes group text that
/// contains spaces (e.g. `"C:\Program Files\Terminal\term.exe" --flag`).
/// There is no escape syntax: a quote character always opens or closes a group.
fn split_command_line(line: &str) -> Result<(String, Vec<String>), String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;

    for ch in line.chars() {
        match (quote, ch) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(ch);
                in_token = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_token = true;
            }
        }
    }

    if quote.is_some() {
        return Err("unterminated quote in command".to_string());
    }
    if in_token {
        tokens.push(current);
    }

    let mut tokens = tokens.into_iter();
    let program = tokens.next().ok_or("empty command")?;
    Ok((program, tokens.collect()))
}

/// Minimal `which`: first executable file named `program` on `PATH`.
#[cfg(not(windows))]
fn which(program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;

    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| {
            candidate
                .metadata()
                .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}
