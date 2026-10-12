//! # raven-projects
//!
//! Project discovery, root detection and project-type identification for
//! Raven Pico.
//!
//! Detection is **marker based**: a project kind is identified by well-known
//! files in a directory (`Cargo.toml`, `CMakeLists.txt`, `project.godot`,
//! ...). A project file and a build system are not the same thing — detection
//! reports the ecosystem, and the adapters in `raven-build` decide which
//! installed tool actually drives it.
//!
//! Guarantees:
//!
//! * Only the requested directory is inspected by default; ancestors are
//!   inspected only when [`DetectOptions::search_parents`] is enabled
//!   (bounded — never a recursive directory scan).
//! * A missing or invalid path is a path error, never a detection result.
//! * All candidates are reported; a primary is chosen only when the ranking
//!   is unambiguous.
//! * A malformed `raven-pico.toml` is reported gracefully, never crashes.
//! * Unknown projects produce a clear error with instructions.

use std::path::{Path, PathBuf};

use raven_core::config::{ConfigStatus, LoadedConfig, PicoConfig, read_config};
use raven_core::error::PicoError;
use raven_core::kind::{Confidence, ProjectKind};
use raven_core::path_to_string;
use raven_core::report::{CandidateDto, ConfigDto, DetectPayload};

/// Marker-based detection rule for one project kind.
struct MarkerRule {
    kind: ProjectKind,
    /// Exact file names to match.
    files: &'static [&'static str],
    /// File extensions to match (without dot), e.g. `"sln"`.
    extensions: &'static [&'static str],
    /// Suffixes to match on the full file name, e.g. `".slnx"`.
    suffixes: &'static [&'static str],
    /// Ranking: higher wins when picking a primary candidate.
    rank: u8,
    confidence: Confidence,
}

const MARKER_RULES: &[MarkerRule] = &[
    MarkerRule {
        kind: ProjectKind::Rust,
        files: &["Cargo.toml"],
        extensions: &[],
        suffixes: &[],
        rank: 90,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::VisualStudio,
        files: &[],
        extensions: &["sln", "vcxproj"],
        suffixes: &[".slnx"],
        rank: 85,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::DotNet,
        files: &[],
        extensions: &["csproj", "fsproj", "vbproj"],
        suffixes: &[],
        rank: 80,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::CMake,
        files: &["CMakeLists.txt"],
        extensions: &[],
        suffixes: &[],
        rank: 78,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::Maven,
        files: &["pom.xml"],
        extensions: &[],
        suffixes: &[],
        rank: 70,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::Gradle,
        files: &[
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
        extensions: &[],
        suffixes: &[],
        rank: 70,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::Godot,
        files: &["project.godot"],
        extensions: &[],
        suffixes: &[],
        rank: 65,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::Python,
        files: &["pyproject.toml", "setup.py", "requirements.txt"],
        extensions: &[],
        suffixes: &[],
        rank: 60,
        confidence: Confidence::High,
    },
    MarkerRule {
        kind: ProjectKind::Lua,
        files: &["main.lua", "conf.lua"],
        extensions: &["rockspec"],
        suffixes: &[],
        rank: 50,
        confidence: Confidence::Medium,
    },
];

/// One detected project candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: ProjectKind,
    pub confidence: Confidence,
    pub rank: u8,
    /// Marker files that matched, sorted.
    pub markers: Vec<String>,
    /// Directory that contains the markers.
    pub root: PathBuf,
}

/// Options controlling detection.
///
/// The default is deliberately conservative: **only the supplied directory
/// is inspected**. Ancestor search is an explicit opt-in (`search_parents`,
/// CLI `--search-parents`) so Pico never silently reports a project the
/// user did not point at.
#[derive(Debug, Clone)]
pub struct DetectOptions {
    /// When the start directory has no markers and no `raven-pico.toml`,
    /// walk up the ancestor chain (bounded) looking for a project root.
    /// Defaults to `false` — parent discovery is opt-in.
    pub search_parents: bool,
    /// Maximum number of ancestor directories to inspect. Defaults to 32.
    pub max_upwards: usize,
}

impl Default for DetectOptions {
    fn default() -> Self {
        DetectOptions {
            search_parents: false,
            max_upwards: 32,
        }
    }
}

