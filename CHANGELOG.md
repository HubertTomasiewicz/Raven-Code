# Changelog

All notable changes to Raven Pico are documented here.

## Unreleased — Raven Code 0.2.0

New application in the workspace (no changes to Pico's own behavior):

* **Raven Code** (`apps/raven-code`): the lightweight code editor of the
  Raven ecosystem — tabs, file explorer, command palette, quick open,
  find & replace, find-in-files, split editor, status bar, bottom panel
  (output / search / diagnostics / symbols), Raven Dark & Light themes,
  settings persistence, drag-and-drop, and Raven Pico SDK integration for
  build/test/run through the shared process layer.
* **raven-editor** (`crates/raven-editor`): UI-free editor core — documents
  with UTF-8/UTF-16 + BOM + CRLF round-tripping, large-file policy, literal
  and regex search, language registry, versioned settings, theme palettes.
* **raven-lsp** (`crates/raven-lsp`): JSON-RPC/LSP client foundation with an
  end-to-end mock-server test (initialize, diagnostics, completion, hover,
  definition, document symbols, shutdown).
* Workspace members and shared deps extended accordingly; all existing Pico
  tests, formats and clippy gates unchanged and green.

## 0.2.0

Maintenance release driven by Windows real-world testing: explicit project
discovery boundaries, honest toolchain diagnostics, and clearer CMake
failure reporting. All 0.1.0 Cargo workflows (build / run / test,
incremental builds, broken-project diagnostics) are preserved.

### Changed — project discovery boundaries (breaking)

* Detection now inspects **only the supplied directory by default**. A
  directory without project markers is reported as "not detected" (exit 3)
  instead of silently resolving to a project found in an ancestor directory.
* Parent-directory discovery is an explicit opt-in: the new global
  `--search-parents` flag (CLI) / `DetectOptions::search_parents = true`
  (library). The walk stays bounded (32 levels), stops at the first
  directory containing markers or a `raven-pico.toml`, and is reported via
  `"searched_upwards": true`.
* `--no-search-upwards` (0.1.0) is removed — superseded by the new default
  and the opt-in flag.
* A missing path is now unambiguously a usage error (`invalid_path`, exit 2,
  "path does not exist: …") and is never rendered as "no project detected";
  the human-readable `detect` output no longer prints the "no project" line
  for path errors.
* The `not_detected` message and hint state the boundary explicitly
  ("no recognizable project found in …", with a pointer to
  `--search-parents`).

### Changed — Windows path display (Fix)

* All human-readable and JSON path output goes through one serialization
  rule set (`raven_core::paths`): Windows verbatim `\\?\` prefixes are
  removed from ordinary paths (`\\?\C:\dir` → `C:\dir`,
  `\\?\UNC\server\share` → `\\server\share`), so PowerShell users no longer
  see `\\?\` noise.
* The prefix is **kept** when stripping would change path semantics:
  components with trailing dots/spaces, reserved DOS device names
  (`CON`, `NUL`, `COM1`…, also as `CON.txt`), `.`/`..` components (verbatim
  paths skip normalization), device/volume paths, and cleaned results longer
  than 240 characters (legacy MAX_PATH safety). Unicode and spaces are
  unaffected. Covered by cross-platform unit tests.

### Changed — toolchain diagnostics (Fix)

* A doctor/toolchains probe only counts when the tool **exits 0** with a
  parseable version — broken installations and Windows Store stubs are no
  longer reported as installed.
* Windows-only tools report `n/a` on other platforms instead of a misleading
  `ok`.
* MSBuild found via vswhere is now version-probed (`-version -nologo`) and
  reported with its version and discovery detail.
* Missing tools list the project kinds that need them
  (`required for: C/C++ (CMake) projects`) in addition to installation hints.
* `doctor.healthy` now means what the schema always promised: no check
  errored **and** at least one toolchain is usable (a machine with no usable
  toolchain reports "attention needed"; doctor still exits 0).
* `toolchains` prints a `found`/`missing` summary in text mode.

### Changed — CMake behaviour (Fix)

* Detection keeps working without CMake installed. When the tool is absent,
  build/test fail with `tool_missing` (exit 4) and a hint that says exactly
  what happened: the project **was detected** as C/C++ (CMake), but the
  build cannot proceed because its required tool is unavailable — plus
  installation and `[build]`/`[test]` configuration guidance.
* `run` on a CMake project without a `[run]` section is refused with exit 9
  and explicit guidance; Pico does not invent an executable path. With a
  configured `[run] command` the configured program executes as usual.

### Added

* `raven_core::paths` — the single path display/serialization module
  (`clean_verbatim`, `display_path`), used by `path_to_string` everywhere
  (reports, tool paths, doctor, process layer).
* 36 new automated tests: discovery-boundary regression tests (unit,
  cross-crate integration, CLI end-to-end), Windows path rule tests,
  probe-accuracy tests, CMake command-construction tests
  (release flags, custom build dirs, ctest invocation, native clean,
  missing-tool hint, no-invented-run-path) — 137 tests total.
* `CHANGELOG.md` (this file).

### Compatibility

* JSON schema stays at version 1; all changes are within the documented
  envelope (statuses gain `n/a`, which consumers must tolerate per the
  stability rules).
* Exit codes are unchanged. `--no-search-upwards` users: remove the flag
  (its behaviour is now the default) and use `--search-parents` where
  ancestor discovery is wanted.
