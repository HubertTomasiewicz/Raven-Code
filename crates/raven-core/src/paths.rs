//! Path display and serialization rules (one implementation, everywhere).
//!
//! On Windows, `std::fs::canonicalize` returns *verbatim* paths
//! (`\\?\C:\Users\dev`). Verbatim paths are great for the filesystem and
//! terrible for humans and JSON consumers. Raven Pico keeps verbatim paths
//! internally (semantics are never lost) and serializes them for humans and
//! JSON through a single rule set:
//!
//! * `\\?\C:\dir\file`   → `C:\dir\file` (classic drive path)
//! * `\\?\UNC\srv\share` → `\\srv\share` (classic UNC path)
//! * everything else (`\\.\PHYSICALDRIVE0`, `\\?\Volume{...}`, `\\?\C:`)
//!   stays verbatim — it has no shorter equivalent.
//!
//! The prefix is only stripped when stripping cannot change what the path
//! points at. The verbatim form is kept when any of the following holds:
//!
//! * a component ends with a dot or a space — Win32 strips trailing dots and
//!   spaces, so the cleaned form would resolve to a *different* name,
//! * a component is a reserved DOS device name (`CON`, `NUL`, `COM1`, ...,
//!   also with an extension such as `CON.txt`) — without the prefix these
//!   resolve to devices, not files,
//! * the cleaned path would exceed 240 characters (legacy `MAX_PATH` = 260
//!   with headroom; without the prefix, longer paths only resolve on systems
//!   with long-path awareness enabled).
//!
//! Unicode and spaces never influence the decision — they survive unchanged.
//! The transformation is pure string logic with no filesystem access, so it
//! behaves identically on every platform and is fully unit-testable on any
//! of them.

use std::path::{Path, PathBuf};

/// Longest cleaned path produced before the verbatim prefix is kept instead.
///
/// 240 leaves headroom below the legacy 260-character `MAX_PATH` limit for a
/// working directory or a relative remainder.
const MAX_CLEANED_LEN: usize = 240;

/// DOS device names that only behave as ordinary files inside a verbatim
/// path (Microsoft's reserved namespace, including the `0` variants).
const RESERVED_DEVICES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", //
    "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", //
    "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// True when a component would change meaning once the verbatim prefix is
/// removed: trailing dots/spaces are stripped by Win32, reserved device
/// names resolve to devices, and `.`/`..` stop being literal (verbatim
/// paths skip normalization; ordinary paths do not).
fn component_needs_verbatim(comp: &str) -> bool {
    if comp == "." || comp == ".." {
        return true;
    }
    if comp.ends_with('.') || comp.ends_with(' ') {
        return true;
    }
    // Reserved names keep their meaning even with an extension: `CON.txt`.
    let stem = comp.split_once('.').map_or(comp, |(stem, _)| stem);
    RESERVED_DEVICES
        .iter()
        .any(|device| device.eq_ignore_ascii_case(stem))
}

/// True when the cleaned (prefix-stripped) form is guaranteed to point at
/// the same target as the verbatim original.
///
/// Components are split on `/` and `\` manually instead of via
/// [`Path::components`], whose parsing is platform-dependent (on Unix a
/// backslash-separated Windows path is a single component).
fn cleaned_is_safe(candidate: &str) -> bool {
    if candidate.chars().count() > MAX_CLEANED_LEN {
        return false;
    }
    candidate
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .all(|part| !component_needs_verbatim(part))
}

/// True for classic drive paths such as `C:\Users` (which the verbatim
/// prefix can be dropped from), false for `C:` (drive-relative) and
/// non-drive payloads.
fn is_drive_path(candidate: &str) -> bool {
    let bytes = candidate.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

/// Returns `path` without the Windows verbatim prefix, but only when the
/// shorter form is provably equivalent (see the module documentation).
/// Non-verbatim, non-UTF-8 and non-drive paths are returned unchanged.
pub fn clean_verbatim(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf(); // non-UTF-8 path: leave untouched
    };

    if let Some(unc_rest) = text.strip_prefix(r"\\?\UNC\") {
        let candidate = format!(r"\\{unc_rest}");
        return if cleaned_is_safe(&candidate) {
            PathBuf::from(candidate)
        } else {
            path.to_path_buf()
        };
    }

    if let Some(rest) = text.strip_prefix(r"\\?\") {
        // Only classic drive paths have a safe shorter form; device paths,
        // volume GUIDs and drive-relative verbatim paths stay verbatim.
        if is_drive_path(rest) && cleaned_is_safe(rest) {
            return PathBuf::from(rest);
        }
        return path.to_path_buf();
    }

    path.to_path_buf()
}

