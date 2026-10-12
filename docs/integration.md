# Integration guide — embedding Raven Pico

Pico's libraries are the backend of the Raven ecosystem. Raven Studio, Raven
Code and third-party applications consume the same APIs the CLI uses — the
binary is just one client. This document shows the integration surface and
the design sketch for the future local Raven service.

## Crate overview

| Crate | Use it for |
|-------|------------|
| `raven-core` | `PicoError` (codes + exit codes), `ProjectKind`, `PicoConfig`, `Report` (JSON v1), `ProgressEvent`/`ProgressSink` |
| `raven-projects` | `detect()`, `select()`, `Detection`/`Candidate`, `validate_directory()` |
| `raven-build` | `execute_operation()`, adapters, `proc::run_process()`, toolchain probing, doctor checks |

Add to your `Cargo.toml`:

```toml
[dependencies]
raven-core = { path = "path/to/raven-pico/crates/raven-core" }
raven-projects = { path = "path/to/raven-pico/crates/raven-projects" }
raven-build = { path = "path/to/raven-pico/crates/raven-build" }
```

(The paths will become registry/urls once the Raven ecosystem publishes
crates; until then, workspace-relative or git dependencies work.)

## The canonical flow

```rust
use raven_build::{execute_operation, CancelToken, Op, OpContext, Profile, StdioMode};
use raven_build::proc::path_dirs;
use raven_core::progress::{ProgressEvent, ProgressSink};
use raven_projects::{detect, select, DetectOptions};

struct UiSink;                                   // 1. Your progress renderer
impl ProgressSink for UiSink {
    fn on_event(&self, event: &ProgressEvent) {
        match event {
            ProgressEvent::CommandSpawned { program, args } => { /* show */ }
            ProgressEvent::CommandFinished { exit_code, duration_ms } => { /* show */ }
            _ => {}
        }
    }
}

pub fn build(root: &std::path::Path) -> Result<bool, raven_core::error::PicoError> {
    // 2. Detect (only the given directory by default; set
    //    DetectOptions::search_parents to also walk ancestors).
    let detection = detect(root, &DetectOptions::default())?;
    // 3. Select kind + root (or a structured AmbiguousProject error).
    let (kind, root) = select(&detection, None)?;
    // 4. Optional config; broken configs are hard errors here.
    let config = match &detection.config {
        Some(loaded) => Some(loaded.require()?.clone()),
        None => None,
    };
    // 5. Build the context.
    let ctx = OpContext {
        root,
        kind,
        config: config.as_ref(),
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env: config.as_ref().map(|c| c.env.clone()).unwrap_or_default(),
        search_paths: path_dirs(),
        sink: Some(&UiSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    };
    // 6. Execute and inspect the outcome.
    let outcome = execute_operation(&ctx, Op::Build)?;
    Ok(outcome.success())
}
```

Runnable, compile-verified examples: `examples/api` —
`detect_project`, `build_project`, `run_process`
(`cargo run -p raven-api-examples --bin build_project -- <path>`).

## Cancellation

Every operation takes a `CancelToken` (an `Arc<AtomicBool>`). Flip it from
any thread — a UI button, a request handler, a timeout:

```rust
let cancel = CancelToken::new();
let token = cancel.clone();
std::thread::spawn(move || {
    if user_pressed_stop_within(30) { token.cancel(); }
});
let outcome = execute_operation(&ctx, Op::Test)?;
if outcome.cancelled() { /* update UI */ }
```

The watchdog kills the direct child within ~20 ms of the flip. The CLI
itself relies on Ctrl+C (the OS kills the foreground group); the token is
the programmatic path.

## Progress events

`ProgressSink` receives: `Stage` (what Pico is doing), `ToolResolved` (tool +
version), `CommandSpawned` / `CommandFinished` (argv, exit code, duration)
and `Warning`. Implement the trait, pass `&sink` into `OpContext`. Events are
sequential per operation; the trait is `Send + Sync` so a channel-based sink
(`std::sync::mpsc::Sender` wrapper) forwards them to a UI thread.

## Structured results

`OperationOutcome` gives you: the resolved tool (name/version/path), the
exact argv, raw `ProcessResult` (exit code, captured stdout/stderr, duration,
cancel/timeout/truncation flags), known artifact paths and adapter notes.
For wire format, `outcome.to_payload(kind, profile, root)` produces exactly
the CLI's `OpPayload`, and `raven_core::report::Report` wraps it in the
versioned JSON envelope — same bytes your users see from `--json`.

## Versioning contract

* JSON report and `raven-pico.toml` both carry `schema_version = 1`.
* Additive evolution only; Pico refuses newer schema versions with an
  upgrade message rather than guessing.
* Error codes (`tool_missing`, `ambiguous_project`, ...) and exit codes
  (0–10) are part of the contract — build UIs on them.

## Future Raven service (design sketch, not implemented in 0.1)

Pico 0.1 deliberately has **no daemon or network service**. The planned
evolution reuses today's seams:

1. **Local background service** — a long-lived process hosting the same
   `OpContext`-based APIs, exposing them over a local IPC channel (named
   pipe / Unix domain socket). It would add *transport*, not new execution
   semantics: process execution, detection and adapters stay exactly as they
   are.
2. **Build IDs** — each accepted request gets an ID; results and artifacts
   are addressable by it (`OperationOutcome` is already value-shaped for
   this).
3. **Progress streaming** — `ProgressSink` events forwarded to subscribers
   of a build ID.
4. **Cancellation** — the service flips the operation's `CancelToken` on
   client request (already the designed use).
5. **Artifact metadata** — adapters report artifact paths today; the service
   would enrich them (size, hash, timestamp) for Raven Studio's UI.
6. **Client SDKs** — thin wrappers for Rust (the crates), and
   JSON-over-IPC clients for Raven Code / third-party tools.
7. **Custom project handlers** — new adapters register into the same
   registry (`adapter_for` / `all_adapters`); a service could load
   ecosystem-provided handlers without touching the core.

Security posture stays identical: no toolchain installation, no shell, no
unrequested network access; configured commands remain explicit project code
execution with the same opt-in rules.
