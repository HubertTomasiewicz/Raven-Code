# Raven Code

Raven Code is a lightweight desktop code and text editor written in Rust and built as part of the Raven ecosystem.

The goal is to keep the editor relatively small and fast while still providing the features you would expect from a modern code editor: tabs, a file explorer, syntax highlighting, search, project integration, configurable shortcuts, and optional language-server support.

Raven Code can also work completely on its own. You do not need Raven Pico, a compiler, or a language server installed just to open and edit files.

## Features

### Editing

- Multiple files open in tabs
- Split editor view
- Undo and redo
- Clipboard support
- Automatic indentation
- Tab and Shift+Tab indentation
- Word wrapping
- Configurable tab width and spaces/tabs
- Line numbers and active-line highlighting
- Configurable editor font and zoom
- Autosaving for file-backed documents
- UTF-8 and UTF-16 file support, including BOM detection
- CRLF and LF line-ending preservation
- Protection against accidentally opening unsupported binary files
- Large-file limits to keep the editor responsive

### Syntax highlighting

Raven Code uses Sublime syntax grammars through `syntect`.

It includes support for a large collection of languages and file types, including:

- Rust
- C and C++
- C#
- Python
- Java
- JavaScript and TypeScript
- Go
- Lua
- GDScript
- JSON
- YAML
- TOML
- XML
- Markdown
- HTML/CSS
- and many others

Custom `.sublime-syntax` files can also be loaded from the user's syntax directory.

### Navigation and search

- Command palette
- Quick file open
- Find and replace
- Regular-expression search
- Whole-word and case-sensitive search
- Find in files
- Go to line
- Recent files and folders
- Workspace file explorer
- Lazy directory loading for large projects
- Next/previous tab navigation

The workspace index used for quick open and file search runs in the background so opening a larger project does not require scanning the entire directory tree up front.

### File management

Files and folders can be managed directly from the editor:

- Create files
- Create directories
- Rename files
- Delete files
- Open files through native dialogs
- Open folders as workspaces
- Drag files and folders onto the window
- Save, Save As, and Save All
- Restore recent workspaces

Unsaved changes are checked before destructive operations, and file writes use a temporary-file approach so a failed save does not simply truncate the original file.

### Themes and settings

Raven Code includes two built-in themes:

- Raven Dark
- Raven Light

Settings are stored in a versioned `raven-code.toml` configuration file. Window size, panel visibility, editor preferences, recent files, zoom level, keybindings, syntax highlighting settings, and other preferences can be persisted there.

An optional custom font can also be configured, with best-effort system font fallback for CJK characters.

### Language Server Protocol

Language intelligence is optional rather than being required for the editor to work.

Raven Code includes an LSP client supporting features such as:

- Diagnostics
- Completion
- Hover information
- Go to definition
- Document symbols
- Document formatting
- Full-document synchronization

Language servers are configured manually and are started only when a matching file is opened. Raven Code does not download or install language servers for you.

For example:

```toml
[lsp.servers.rust-analyzer]
command = "rust-analyzer"
languages = ["rust"]
```

Once configured, language-server features can be accessed with shortcuts such as `Ctrl+Space` for completion and `F12` for go-to-definition.

The LSP implementation is based on JSON-RPC over standard input/output and includes request handling, cancellation, diagnostics, and protocol framing.

### Raven Pico integration

Raven Code shares several crates with Raven Pico and can use the Raven Pico SDK to interact with projects.

From inside the editor you can:

- Build a project
- Run tests
- Run the project
- Clean build artifacts
- View build output
- Cancel running operations
- Run an arbitrary external command
- Open an external terminal in the current workspace

Project detection and build operations use the same project and process handling used by the Raven Pico CLI, rather than maintaining a separate implementation inside the editor.

## Keyboard shortcuts

Some of the main shortcuts are:

