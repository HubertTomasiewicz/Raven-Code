# Raven Code — UI framework evaluation

The brief asked to "evaluate the UI framework already used by Raven Pico"
and to prefer retaining it if viable. This document records the evaluation
and the resulting decision.

## What Raven Pico 0.2.0 actually uses

Raven Pico is a **CLI build backend**. Its `apps/raven-pico/src/ui.rs` is
terminal presentation only (ANSI colors, report rendering, a
`ProgressSink` for stderr). The dependency tree is clap, serde, serde_json,
toml, thiserror (+ dev-only tempfile/testkit) — there is **no GUI framework
anywhere in the workspace**, and no windowing infrastructure to reuse.

So the question was not "keep or replace" but "which framework fits the
Raven architecture". The requirements from the brief and the codebase:

1. Rust-native, maintainable, mature — no second language/JS runtime.
2. Cross-platform with Windows as the ecosystem's primary target.
3. High-DPI, dark/light theming, resizable multi-panel layout — built in or
   cheap.
4. A real text-editing surface (caret, selection, IME, scroll) that can be
   extended with syntax highlighting — without writing an editor engine
   from scratch.
5. Small dependency footprint consistent with Pico's "minimal, auditable"
   ethos; no system-library build requirements on Windows.

## Candidates considered

| Framework | Assessment |
|---|---|
| **egui / eframe 0.36** | Pure Rust, immediate mode, mature and very actively maintained. Built-in dark/light styling, high-DPI, panels/docks, `TextEdit` with a custom layout hook (syntax highlighting without forking the widget), window icons, drag & drop, software-friendly GL backends. No system dependencies at build time on any platform. |
| iced | Retained-mode Elm style; text editing support historically weaker (no first-class code-edit surface), widget set geared to apps rather than editors. |
| Slint | Declarative, good fit for kiosks/embedded; licensing considerations (GPL/commercial) sit awkwardly next to Pico's MIT. |
| Tauri | WebView-based: ships a browser engine, JS bridge — the opposite of "lightweight, Rust core". |
| gtk4-rs | Heavy system dependency chain, painful on Windows. |
| gpui (Zed) | Not published/mature for external use at decision time. |
| Custom winit+wgpu | Exactly the "write an editor engine from scratch" path the brief forbids when a maintained library exists. |

## Decision: egui / eframe 0.36

Chosen for: Rust purity and maturity; zero build-time system deps
(Windows-first friendly); the documented `TextEdit::layouter` hook, which
lets Raven Code inject a syntect-backed, incrementally cached `LayoutJob`
(the same approach the community `egui_code_editor` crate takes — implemented
in-tree to avoid a version-coupled dependency and to add the gutter, caching
and read-only modes we need); built-in theming that maps 1:1 onto the Raven
theme palettes; and a layout system (`Panel`/`CentralPanel`, `Area`,
`Window`) that covers the required shell (menu, explorer, tabs, status bar,
bottom panel, palette, dialogs) without custom window management.

Dependency additions are confined to `apps/raven-code`:
`eframe` (glow backend), `syntect`, `rfd` (portal backend on Linux, native
dialog on Windows), `image` (PNG icon decode), `regex`. The Raven Pico
crates gained no new dependencies (only `libc` under `cfg(unix)` in
`raven-lsp` for the SIGTERM shutdown fallback).

## Notes on the 0.36 API that shaped the design

* `eframe::App` now splits `logic` (non-painting; background pumps, global
  shortcuts) from `ui` (panel drawing) — Raven Code drains job/LSP/index
  channels in `logic` and renders in `ui`.
* `Panel` (top/bottom/left/right) and `CentralPanel` are `Ui`-based;
  `Area`/`Window` remain context-based — the overlays use the latter.
* Multiline `TextEdit` sizes to its full galley and scrolls via the
  *enclosing* `ScrollArea`, which is what makes the separate line-number
  gutter (a sibling slot painted from the galley's per-line positions)
  scroll-accurate with zero scroll plumbing.
* Menus close on click by default (`PopupCloseBehavior`); `close_menu` is
  gone.
