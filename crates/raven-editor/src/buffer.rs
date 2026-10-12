//! Document buffers: load, edit, save — with honest encoding handling.
//!
//! Policy (deliberately conservative — the editor must never silently
//! corrupt a file it does not fully understand):
//!
//! * UTF-8 (with or without BOM) and UTF-16 LE/BE (BOM required) open
//!   **editable** and are saved back in the *same* encoding with the same
//!   BOM and the same line-ending style.
//! * Any other byte content (Latin-1, Shift-JIS, binary, ...) is refused
//!   with [`DocError::UnsupportedEncoding`] instead of being decoded lossily.
//! * Line endings are detected on load (first end-of-line wins) and restored
//!   on save. In-memory text always uses `\n`, so widgets and search see one
//!   consistent form; [`Document::save`] converts back to the target style.
//! * Large files follow [`LoadPolicy`]: syntax highlighting is skipped
//!   above [`LoadPolicy::highlight_off_bytes`] and the document opens
//!   read-only above [`LoadPolicy::readonly_bytes`]. Nothing above the read
//!   limit is loaded at all.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Hard upper bound for any file this editor will load (32 MiB).
const ABSOLUTE_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Errors surfaced while loading or saving a document.
#[derive(Debug, Error)]
pub enum DocError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),
    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),
    #[error("path is a directory, not a file: {0}")]
    IsADirectory(PathBuf),
    #[error(
        "unsupported encoding in {path}: the file is not valid UTF-8 or UTF-16 (it was not modified)"
    )]
    UnsupportedEncoding { path: PathBuf },
    #[error("file too large to open ({bytes} bytes; limit is {limit} bytes): {path}")]
    TooLarge {
        path: PathBuf,
        bytes: u64,
        limit: u64,
    },
    #[error("failed to write {path}: {source}")]
    WriteFailed {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read {path}: {source}")]
    ReadFailed {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("internal error: {0}")]
    Internal(String),
}

/// Text encodings this editor reads and writes.
///
/// The enum is stored per document: whatever was detected on load is used
/// verbatim on save, so opening and saving a file without edits produces a
/// byte-identical file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8 without BOM.
    Utf8,
    /// UTF-8 with BOM.
    Utf8Bom,
    /// UTF-16 little-endian with BOM.
    Utf16Le,
    /// UTF-16 big-endian with BOM.
    Utf16Be,
}

impl Encoding {
    /// Human-readable name shown in the status bar.
    pub fn display_name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
        }
    }
}

/// End-of-line style detected on load and restored on save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    /// The platform-native default for *new* files.
    pub fn native() -> LineEnding {
        if cfg!(windows) {
            LineEnding::Crlf
        } else {
            LineEnding::Lf
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

/// Thresholds deciding how a file is opened (see module docs).
#[derive(Debug, Clone, Copy)]
pub struct LoadPolicy {
    /// Above this size (bytes) syntax highlighting is disabled.
    pub highlight_off_bytes: u64,
    /// Above this size (bytes) the document opens read-only.
    pub readonly_bytes: u64,
}

impl Default for LoadPolicy {
    fn default() -> Self {
        LoadPolicy {
            highlight_off_bytes: 2 * 1024 * 1024,
            readonly_bytes: 16 * 1024 * 1024,
        }
    }
}

/// How a document was opened, decided by [`Document::load_with_policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    /// Fully interactive: editable and syntax-highlighted.
    Full,
    /// Editable, but syntax highlighting is disabled (large file).
    Plain,
    /// Read-only: the file is too large to edit comfortably.
    ReadOnly,
}

/// A text document held in memory.
///
/// The in-memory [`Document::text`] always uses `\n` line endings; the
/// original style is remembered in [`Document::line_ending`] and restored on
/// save. New (unsaved) documents default to the platform-native style.
#[derive(Debug, Clone)]
pub struct Document {
    text: String,
    encoding: Encoding,
    line_ending: LineEnding,
    pub mode: LoadMode,
    /// Set when the on-disk file is known to be backed by a path.
    path: Option<PathBuf>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            text: String::new(),
            encoding: Encoding::Utf8,
            line_ending: LineEnding::native(),
            mode: LoadMode::Full,
            path: None,
        }
    }
}

impl Document {
    /// Creates an empty, unsaved document with platform-native defaults.
    pub fn new_empty() -> Document {
        Document::default()
    }

    /// Loads a file with the default policy.
    pub fn load(path: &Path) -> Result<Document, DocError> {
        Document::load_with_policy(path, &LoadPolicy::default())
    }