/// Result of detection.
#[derive(Debug, Clone)]
pub struct Detection {
    /// The directory detection started from.
    pub searched: PathBuf,
    /// The selected project root (the start directory or an ancestor).
    pub root: PathBuf,
    /// All candidates found at the root, ranked best-first.
    pub candidates: Vec<Candidate>,
    /// The unambiguous primary candidate, if the ranking is unambiguous or a
    /// configuration / CLI override applies.
    pub primary: Option<Candidate>,
    /// True when two or more candidates tie for the top rank.
    pub ambiguous: bool,
    /// The optional `raven-pico.toml` found at the root.
    pub config: Option<LoadedConfig>,
    /// True when the root is an ancestor of the start directory.
    pub searched_upwards: bool,
}

impl Detection {
    /// Applies an explicit kind override, marking it as the primary.
    ///
    /// Used for `--kind` on the CLI and `[project] kind` in the config.
    pub fn with_forced_kind(mut self, kind: ProjectKind) -> Detection {
        let root = self.root.clone();
        let forced = Candidate {
            kind,
            confidence: Confidence::Explicit,
            rank: 100,
            markers: Vec::new(),
            root,
        };
        // Keep the detected candidate of the same kind (with its markers) but
        // make sure the primary is the forced one.
        if let Some(existing) = self
            .candidates
            .iter_mut()
            .find(|candidate| candidate.kind == kind)
        {
            existing.confidence = Confidence::Explicit;
            existing.rank = 100;
        } else {
            self.candidates.insert(0, forced.clone());
        }
        self.primary = self.candidates.iter().find(|c| c.kind == kind).cloned();
        self.ambiguous = false;
        self
    }

    /// Converts the detection into the versioned JSON payload.
    pub fn to_payload(&self) -> DetectPayload {
        let config = self.config.as_ref().map(|loaded| ConfigDto {
            path: path_to_string(&loaded.path),
            status: loaded.status.as_str().to_string(),
            message: match &loaded.status {
                ConfigStatus::Ok => None,
                ConfigStatus::ParseError { message } => Some(message.clone()),
                ConfigStatus::UnsupportedVersion { found } => Some(format!(
                    "schema_version {found} is newer than this build supports"
                )),
            },
            schema_version: loaded.config.as_ref().map(|config| config.schema_version),
            kind_override: loaded
                .config
                .as_ref()
                .and_then(|config| config.kind_override())
                .map(|kind| kind.as_str().to_string()),
            defined_sections: loaded
                .config
                .as_ref()
                .map(PicoConfig::defined_sections)
                .unwrap_or_default(),
        });

        let candidate_dto = |candidate: &Candidate| CandidateDto {
            kind: candidate.kind.as_str().to_string(),
            display: candidate.kind.display_name().to_string(),
            confidence: candidate.confidence.as_str().to_string(),
            markers: candidate.markers.clone(),
            root: path_to_string(&candidate.root),
        };

        DetectPayload {
            searched: path_to_string(&self.searched),
            root: Some(path_to_string(&self.root)),
            primary: self.primary.as_ref().map(candidate_dto),
            candidates: self.candidates.iter().map(candidate_dto).collect(),
            ambiguous: self.ambiguous,
            config,
            searched_upwards: self.searched_upwards,
        }
    }
}

/// Validates a user-provided path and returns it as an absolute directory
/// path.
///
/// * A missing path is [`PicoError::PathNotFound`] — a usage error, never a
///   "not detected" result.
/// * A file path is [`PicoError::NotADirectory`].
/// * The returned path is canonicalized (so parent traversal and reports are
///   well-defined) and then cleaned of Windows verbatim `\\?\` prefixes
///   where that cannot change meaning (see `raven_core::paths`).
///
/// Public because the CLI and future SDK clients need the same validation
/// before writing files (e.g. `init`).
pub fn validate_directory(path: &Path) -> Result<PathBuf, PicoError> {
    let metadata = std::fs::metadata(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => PicoError::PathNotFound {
            path: path.to_path_buf(),
        },
        _ => PicoError::Io(err),
    })?;
    if !metadata.is_dir() {
        return Err(PicoError::NotADirectory {
            path: path.to_path_buf(),
        });
    }
    // Canonicalize so parent traversal is well-defined for relative inputs
    // (the parent of "examples" is "" and would fail on read_dir) and so
    // reports contain absolute paths; then drop the verbatim prefix when it
    // is safe to do so (purely cosmetic — semantics are preserved).
    let canonical = std::fs::canonicalize(path).map_err(PicoError::Io)?;
    Ok(raven_core::clean_verbatim(&canonical))
}