| Action | Shortcut |
| --- | --- |
| Command palette | `Ctrl+Shift+P` |
| Quick open | `Ctrl+P` |
| New file | `Ctrl+N` |
| Open file | `Ctrl+O` |
| Open folder | `Ctrl+Shift+O` |
| Save | `Ctrl+S` |
| Save As | `Ctrl+Shift+S` |
| Save All | `Ctrl+Alt+S` |
| Find | `Ctrl+F` |
| Replace | `Ctrl+H` |
| Toggle explorer | `Ctrl+B` |
| Toggle bottom panel | `Ctrl+J` |
| Build project | `Ctrl+Shift+B` |
| Run tests | `Ctrl+Shift+T` |
| Next tab | `Ctrl+PageDown` |
| Previous tab | `Ctrl+PageUp` |
| Split editor | `Ctrl+\` |
| Toggle theme | `Ctrl+Shift+D` |
| Zoom in | `Ctrl+=` |
| Zoom out | `Ctrl+-` |
| Reset zoom | `Ctrl+0` |

The complete list can be viewed from **Help > Keyboard Shortcuts**. Keybindings can also be changed in `raven-code.toml`.

## Installation and building

Raven Code is currently built from source as part of the Raven workspace.

You will need Rust stable 1.85 or newer with edition 2024 support.

Clone the repository and build the application:

```bash
cargo build --release -p raven-code
```

The resulting binary will be located at:

```text
target/release/raven-code
```

On Windows:

```text
target\release\raven-code.exe
```

Run the editor directly with:

```bash
cargo run --release -p raven-code
```

Or pass files and folders when starting it:

```bash
raven-code src/main.rs
```

```bash
raven-code .
```

Multiple files can be passed as well and will be opened as tabs.

```bash
raven-code main.rs lib.rs README.md
```

The application also supports:

```bash
raven-code --help
raven-code --version
```

## Configuration

Raven Code creates its configuration when settings are saved.

On Linux the configuration is stored under:

```text
~/.config/raven-code
```

On Windows:

```text
%APPDATA%\RavenCode
```

The main configuration file is:

```text
raven-code.toml
```

Configuration is versioned with `schema_version = 1`. Invalid configuration does not prevent the editor from starting; Raven Code falls back to defaults and displays a notice.

The configuration can be used for things such as:

- Editor appearance
- Font settings
- Autosave
- Word wrapping
- File size limits
- Workspace behaviour
- Recent projects
- Keyboard shortcuts
- Language servers

## Project structure

Raven Code is part of a larger Rust workspace.

The main editor-related crates are:

```text
apps/
  raven-code/       Desktop application

crates/
  raven-editor/     Editor and document functionality
  raven-lsp/        Language Server Protocol client
  raven-core/       Shared Raven types and utilities
  raven-projects/   Project detection
  raven-build/      Build and process integration
```

The GUI itself is built with `eframe`/`egui`. Syntax highlighting is provided by `syntect`, native file dialogs by `rfd`, and language-server communication is handled by the Raven LSP crate.

Keeping the editor logic in separate crates means some of the functionality can be reused without depending on the GUI.

## Development

The full workspace can be checked with:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

The LSP client also has an end-to-end test using a protocol-correct mock language server, so the editor does not need a real language server installed just to test the LSP implementation.

## Current limitations

Raven Code is still a relatively young editor, so it does not try to compete feature-for-feature with larger editors yet.

Some things that are currently missing or limited include:

- No built-in terminal emulator panel
- No multi-cursor editing
- No snippets
- No tab dragging/reordering
- No tab pinning
- No symbol rename or code actions
- No semantic token support
- LSP completion does not currently handle every advanced `textEdit` or snippet case
- Very large files are intentionally restricted rather than being loaded through a virtual/paged buffer
- Real language servers have not all been tested in every supported environment

The project is intended to grow into these areas over time, but the current implementation focuses on keeping the core editor usable and reasonably simple.

## License

Raven Code is part of the Raven project and is licensed under the MIT License.