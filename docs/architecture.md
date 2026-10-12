# Architecture

Raven Pico is a Cargo workspace with a strict layering: reusable libraries at
the bottom, one thin binary at the top. Nothing in the libraries knows the
CLI exists, which is what makes Pico embeddable by future Raven applications.

```
┌────────────────────────────────────────────────────────────────┐
│ apps/raven-pico          CLI binary (clap, terminal rendering) │
├────────────────────────────────────────────────────────────────┤
│ tests/                   cross-crate integration tests         │
├────────────────────────────────────────────────────────────────┤
│ raven-build              adapters · process execution ·        │
│                          toolchain discovery · doctor          │
├────────────────────────────────────────────────────────────────┤
│ raven-projects           marker detection · root search ·      │
│                          candidate ranking                     │
├────────────────────────────────────────────────────────────────┤
│ raven-core               errors · kinds · config · JSON report │
│                          progress events                       │
└────────────────────────────────────────────────────────────────┘
   testkit (dev-only): fixture projects + fake toolchains
```

## Crate responsibilities

### raven-core

* **`error.rs`** — `PicoError`, the single error type. Every variant carries a
  stable `ErrorCode` (`"tool_missing"`, ...) and a documented exit code
  (0–10). Ordinary failures never panic.
* **`kind.rs`** — `ProjectKind` (10 ecosystems) and `Confidence`
  (`explicit | high | medium`).
* **`config.rs`** — `raven-pico.toml` (schema version 1). `deny_unknown_fields`
  catches typos; unknown *schema versions* are rejected with an upgrade hint.
  Loading is graceful: detection reports a broken config as a status, while
  operations refuse to run.
* **`report.rs`** — the JSON report envelope (schema version 1). One
  `Report` per command invocation; the payload lives under the key named
  after the command. The schema is additive-only; changes require a version
  bump.
* **`progress.rs`** — `ProgressEvent` + `ProgressSink` trait. This is the
  seam future Raven UIs plug into: the CLI renders events to a terminal, a
  service could stream them over IPC.

### raven-projects

Marker-based detection. Rules table (`MARKER_RULES`) maps file names /
extensions to kinds with a **rank**:

| Rank | Kind | Marker |
|------|------|--------|
| 90 | rust | `Cargo.toml` |
| 85 | visual-studio | `.sln`, `.slnx`, `.vcxproj` |
| 80 | dotnet | `.csproj`, `.fsproj`, `.vbproj` |
| 78 | cmake | `CMakeLists.txt` |
| 70 | maven / gradle | `pom.xml` / `build.gradle(.kts)` |
| 65 | godot | `project.godot` |
| 60 | python | `pyproject.toml`, `setup.py`, `requirements.txt` |
| 50 | lua | `main.lua`, `conf.lua`, `*.rockspec` |

Selection rules:

1. An explicit override (`--kind`, or `[project] kind` in the config) wins.
2. A config with commands but no kind becomes a `custom` candidate (rank 100).
3. The top-ranked candidate is the primary **only when the top rank is
   unique**; a tie (e.g. `pom.xml` + `build.gradle`) is *ambiguous* and
   operations require an explicit choice. Detection always reports *all*
   candidates.

