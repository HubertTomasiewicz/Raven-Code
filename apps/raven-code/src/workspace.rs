//! Workspace state: the folder tree for the explorer and the bounded file
//! index that powers Quick Open.
//!
//! Performance rules honored here (brief §9):
//!
//! * the explorer reads directories **lazily** — only expanded nodes are
//!   scanned, never the whole workspace,
//! * the Quick Open index is a **bounded** background walk (entry cap,
//!   junk-directory filter, no symlink following) started on folder open
//!   and manual refresh only — never re-scanned per frame or per keystroke,
//! * results are delivered through a channel; the UI polls, never blocks.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// Directories that are almost always generated output; skipped by the
/// Quick Open index (the explorer still shows them when expanded).
const INDEX_SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "build",
    "dist",
    ".venv",
    "venv",
    "__pycache__",
    ".idea",
    ".gradle",
    ".godot",
    "vendor",
];

/// Hard cap so a huge checkout cannot exhaust memory.
const INDEX_ENTRY_CAP: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexStatus {
    Idle,
    Scanning,
    Done,
}

/// Cached view of one directory (the explorer's lazy unit of work).
#[derive(Debug, Clone)]
pub struct DirListing {
    pub dirs: Vec<(String, PathBuf)>,
    pub files: Vec<(String, PathBuf)>,
}

impl DirListing {
    pub fn read(path: &Path) -> DirListing {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                if is_dir {
                    dirs.push((name, entry.path()));
                } else {
                    files.push((name, entry.path()));
                }
            }
        }
        // Locale-independent, deterministic: byte sort, dirs first.
        dirs.sort_by_key(|(name, _)| name.to_lowercase());
        files.sort_by_key(|(name, _)| name.to_lowercase());
        DirListing { dirs, files }
    }
}

/// Shared state of the Quick Open background index.
pub struct FileIndex {
    status: Arc<Mutex<IndexStatus>>,
    files: Arc<Mutex<Vec<PathBuf>>>,
    receiver: Mutex<Option<Receiver<Vec<PathBuf>>>>,
}

impl Default for FileIndex {
    fn default() -> Self {
        FileIndex {
            status: Arc::new(Mutex::new(IndexStatus::Idle)),
            files: Arc::new(Mutex::new(Vec::new())),
            receiver: Mutex::new(None),
        }
    }
}

impl FileIndex {
    pub fn status(&self) -> IndexStatus {
        *self.status.lock().unwrap()
    }

    /// Snapshot of the indexed files (used by find-in-files).
    pub fn files(&self) -> Vec<PathBuf> {
        self.files.lock().unwrap().clone()
    }

    /// Starts a background scan of `root`. Any previous scan result is
    /// replaced when the new one completes.
    pub fn scan(&self, root: PathBuf) {
        {
            let mut status = self.status.lock().unwrap();
            if *status == IndexStatus::Scanning {
                return; // one scan at a time; refresh can be pressed again
            }
            *status = IndexStatus::Scanning;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.receiver.lock().unwrap() = Some(receiver);
        let status = self.status.clone();
        let files = self.files.clone();

        std::thread::Builder::new()
            .name("raven-code-index".into())
            .spawn(move || {
                let found = walk(root);
                let _ = sender.send(found);
            })
            .expect("spawn index thread");

        let _ = (status, files); // consumed via poll(); keeps Arcs alive for clarity
    }

    /// Non-blocking poll from the UI: applies finished scan results.
    pub fn poll(&self) -> bool {
        let maybe = self.receiver.lock().unwrap().take();
        if let Some(receiver) = maybe {
            match receiver.try_recv() {
                Ok(files) => {
                    *self.files.lock().unwrap() = files;
                    *self.status.lock().unwrap() = IndexStatus::Done;
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // put it back, scan still running
                    *self.receiver.lock().unwrap() = Some(receiver);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    *self.status.lock().unwrap() = IndexStatus::Done;
                }
            }
        }
        false
    }

    /// Fuzzy-ish match: case-insensitive subsequence scoring over file
    /// names, with the path as tiebreaker. Returns (score, path) pairs.
    pub fn search(&self, query: &str, limit: usize) -> Vec<(i64, PathBuf)> {
        let files = self.files.lock().unwrap();
        let needle = query.to_lowercase();
        let mut scored: Vec<(i64, PathBuf)> = files
            .iter()
            .filter_map(|path| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let score = fuzzy_score(&name.to_lowercase(), &needle)?;
                // Prefer shallower paths slightly.
                let depth = path.components().count() as i64;
                Some((score * 10 - depth, path.clone()))
            })
            .collect();
        scored.sort_by_key(|(score, _)| -*score);
        scored.truncate(limit);
        scored
    }
}

