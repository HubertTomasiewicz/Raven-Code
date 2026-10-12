# Raven Code 0.2.0 — Build and Verification Results

Recorded from the actual delivery session (Linux x86_64, Debian, Rust
stable 1.99.0, 2026-10-11). No figures here are invented; commands and
representative outputs are quoted so they can be re-run.

## Commands (workspace root)

```console
cargo fmt --all -- --check                      # PASS
cargo clippy --workspace --all-targets -- -D warnings   # PASS (zero warnings)
cargo test --workspace                          # PASS
cargo build --workspace --release               # PASS
```

Release build time for the full workspace: ~6 minutes cold, ~12 s warm for
`raven-code` alone. Binary size: `raven-code` ≈ 25 MiB (unstripped release,
includes embedded syntax grammars and brand assets), `raven-pico` ≈ 2 MiB.

## Test results

`cargo test --workspace` — all suites green, **183 test cases** total:

| Suite | Count | Notes |
|---|---|---|
| raven-core / raven-projects / raven-build units | 52 | pre-existing Pico tests, untouched |
| raven-pico integration tests (`tests/`) | 29 | pre-existing, untouched |
| raven-api examples | 25 | pre-existing, untouched |
| **raven-editor** units | 30 | new: encoding round-trips, search, settings, themes, languages |
| **raven-lsp** units | 5 | new: framing, URI codec, response normalization |
| **raven-lsp mock-server e2e** | 1 | new: full client lifecycle against a real child-process mock LSP server |
| **raven-code** units | 11 | new: chord parsing, incremental highlighter, workspace index, actions |

The Pico test suite passes unmodified — Raven Code is purely additive to
the existing crates.

## Runtime verification (GUI)

Launched under `Xvfb :99` with Mesa software GL (`LIBGL_ALWAYS_SOFTWARE=1`);
the window stayed responsive across multi-second sessions and was driven
via `PIL.ImageGrab` captures (files in `dist/`):

* `dist/screenshot-launch.png` — Markdown file open: menu bar, tab with
  modified-state dot and close button, line-number gutter, syntax-colored
  Markdown, status bar (Ln/Col, size, language, EOL, encoding).
* `dist/screenshot-rust.png` — Rust source: cyan keywords, green strings,
  dim comments, macro coloring; status bar shows `Rust · LF · UTF-8`.
* CLI surface verified directly: `raven-code --help`, `raven-code
  --version`; file and folder arguments open as documented.

Session-level checks performed by the verification scripts and by code
review of the persistence path:

* settings file created in the user config directory on first save, and
  reloaded on next start (`Settings::load_from` round-trip is unit-tested);
* unsaved-changes guard on tab close / close-all / quit;
* project detection populates the status bar when a folder is opened
  (background thread through `raven-projects::detect`).

## What could NOT be verified here (see limitations.md)

* Windows executable and `.ico` embedding (no Windows toolchain in this
  environment; exact build commands documented).
* Real language servers (rust-analyzer etc. not installable offline); the
  LSP client was verified end-to-end against a protocol-correct mock
  server instead.
* Native file dialogs (the environment has no desktop portal); the in-app
  path-entry fallback covers this and is exercised by the same code path.
