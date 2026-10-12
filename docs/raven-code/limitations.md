# Raven Code 0.2.0 — Known Limitations

Honest accounting of what this release does not do, cannot do, or has not
been verified to do in the build environment.

## Verified environment vs. targets

* The verified build and launch environment is **Linux x86_64 (Debian,
  Xvfb + Mesa software GL)**. Screenshots in `dist/` come from those
  sessions. The code is cross-platform by construction (egui/eframe,
  platform-isolated modules), and Windows is the ecosystem's primary
  target, but **no Windows executable was produced here** — see
  `apps/raven-code/README.md` for the exact build command.
* Running the GUI on a minimal Linux box may need `libxkbcommon-x11` and the
  usual X11 client libraries installed (they were fetched locally for the
  verification session). A normal desktop distribution has them.

## Editing

* Undo/redo uses egui's widget history. Programmatic whole-buffer
  replacements (Replace All, LSP formatting, completion inserts) are *not*
  separate undo units — they appear as one step or may reset the local
  history for that buffer.
* With word wrap **off**, the gutter is a sibling of the text inside the
  scroll area; a very long line scrolls the text horizontally past the
  gutter (VSCode-style pinned gutters need a custom widget surface).
* The current-line highlight tints the text (alpha overlay) rather than
  painting behind it.
* Files are loaded fully into memory (strings); the 2/16/32 MiB policy
  thresholds keep the UI responsive, but there is no virtual/paged buffer
  for arbitrarily large logs.
* Drag-and-drop opens files/folders; dragging text *into* the editor relies
  on the platform's IME path and was not verified.

## Language intelligence

* All LSP features were verified against a **mock server**, not against
  rust-analyzer/pyright (none installable in the build environment). The
  client speaks standard JSON-RPC/LSP and follows the spec for the methods
  it uses, but real servers may surface edge cases (incremental sync
  requests, server-initiated `workspace/configuration`, progress
  notifications) that the client currently answers with `null` or ignores.
* Completion supports plain `insertText`/label insertion; `textEdit`-based
  snippets and `additionalTextEdits` are not applied.
* Diagnostics render as a list; no squiggly underlines in the editor yet.
* Only full-document sync is negotiated. Servers that demand incremental
  sync still work (we send full text, which the protocol allows), but very
  large files re-send entirely per change.
* Formatting is applied only when the server returns a well-formed edit
  list; overlapping/malformed edits are skipped defensively.

## Integration

* Build/Test/Run use the workspace folder's detected project; there is no
  per-file "run this script" affordance yet, and no MSBuild/vswhere
  discovery was verifiable off-Windows (the raven-build adapters handle it
  on Windows by design).
* The bottom-panel Output view is captured, not interactive: no stdin, no
  PTY. Interactive programs should use **Run: Open External Terminal**.
* No file-watcher: the explorer and Quick Open index refresh on demand
  (F5 / reopening), not on external changes.

## Packaging

* A single Linux binary ships in `dist/`; no installer, no self-update, no
  icon-theme bundling beyond the embedded assets.
* The Windows `.ico` and winresource embedding are wired in `build.rs` but
  were compile-verified only for the non-Windows no-op path.