    /// Loads a file, applying the large-file policy. The document remembers
    /// `path` so a plain [`Document::save`] writes back to the same file.
    pub fn load_with_policy(path: &Path, policy: &LoadPolicy) -> Result<Document, DocError> {
        let bytes = read_file(path)?;
        let size = bytes.len() as u64;

        let mode = if size > policy.readonly_bytes {
            LoadMode::ReadOnly
        } else if size > policy.highlight_off_bytes {
            LoadMode::Plain
        } else {
            LoadMode::Full
        };

        let (text, encoding) = decode(&bytes, path)?;
        let line_ending = detect_line_ending(&text).unwrap_or_else(LineEnding::native);
        let text = normalize_newlines(&text);

        Ok(Document {
            text,
            encoding,
            line_ending,
            mode,
            path: Some(path.to_path_buf()),
        })
    }

    /// Sets the whole buffer contents (e.g. replace-all, formatting).
    /// The replacement is normalized to `\n` line endings like any edit.
    pub fn set_text(&mut self, text: &str) {
        self.text = normalize_newlines(text);
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Mutable access to the raw buffer for editor widgets. The text is
    /// always `\n`-normalized; the target line-ending style is restored on
    /// save.
    pub fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// Converts the document to a different line-ending style (status-bar
    /// command). Returns `true` when anything changed.
    pub fn set_line_ending(&mut self, ending: LineEnding) -> bool {
        if self.line_ending == ending {
            return false;
        }
        self.line_ending = ending;
        true
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Associates the document with a path (Save As / New File -> Save).
    pub fn set_path(&mut self, path: Option<PathBuf>) {
        self.path = path;
    }

    /// Number of bytes the buffer would occupy on disk in the current
    /// encoding (approximate: UTF-16 counts 2 bytes per char, surrogate
    /// pairs 4 — good enough for policy checks).
    pub fn encoded_size(&self) -> u64 {
        match self.encoding {
            Encoding::Utf8 => self.text.len() as u64,
            Encoding::Utf8Bom => self.text.len() as u64 + 3,
            Encoding::Utf16Le | Encoding::Utf16Be => {
                2 + self.text.chars().map(|c| c.len_utf16() as u64).sum::<u64>()
            }
        }
    }

    /// Saves to the document's own path.
    pub fn save(&mut self) -> Result<(), DocError> {
        let path = self.path.clone().ok_or_else(|| {
            DocError::Internal("save() called on an untitled document; use save_to()".into())
        })?;
        self.save_to(&path)
    }

    /// Saves to an explicit path (Save As) and remembers it.
    pub fn save_to(&mut self, path: &Path) -> Result<(), DocError> {
        let payload = self.encode_for_disk();
        write_file(path, &payload)?;
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    /// Writes raw binary bytes using the same sibling-temp replacement path
    /// as text documents. This keeps binary preview saves out of text encoding.
    pub fn write_bytes_to(path: &Path, bytes: &[u8]) -> Result<(), DocError> {
        write_file(path, bytes)
    }

    /// Serializes the buffer exactly as it will hit the disk, restoring the
    /// stored BOM and line-ending style.
    fn encode_for_disk(&self) -> Vec<u8> {
        let disk_text = match self.line_ending {
            LineEnding::Lf => self.text.clone(),
            LineEnding::Crlf => self.text.replace('\n', "\r\n"),
        };
        let mut out = Vec::with_capacity(disk_text.len() + 4);
        match self.encoding {
            Encoding::Utf8 => out.extend_from_slice(disk_text.as_bytes()),
            Encoding::Utf8Bom => {
                out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
                out.extend_from_slice(disk_text.as_bytes());
            }
            Encoding::Utf16Le => {
                out.extend_from_slice(&[0xFF, 0xFE]);
                for unit in disk_text.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
            }
            Encoding::Utf16Be => {
                out.extend_from_slice(&[0xFE, 0xFF]);
                for unit in disk_text.encode_utf16() {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
            }
        }
        out
    }
}

/// Reads a file with size checks and mapped IO errors.
fn read_file(path: &Path) -> Result<Vec<u8>, DocError> {
    let meta = fs::metadata(path).map_err(|source| map_read_io(path, source))?;
    if meta.is_dir() {
        return Err(DocError::IsADirectory(path.to_path_buf()));
    }
    if meta.len() > ABSOLUTE_MAX_BYTES {
        return Err(DocError::TooLarge {
            path: path.to_path_buf(),
            bytes: meta.len(),
            limit: ABSOLUTE_MAX_BYTES,
        });
    }
    fs::read(path).map_err(|source| map_read_io(path, source))
}

fn map_read_io(path: &Path, source: io::Error) -> DocError {
    match source.kind() {
        io::ErrorKind::NotFound => DocError::NotFound(path.to_path_buf()),
        io::ErrorKind::PermissionDenied => DocError::PermissionDenied(path.to_path_buf()),
        _ => DocError::ReadFailed {
            path: path.to_path_buf(),
            source,
        },
    }
}

fn write_file(path: &Path, payload: &[u8]) -> Result<(), DocError> {
    // Write via a sibling temp file then rename, so a failed write never
    // truncates the original (best effort on platforms where rename over an
    // existing file is atomic; falls back to a direct write on failure).
    let tmp = path.with_extension(format!(
        "{}~raven~",
        path.extension()
            .map(|ext| format!("{ext}", ext = ext.to_string_lossy()))
            .unwrap_or_default()
    ));
    if fs::write(&tmp, payload).is_ok() {
        if fs::rename(&tmp, path).is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(&tmp);
    }
    fs::write(path, payload).map_err(|source| DocError::WriteFailed {
        path: path.to_path_buf(),
        source,
    })
}

/// Sniffs BOM, then tries UTF-8 strictly, then UTF-16 without BOM is refused.
fn decode(bytes: &[u8], path: &Path) -> Result<(String, Encoding), DocError> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return match std::str::from_utf8(&bytes[3..]) {
            Ok(text) => Ok((text.to_string(), Encoding::Utf8Bom)),
            Err(_) => Err(DocError::UnsupportedEncoding {
                path: path.to_path_buf(),
            }),
        };
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16(&bytes[2..], false, path).map(|text| (text, Encoding::Utf16Le));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16(&bytes[2..], true, path).map(|text| (text, Encoding::Utf16Be));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok((text.to_string(), Encoding::Utf8)),
        Err(_) => Err(DocError::UnsupportedEncoding {
            path: path.to_path_buf(),
        }),
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool, path: &Path) -> Result<String, DocError> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    let mut text = String::with_capacity(units.len());
    for decoded in std::char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(ch) => text.push(ch),
            Err(_) => {
                return Err(DocError::UnsupportedEncoding {
                    path: path.to_path_buf(),
                });
            }
        }
    }
    Ok(text)
}