Root search: the start directory wins if it has markers or a config.
Ancestors are inspected **only** when explicitly requested (`--search-parents`
on the CLI, `DetectOptions::search_parents` in the library — default off, so
a markerless directory is "not detected" rather than silently resolving to
an ancestor project). The walk is bounded (default 32) and only single-level
directory listings are used — no recursive scanning. Inputs are canonicalized
(and Windows verbatim `\\?\` prefixes are stripped when safe) so relative
paths and parent-walking behave identically everywhere.

### raven-build

Three independent layers:

**`proc.rs` — process execution.** `ProcessSpec` → `ProcessResult`.
* stdout/stderr drained on two threads *concurrently with* the wait loop —
  a full pipe cannot deadlock, and cancel/timeout fire while output is
  pending.
* Captured output capped per stream (10 MiB); overflow is still drained
  (discarded) so children finish, reported via `truncated`.
* Watchdog loop honours `CancelToken` and `timeout`; kill targets the direct
  child (documented limitation: grandchildren may delay pipe EOF).
* Bare program names resolved against explicit `search_paths` (empty → real
  `PATH`), with Windows executable-extension probing (`.exe/.cmd/.bat`).
* Launch failures are structured (`spawn_error`), never panics; exit codes
  preserved; duration measured. `StdioMode::Inherit` lets text-mode `run`
  stay interactive.

**`toolchain.rs` — discovery.** Catalog of known tools (programs, version
args, platform scope, actionable hints). Version probes have a 4 s timeout
and forgiving parsers (java prints to stderr, gradle to stdout, cmake has a
prefix). `find_msbuild()` uses vswhere, then falls back to scanning the
standard Visual Studio roots.

**Adapters** — one module per ecosystem implementing `BuildAdapter`
(`capabilities`, `build`, `test`, `run`, `clean`). Command selection
precedence is centralized in `execute_operation`:

1. `raven-pico.toml` section for the operation (project-defined command) —
   `--release` is rejected here because configured commands have no profile.
2. Native adapter, if its capability for the operation is `Available`.
3. Otherwise a structured `UnsupportedOperation` error (exit 9) — Pico never
   claims support it does not have.

`doctor.rs` produces environment checks (runtime facts, PATH sanity, tool
probes) used by both `doctor` and `toolchains`.

### apps/raven-pico

clap definitions (`cli.rs`), terminal rendering + `ProgressSink`
implementation (`ui.rs`), and orchestration (`main.rs`) — detect → select →
configure → execute → report. Exit codes come from `PicoError`; JSON mode
guarantees a pure stdout.

## Data flow of an operation

```
CLI flags ──▶ validate path (canonicalize)
          ──▶ detect(start)            raven-projects: markers, config, ranking
          ──▶ select(detection, kind)  kind + root (or structured error)
          ──▶ OpContext                profile, extra args, env, sink, cancel
          ──▶ execute_operation(op)    config section ▸ native adapter ▸ error
                  └─▶ proc::run_process     (resolve ▸ spawn ▸ drain ▸ watchdog)
          ──▶ OperationOutcome ──▶ Report (JSON v1) or terminal rendering
```

## Design decisions and rationale

* **Sync, thread-based execution.** No async runtime, no tokio: the
  operations are inherently process-bound; threads solve pipe draining and
  watchdogs cleanly and keep the dependency tree small. A future service can
  wrap these APIs in any runtime.
* **Cancellation as a first-class token** (not signal handling): a future UI
  cancels by flipping an `AtomicBool`, which the watchdog turns into
  `child.kill()`. The CLI relies on the OS (Ctrl+C kills the foreground
  process group) for interactive use.
* **Detection ≠ build system.** A directory with `Cargo.toml` is the `rust`
  *kind*; the adapter decides the actual tool. This keeps detection honest
  and adapters swappable.
* **Config commands override native adapters** per operation, giving every
  kind an escape hatch — including kinds Pico cannot natively drive
  (`custom`).
* **Versioned contracts.** JSON report and config both carry
  `schema_version = 1`. Additive evolution only; breaking changes bump the
  version and Pico refuses newer files with an upgrade hint.

## Future service design (documented, not implemented)

Pico 0.1 deliberately ships **no daemon**. The intended evolution (see
`docs/integration.md`):

* a local background service hosting the same `OpContext`-based APIs,
* build IDs and artifact metadata on every operation,
* progress events streamed to subscribers,
* client SDKs for Raven Studio / Raven Code / third-party tools,
* custom project handlers registered by future ecosystem components.

The seams for all of these already exist (`ProgressSink`, `CancelToken`,
`OperationOutcome` artifacts, adapter registry) — the service would add
transport, not new execution semantics.