/// Scans one directory (single level, no recursion) and returns candidates.
fn scan_dir(dir: &Path) -> std::io::Result<Vec<Candidate>> {
    let mut file_names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            file_names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    file_names.sort();

    let mut candidates: Vec<Candidate> = Vec::new();
    for rule in MARKER_RULES {
        let mut markers: Vec<String> = Vec::new();
        for name in &file_names {
            let lower = name.to_ascii_lowercase();
            let matched = rule.files.iter().any(|f| lower == f.to_ascii_lowercase())
                || rule
                    .extensions
                    .iter()
                    .any(|ext| Path::new(&lower).extension().is_some_and(|e| e == *ext))
                || rule
                    .suffixes
                    .iter()
                    .any(|suffix| lower.ends_with(&suffix.to_ascii_lowercase()));
            if matched {
                markers.push(name.clone());
            }
        }
        if markers.is_empty() {
            continue;
        }
        candidates.push(Candidate {
            kind: rule.kind,
            confidence: rule.confidence,
            rank: rule.rank,
            markers,
            root: dir.to_path_buf(),
        });
    }
    // Best rank first; ties broken by kind name for determinism.
    candidates.sort_by(|a, b| {
        b.rank
            .cmp(&a.rank)
            .then_with(|| a.kind.as_str().cmp(b.kind.as_str()))
    });
    Ok(candidates)
}

/// Looks for `raven-pico.toml` in `dir`, mapping errors to `None`-style
/// statuses so detection stays graceful.
fn load_config_graceful(dir: &Path) -> Option<LoadedConfig> {
    match read_config(dir) {
        Ok(loaded) => loaded,
        Err(err) => Some(LoadedConfig {
            path: dir.join(raven_core::config::CONFIG_FILE_NAME),
            config: None,
            status: ConfigStatus::ParseError {
                message: format!(
                    "could not read {}: {err}",
                    raven_core::config::CONFIG_FILE_NAME
                ),
            },
        }),
    }
}

/// Detects the project at `start`.
///
/// # Discovery rules (deliberately explicit)
///
/// 1. `start` must exist: a missing path is [`PicoError::PathNotFound`] and
///    a file is [`PicoError::NotADirectory`] — both usage errors (exit 2),
///    never reported as "no project detected".
/// 2. If `start` contains project markers or a `raven-pico.toml`, `start`
///    is the project root. It always wins over any ancestor.
/// 3. Otherwise detection fails with [`PicoError::ProjectNotDetected`].
///    Ancestors are inspected **only** when
///    [`DetectOptions::search_parents`] is `true` (CLI: `--search-parents`);
///    the default is `false`, so a markerless directory never silently
///    reports an ancestor project.
/// 4. Parent search is bounded by [`DetectOptions::max_upwards`] and stops
///    at the first directory with markers or a configuration file; only
///    single-level listings are read, never a recursive scan.
///
/// # Errors
///
/// * [`PicoError::PathNotFound`] — `start` does not exist.
/// * [`PicoError::NotADirectory`] — `start` is a file.
/// * [`PicoError::ProjectNotDetected`] — no markers in `start` (or in its
///   ancestors when parent search is enabled).
pub fn detect(start: &Path, options: &DetectOptions) -> Result<Detection, PicoError> {
    let start_dir = validate_directory(start)?;

    let mut root = start_dir.clone();
    let mut candidates = scan_dir(&root)?;
    let mut searched_upwards = false;

    let mut config = load_config_graceful(&root);
    let has_config_file = config.is_some();

    // Root heuristics: the start directory wins if it has markers or a
    // raven-pico.toml; otherwise walk up looking for either.
    if candidates.is_empty() && !has_config_file && options.search_parents {
        let mut current = start_dir.as_path();
        for _ in 0..options.max_upwards {
            let Some(parent) = current.parent() else {
                break;
            };
            if parent == current {
                break; // reached the filesystem root
            }
            let parent_candidates = scan_dir(parent)?;
            let parent_config = load_config_graceful(parent);
            let found = !parent_candidates.is_empty() || parent_config.is_some();
            current = parent;
            if found {
                root = parent.to_path_buf();
                candidates = parent_candidates;
                config = parent_config;
                searched_upwards = true;
                break;
            }
        }
    }

    // Configured commands without an explicit kind make the project "custom":
    // the raven-pico.toml is the source of truth.
    if let Some(loaded) = &config {
        if let Some(parsed) = &loaded.config {
            if parsed.kind_override().is_none() && !parsed.operations().is_empty() {
                candidates.insert(
                    0,
                    Candidate {
                        kind: ProjectKind::Custom,
                        confidence: Confidence::Explicit,
                        rank: 100,
                        markers: vec![raven_core::config::CONFIG_FILE_NAME.to_string()],
                        root: root.clone(),
                    },
                );
            }
        }
    }

    if candidates.is_empty() && config.is_none() {
        return Err(PicoError::ProjectNotDetected { path: start_dir });
    }

    let detection = assemble_detection(start_dir, root, candidates, config, searched_upwards);
    Ok(detection)
}

