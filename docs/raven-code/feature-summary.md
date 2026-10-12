# Raven Code 0.2.0 — Feature Summary

Status legend: **works** (implemented and exercised), **foundation**
(implemented with basic UI, exercised only against the mock server),
**planned** (not in this release).

## File management — works

* Open files (native dialog, path-entry fallback, drag-and-drop onto the
  window, command-line arguments, recent files menu).
* Open folders as workspaces (native dialog, path fallback, recent folders,
  restore last session on launch).
* File explorer with lazy per-directory loading (expanded nodes are scanned
  on demand — never the whole tree).
* Create file / create folder / rename / delete from explorer context menus;
  the rename path reopens affected documents under the new name.
* Delete asks for confirmation. It blocks deletion when a matching open
  document has unsaved edits, and closes clean matching tabs after success.
* Multiple open files as tabs in one or two editor panes (split view,
  `Ctrl+\`); closing prompts before discarding unsaved work; closing the
  window guards every modified buffer.
* Save / Save As / Save All with write-via-temp-file (a failed write never
  truncates the original).
* File-backed documents autosave 900 ms after the last edit. The status bar
  shows the pending countdown, saving/saved state, or unsaved state. Set
  `editor.autosave_delay_ms = 0` to disable autosave; untitled files remain
  unsaved until they have a path.
* Encodings: UTF-8, UTF-8 BOM, UTF-16 LE/BE are detected, edited and written
  back in the same encoding; anything else (Latin-1, Shift-JIS, binary) is
  refused with a clear message instead of being corrupted. CRLF/LF endings
  are detected per file, normalized in memory and restored on save; the
  status bar toggles the style on click.
* Large-file policy: syntax highlighting auto-disables above 2 MiB, files
  above 16 MiB open read-only, everything above 32 MiB is refused (all three
  thresholds configurable).

## Text editing — works

* Syntax highlighting for the full bundled Sublime grammar set (Rust, C/C++,
  C#, Python, JavaScript/TypeScript, Go, Java, Lua, GDScript, TOML, YAML,
  JSON, XML, Markdown and dozens more) through an **incremental per-line
  parse-state cache** — an edit re-parses only from the changed line, and
  only lines the layouter asks for.
* Line-number gutter that stays glued to the text at any scroll position;
  active line highlight; configurable font family/size and zoom.
* Undo/redo (widget-native history), clipboard, selection, word navigation.
* Auto-indent on Enter (continues block indentation after `{`, `:`, …),
  Tab/Shift+Tab indent & dedent over selections with configurable tab width
  and spaces-vs-tabs.
* Word wrap (on by default) toggleable per settings.
* Custom `.sublime-syntax` grammars load from the user syntax folder; see
  [custom-syntaxes.md](custom-syntaxes.md).
* Find & replace in the current file: literal / case-sensitive / whole-word /
  regex modes, match count, next/previous, replace, replace-all — built on
  the same `raven-editor::search` engine the workspace search uses.
* Language registry is data, not code: 39 languages with comment tokens and
  extensions; no per-language behavior is hardcoded in the UI layer.

## Navigation and commands — works

* Command palette (`Ctrl+Shift+P`) with fuzzy matching over every action.
* Quick open (`Ctrl+P`) over a bounded (5 000 entries) background index of
  the workspace that skips `target/`, `.git/`, `node_modules/` etc.
* Find in files: background thread over the index, 1 000-hit cap, NUL-byte
  binary sniffing, results link straight to the line.
* Go to line dialog; recent files/folders menus; next/previous tab.
* Left activity rail switches Explorer and workspace Search. Explorer rows
  use the supplied Symbols file/folder SVGs.
* Every action has a documented default chord and is rebindable via settings.

## Configuration and themes — works

* Versioned (`schema_version = 1`) `raven-code.toml` persisted in the user
  config directory; unknown keys ignored, invalid files fall back to
  defaults **plus a visible notice** — the editor never refuses to start.
* Window geometry, sidebar/bottom visibility, wrap, tab width, font,
  highlighting, thresholds, recents, zoom and keybinds all persist
  (debounced writes).
* Two built-in themes — **Raven Dark** (default, cyan accent) and
  **Raven Light** — covering every surface plus a matching syntect theme so
  highlighting is brand-coherent in both modes. Unknown theme ids fall back
  to dark.
* Optional user font (`[appearance] font_path`) and best-effort system CJK
  fallback font loading.

## Raven Pico SDK integration — works

* Build / Test / Run / Clean on the opened workspace go through the
  **raven-projects + raven-build** crates: marker detection, adapter
  selection, the hardened process layer (no shell, 10 MiB/stream capture
  cap, structured launch errors) — the exact code paths `raven-pico` CLI
  uses, exercised as a library as the integration docs intended.
* Output panel streams progress events (`ProgressSink`) and captured
  stdout/stderr line-by-line; running jobs show a status chip and can be
  cancelled through the SDK's `CancelToken`.
* Run Command… executes an arbitrary program + args (no shell) in the
  workspace root through the same process layer.
* Open External Terminal launches a real terminal emulator at the workspace
  root (platform defaults or a configured override) — an honest alternative
  to a fake embedded terminal.
* Status bar shows the detected project kind (e.g. "Rust (Cargo)").

## Language intelligence — foundation

* `crates/raven-lsp`: JSON-RPC 2.0 over stdio with `Content-Length` framing,
  reader/writer threads, request-id correlation, `$/cancelRequest`,
  protocol-conformant shutdown, and defensive handling of server→client
  requests. Covered by an end-to-end test against a mock server that
  exercises initialize, didOpen → publishDiagnostics, completion, hover,
  definition, document symbols and shutdown.
* Servers are configured per language in settings, spawned lazily on first
  open of a matching file, never at launch, never downloaded.
* Diagnostics panel (click to jump), status-bar error/warning counters.
* Completion popup (`Ctrl+Space`, arrows/Enter to accept), hover window
  (`Alt+H`), go-to-definition (`F12`, cross-file), document symbols tab,
  formatting (`Shift+Alt+F`) applied as LSP text edits.
* Full-text document sync (`didOpen`/`didChange` versioned, `didSave`).

## Branding — works

* The official crow SVG is embedded unchanged in `apps/raven-code/assets/`
  and rendered for the window/taskbar icon (PNG set + Windows ICO via
  winresource) and the in-app About/welcome screens.
* Cyan product accent per the ecosystem brief; dark theme default.

## Not in this release (planned)

* Terminal emulator panel (external terminal provided instead).
* Per-pane tab-close commands beyond the single-pane focus model, drag-tab
  reordering, tab pinning.
* Mouse-hover tooltips, multi-cursor editing, snippets.
* Rename-symbol / code actions / semantic tokens.
* Extension marketplace/installer surface beyond the settings-based LSP and
  command configuration (the seams exist; see limitations.md).