/// The single string form used for human-readable output and JSON
/// serialization: [`clean_verbatim`] followed by a lossy display.
pub fn display_path(path: &Path) -> String {
    clean_verbatim(path).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_verbatim_drive_prefix() {
        assert_eq!(
            clean_verbatim(Path::new(r"\\?\C:\Users\dev\my project (copy)")),
            PathBuf::from(r"C:\Users\dev\my project (copy)")
        );
        // Round-trips back to the same location.
        assert_eq!(
            clean_verbatim(Path::new(r"\\?\C:\work")),
            PathBuf::from(r"C:\work")
        );
    }

    #[test]
    fn converts_verbatim_unc_to_standard_unc() {
        assert_eq!(
            clean_verbatim(Path::new(r"\\?\UNC\buildserver\share\proj")),
            PathBuf::from(r"\\buildserver\share\proj")
        );
    }

    #[test]
    fn keeps_device_and_volume_paths_verbatim() {
        for path in [r"\\.\PhysicalDrive0", r"\\?\Volume{a0b1c2}\", r"\\?\C:"] {
            assert_eq!(
                clean_verbatim(Path::new(path)),
                PathBuf::from(path),
                "{path} must stay verbatim"
            );
        }
    }

    #[test]
    fn keeps_verbatim_when_a_component_ends_with_dot_or_space() {
        for path in [r"\\?\C:\odd \dir.\file.txt", r"\\?\C:\trailing space "] {
            assert_eq!(
                clean_verbatim(Path::new(path)),
                PathBuf::from(path),
                "{path} must stay verbatim"
            );
        }
    }

    #[test]
    fn keeps_verbatim_for_reserved_device_names() {
        for path in [
            r"\\?\C:\docs\CON\out",
            r"\\?\C:\logs\con.txt",
            r"\\?\C:\NUL",
        ] {
            assert_eq!(
                clean_verbatim(Path::new(path)),
                PathBuf::from(path),
                "{path} must stay verbatim"
            );
        }
    }

    #[test]
    fn keeps_verbatim_for_very_long_paths() {
        // A path longer than the 240-character cleanup limit keeps its
        // prefix: without it the path only resolves with long-path support.
        let deep = r"\directory".repeat(24);
        let verbatim = format!(r"\\?\C:\root{deep}");
        assert!(verbatim.len() > 240);
        assert_eq!(
            clean_verbatim(Path::new(&verbatim)),
            PathBuf::from(verbatim)
        );
    }

    #[test]
    fn keeps_verbatim_when_it_contains_dot_components() {
        // Verbatim paths skip normalization: `..` stays literal there but
        // collapses once the prefix is gone — different targets.
        for path in [r"\\?\C:\a\..\b", r"\\?\C:\.\x"] {
            assert_eq!(
                clean_verbatim(Path::new(path)),
                PathBuf::from(path),
                "{path} must stay verbatim"
            );
        }
    }

    #[test]
    fn ordinary_paths_are_never_touched() {
        for path in [
            r"C:\Users\dev\proj",
            "/home/z/raven-pico",
            "relative/path",
            r"\\server\share\already-unc",
        ] {
            assert_eq!(
                clean_verbatim(Path::new(path)),
                PathBuf::from(path),
                "{path} must be unchanged"
            );
        }
    }

    #[test]
    fn unicode_paths_survive_the_cleanup() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users\développeur\проект")),
            r"C:\Users\développeur\проект"
        );
        assert_eq!(
            display_path(Path::new("/home/z/日本語プロジェクト")),
            "/home/z/日本語プロジェクト"
        );
    }

    #[test]
    fn display_path_and_path_to_string_agree() {
        let sample = Path::new(r"\\?\D:\a b\c.d");
        assert_eq!(super::display_path(sample), crate::path_to_string(sample));
    }

    #[test]
    fn empty_and_root_paths_are_stable() {
        assert_eq!(clean_verbatim(Path::new("")), PathBuf::from(""));
        assert_eq!(display_path(Path::new("/")), "/");
    }
}