/// Picks the primary candidate: forced kinds win, then the top rank. A tie at
/// the top rank makes the detection ambiguous (no primary).
fn assemble_detection(
    searched: PathBuf,
    root: PathBuf,
    mut candidates: Vec<Candidate>,
    config: Option<LoadedConfig>,
    searched_upwards: bool,
) -> Detection {
    // A valid configuration kind override forces the primary.
    if let Some(loaded) = &config {
        if let Some(parsed) = &loaded.config {
            if let Some(kind) = parsed.kind_override() {
                if let Some(existing) = candidates.iter_mut().find(|c| c.kind == kind) {
                    existing.confidence = Confidence::Explicit;
                    existing.rank = 100;
                } else {
                    candidates.insert(
                        0,
                        Candidate {
                            kind,
                            confidence: Confidence::Explicit,
                            rank: 100,
                            markers: vec![raven_core::config::CONFIG_FILE_NAME.to_string()],
                            root: root.clone(),
                        },
                    );
                }
            }
        }
    }

    let top_rank = candidates.first().map(|c| c.rank);
    let ambiguous = match top_rank {
        Some(top) => candidates.iter().filter(|c| c.rank == top).count() > 1,
        None => false,
    };
    let primary = if ambiguous {
        None
    } else {
        candidates.first().cloned()
    };

    Detection {
        searched,
        root,
        candidates,
        primary,
        ambiguous,
        config,
        searched_upwards,
    }
}

/// Selects the project kind and root for an operation (build/test/run/clean).
///
/// Precedence: `--kind` override, then `[project] kind` from the
/// configuration, then the unambiguous primary candidate. Ambiguous
/// detections and broken configurations are hard errors here.
pub fn select(
    detection: &Detection,
    kind_override: Option<ProjectKind>,
) -> Result<(ProjectKind, PathBuf), PicoError> {
    // A broken configuration is a hard stop: the user clearly intended
    // overrides, so silently ignoring them would be dangerous.
    if let Some(loaded) = &detection.config {
        if loaded.status != ConfigStatus::Ok {
            return Err(loaded.require().unwrap_err());
        }
    }

    if let Some(kind) = kind_override {
        return Ok((kind, detection.root.clone()));
    }

    if let Some(loaded) = &detection.config {
        if let Some(parsed) = &loaded.config {
            if let Some(kind) = parsed.kind_override() {
                return Ok((kind, detection.root.clone()));
            }
        }
    }

    if let Some(primary) = &detection.primary {
        return Ok((primary.kind, detection.root.clone()));
    }

    if detection.ambiguous {
        let kinds = detection
            .candidates
            .iter()
            .map(|c| c.kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(PicoError::AmbiguousProject {
            path: detection.root.clone(),
            kinds,
        });
    }

    Err(PicoError::ProjectNotDetected {
        path: detection.searched.clone(),
    })
}

#[cfg(test)]
mod tests;