/// Bounded, non-recursive-priority walk with junk filtering.
fn walk(root: PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if out.len() >= INDEX_ENTRY_CAP {
                return out;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_dir() {
                if name.starts_with('.') || INDEX_SKIP_DIRS.contains(&name.as_ref()) {
                    continue;
                }
                stack.push(entry.path());
            } else if file_type.is_file() {
                if name.starts_with('.') {
                    continue;
                }
                out.push(entry.path());
            }
        }
    }
    out
}

/// Case-insensitive subsequence score; `None` = no match. Higher is better.
pub fn fuzzy_score(haystack: &str, needle: &str) -> Option<i64> {
    if needle.is_empty() {
        return Some(0);
    }
    let mut score = 0i64;
    let mut search_from = 0usize;
    let mut consecutive = 0i64;
    for (needle_index, nchar) in needle.chars().enumerate() {
        let position = haystack[search_from..].find(nchar)?;
        let absolute = search_from + position;
        score += if position == 0 && needle_index > 0 {
            consecutive += 2;
            consecutive
        } else {
            consecutive = 0;
            1
        };
        if position == 0 && needle_index == 0 {
            score += 3; // prefix bonus
        }
        search_from = absolute + nchar.len_utf8();
    }
    Some(score)
}

/// The workspace: current root plus per-directory cached listings.
pub struct Workspace {
    pub root: PathBuf,
    listings: std::collections::HashMap<PathBuf, DirListing>,
}

impl Workspace {
    pub fn open(root: PathBuf) -> Workspace {
        let mut listings = std::collections::HashMap::new();
        listings.insert(root.clone(), DirListing::read(&root));
        Workspace { root, listings }
    }

    pub fn listing(&mut self, path: &Path) -> &DirListing {
        self.listings
            .entry(path.to_path_buf())
            .or_insert_with(|| DirListing::read(path))
    }

    /// Forces a refresh of one directory (F5 / refresh button).
    pub fn refresh(&mut self, path: &Path) {
        self.listings
            .insert(path.to_path_buf(), DirListing::read(path));
    }

    pub fn refresh_all(&mut self) {
        let paths: Vec<PathBuf> = self.listings.keys().cloned().collect();
        for path in paths {
            self.listings.insert(path.clone(), DirListing::read(&path));
        }
    }

    /// Human name for the status bar / title.
    pub fn display_name(&self) -> String {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_scores_match_prefix_and_sequence() {
        assert!(fuzzy_score("main.rs", "main").is_some());
        assert!(fuzzy_score("main.rs", "mnrs").is_some());
        assert!(fuzzy_score("main.rs", "xyz").is_none());
        assert!(fuzzy_score("main.rs", "m") > Some(0));
    }

    #[test]
    fn listing_sorts_dirs_first() {
        let dir = std::env::temp_dir().join(format!("raven-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("zdir")).unwrap();
        std::fs::create_dir_all(dir.join("adir")).unwrap();
        std::fs::write(dir.join("b.txt"), "x").unwrap();
        std::fs::write(dir.join("a.txt"), "x").unwrap();

        let listing = DirListing::read(&dir);
        assert_eq!(listing.dirs[0].0, "adir");
        assert_eq!(listing.files[0].0, "a.txt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_walk_is_bounded_and_skips_junk() {
        let dir = std::env::temp_dir().join(format!("raven-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("target/junk"), "x").unwrap();
        std::fs::write(dir.join(".git/config"), "x").unwrap();

        let files = walk(dir.clone());
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
