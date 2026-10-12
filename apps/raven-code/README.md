# Raven Code

**The Raven ecosystem's lightweight, fast code and text editor.**

Raven Code is a real Rust desktop application built on the same library
crates as Raven Pico (`raven-core`, `raven-projects`, `raven-build`) plus two
new editor-specific crates (`raven-editor`, `raven-lsp`). It edits any file
or folder, needs **no compiler, language server, or other Raven application**
to run, and treats language intelligence as an optional add-on.

```
Raven ecosystem
┌──────────────┐   ┌─────────────────────────────────────────────┐
│ Raven Studio │   │ apps/raven-code (eframe/egui shell)         │
│ Raven Code   │◄──┤ crates/raven-editor (buffers · search ·     │
│ Raven Pico   │   │   settings · themes · languages)            │
│ Raven Hub    │   │ crates/raven-lsp (JSON-RPC LSP client)      │
└──────────────┘   │ raven-core / raven-projects / raven-build   │
                   │   (shared Raven Pico SDK)                   │
                   └─────────────────────────────────────────────┘
```

## Building and running

Prerequisites: **Rust stable 1.85+** (edition 2024) via
[rustup](https://rustup.rs). No other runtime dependencies.

```console
cd raven-pico                       # the workspace root
cargo build --release -p raven-code
# binary: target/release/raven-code
```

Run it:

```console
./target/release/raven-code                    # empty start
./target/release/raven-code src/main.rs        # open files in tabs
./target/release/raven-code .                  # open a folder workspace
./target/release/raven-code --help
```

First launch creates `raven-code.toml` in your user config directory
(`~/.config/raven-code` on Linux, `%APPDATA%\RavenCode` on Windows) the first
time settings are saved. Delete the file to reset to defaults.

### Windows build

Windows is the primary target platform of the Raven ecosystem; this build was
verified on Linux x86_64. To produce `raven-code.exe` on a Windows machine
(with Visual Studio Build Tools installed):

```console
cargo build --release -p raven-code
# binary: target\release\raven-code.exe (brand icon embedded via winresource)
```

The window/taskbar icon comes from the official Raven crow SVG, rasterized to
PNG and ICO assets under `apps/raven-code/assets/` (see
`scripts/gen_brand_assets.py` in the delivery workspace).

## Using the editor

| Action | Shortcut |
|--------|----------|
| Command palette | `Ctrl+Shift+P` |
| Quick open file | `Ctrl+P` |
| New / Open file / Open folder | `Ctrl+N` / `Ctrl+O` / `Ctrl+Shift+O` |
| Save / Save As / Save all | `Ctrl+S` / `Ctrl+Shift+S` / `Ctrl+Alt+S` |
| Find / Replace | `Ctrl+F` / `Ctrl+H` |
| Toggle explorer / bottom panel | `Ctrl+B` / `Ctrl+J` |
| Build / Test project (Raven Pico SDK) | `Ctrl+Shift+B` / `Ctrl+Shift+T` |
| Next / previous tab | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Toggle split editor | `Ctrl+\` |
| Toggle dark/light theme | `Ctrl+Shift+D` |
| Zoom | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |

Every command is listed in **Help ▸ Keyboard Shortcuts** and can be rebound
under `[keys]` in `raven-code.toml` by action id, e.g. `save = "Ctrl+S"`.

Editing basics: standard clipboard/undo shortcuts work natively
(`Ctrl+Z/Ctrl+Y`, `Ctrl+X/C/V`, `Ctrl+A`); `Tab`/`Shift+Tab` indent and
dedent selections; pressing Enter auto-indents and continues block
indentation. Files that are not valid UTF-8/UTF-16 are refused rather than
corrupted; BOMs and CRLF/LF line endings round-trip byte-identically.

## Language servers (optional)

Raven Code never installs anything. To attach a language server, add it to
settings and it starts lazily the first time a matching file is opened:

```toml
[lsp.servers.rust-analyzer]
command = "rust-analyzer"
languages = ["rust"]
```

With a server configured: diagnostics appear in the bottom panel,
`Ctrl+Space` triggers completion, `F12` goes to definition, `Shift+Alt+F`
formats the document, and document symbols are listed in the Symbols tab.
See `docs/raven-code/limitations.md` for the honest support matrix.

## Build, test, verify

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

The LSP client is covered by an end-to-end test against a protocol-correct
mock server (`crates/raven-lsp/tests/mock_server.rs`) — no real language
server required.

## Documentation index

* [docs/raven-code/feature-summary.md](../../docs/raven-code/feature-summary.md)
* [docs/raven-code/limitations.md](../../docs/raven-code/limitations.md)
* [docs/raven-code/custom-syntaxes.md](../../docs/raven-code/custom-syntaxes.md)
* [docs/raven-code/ui-framework.md](../../docs/raven-code/ui-framework.md)
* [docs/architecture.md](../architecture.md) — the shared workspace architecture