/// Detects the dominant-looking line-ending style: first end-of-line wins,
/// which matches what users expect for files written by one tool.
fn detect_line_ending(text: &str) -> Option<LineEnding> {
    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch == '\n' {
            if idx > 0 && text.as_bytes()[idx - 1] == b'\r' {
                return Some(LineEnding::Crlf);
            }
            return Some(LineEnding::Lf);
        }
        if ch == '\r' {
            return match chars.peek() {
                Some((_, '\n')) => Some(LineEnding::Crlf),
                _ => Some(LineEnding::Crlf), // lone CR is treated as CRLF family
            };
        }
    }
    None
}

/// Converts `\r\n` and lone `\r` to `\n` for the in-memory form.
fn normalize_newlines(text: &str) -> String {
    if !text.as_bytes().contains(&b'\r') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("raven-editor-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_utf8() {
        let dir = tmp_dir("utf8");
        let path = dir.join("a.txt");
        fs::write(&path, "hello\nworld\n").unwrap();
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.encoding(), Encoding::Utf8);
        assert_eq!(doc.line_ending(), LineEnding::Lf);
        assert_eq!(doc.text(), "hello\nworld\n");
        doc.set_text("hello\nchanged\n");
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello\nchanged\n");
    }

    #[test]
    fn roundtrip_utf8_bom_preserved() {
        let dir = tmp_dir("bom");
        let path = dir.join("b.txt");
        let mut payload = vec![0xEF, 0xBB, 0xBF];
        payload.extend_from_slice("x: 1\r\ny: 2\r\n".as_bytes());
        fs::write(&path, &payload).unwrap();
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.encoding(), Encoding::Utf8Bom);
        assert_eq!(doc.line_ending(), LineEnding::Crlf);
        assert_eq!(doc.text(), "x: 1\ny: 2\n");
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), payload, "BOM + CRLF must survive");
    }

    #[test]
    fn roundtrip_utf16le() {
        let dir = tmp_dir("u16le");
        let path = dir.join("c.txt");
        let mut payload = vec![0xFF, 0xFE];
        for unit in "naïve — text\n".encode_utf16() {
            payload.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&path, &payload).unwrap();
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.encoding(), Encoding::Utf16Le);
        assert_eq!(doc.text(), "naïve — text\n");
        doc.set_text("naïve — edited\n");
        doc.save().unwrap();
        let read_back = fs::read(&path).unwrap();
        assert!(read_back.starts_with(&[0xFF, 0xFE]));
        let decoded = String::from_utf16_lossy(
            &read_back[2..]
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        );
        assert_eq!(decoded, "naïve — edited\n");
    }

    #[test]
    fn roundtrip_utf16be() {
        let dir = tmp_dir("u16be");
        let path = dir.join("d.txt");
        let mut payload = vec![0xFE, 0xFF];
        for unit in "big endian\n".encode_utf16() {
            payload.extend_from_slice(&unit.to_be_bytes());
        }
        fs::write(&path, &payload).unwrap();
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.encoding(), Encoding::Utf16Be);
        assert_eq!(doc.text(), "big endian\n");
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), payload);
    }

    #[test]
    fn unsupported_encoding_is_refused() {
        let dir = tmp_dir("latin1");
        let path = dir.join("e.bin");
        fs::write(&path, [0x42, 0x69, 0xE9, 0x72 /* invalid UTF-8 */]).unwrap();
        let error = Document::load(&path).unwrap_err();
        assert!(matches!(error, DocError::UnsupportedEncoding { .. }));
    }

    #[test]
    fn missing_and_directory_paths_are_structured_errors() {
        let dir = tmp_dir("errors");
        assert!(matches!(
            Document::load(&dir.join("nope.txt")),
            Err(DocError::NotFound(_))
        ));
        assert!(matches!(
            Document::load(&dir),
            Err(DocError::IsADirectory(_))
        ));
    }

    #[test]
    fn large_files_open_read_only_and_plain() {
        let dir = tmp_dir("policy");
        let path = dir.join("big.txt");
        let small = "a\n".repeat(100);
        fs::write(&path, &small).unwrap();
        let policy = LoadPolicy {
            highlight_off_bytes: 10,
            readonly_bytes: 100,
        };
        fs::write(&path, "a\n".repeat(5)).unwrap();
        assert_eq!(
            Document::load_with_policy(&path, &policy).unwrap().mode,
            LoadMode::Full
        );
        fs::write(&path, "a".repeat(50)).unwrap();
        assert_eq!(
            Document::load_with_policy(&path, &policy).unwrap().mode,
            LoadMode::Plain
        );
        fs::write(&path, "a".repeat(200)).unwrap();
        assert_eq!(
            Document::load_with_policy(&path, &policy).unwrap().mode,
            LoadMode::ReadOnly
        );
    }

    #[test]
    fn line_ending_conversion() {
        let mut doc = Document::new_empty();
        // Start from a known style so this assertion also works on Windows,
        // where the native default is already CRLF.
        assert!(doc.set_line_ending(LineEnding::Lf));
        assert!(doc.set_line_ending(LineEnding::Crlf));
        assert!(!doc.set_line_ending(LineEnding::Crlf));
        doc.set_text("a\nb\n");
        doc.save_to(&tmp_dir("eol").join("f.txt")).unwrap();
        // saved via save_to; verify encoded form through encode path
        let payload = doc.encode_for_disk();
        assert_eq!(String::from_utf8(payload).unwrap(), "a\r\nb\r\n");
    }

    #[test]
    fn save_as_updates_path() {
        let dir = tmp_dir("saveas");
        let mut doc = Document::new_empty();
        doc.set_text("content\n");
        let target = dir.join("new.txt");
        doc.save_to(&target).unwrap();
        assert_eq!(doc.path(), Some(target.as_path()));
        let expected = match LineEnding::native() {
            LineEnding::Lf => b"content\n".as_slice(),
            LineEnding::Crlf => b"content\r\n".as_slice(),
        };
        assert_eq!(fs::read(&target).unwrap(), expected);
    }

    #[test]
    fn lone_carriage_returns_are_normalized() {
        let text = "a\rb\rc\n";
        assert_eq!(normalize_newlines(text), "a\nb\nc\n");
        assert_eq!(detect_line_ending(text), Some(LineEnding::Crlf));
    }

    #[test]
    fn untitled_save_is_an_internal_error() {
        let mut doc = Document::new_empty();
        assert!(matches!(doc.save(), Err(DocError::Internal(_))));
    }
}
