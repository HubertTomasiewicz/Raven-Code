# Security and reliability policy

Raven Pico is a developer tool that runs other people's build systems. This
document states exactly what it will and will not do, and why.

## Hard rules

1. **No silent installation.** Pico never installs, downloads, updates or
   removes toolchains, packages or dependencies. Missing tools produce a
   structured error (`tool_missing`, exit 4) with an actionable hint.
2. **No unrequested execution.** Pico runs only what the user asked for:
   `build`/`test`/`run`/`clean` on the selected project. It never executes
   downloaded scripts, hooks or "helpful" post-steps on its own initiative.
3. **No shell.** All process spawns construct argv directly
   (`Command::new(program).args(...)`). There is no shell interpretation
   anywhere — no `sh -c`, no `cmd /S /C` from user input — which removes an
   entire class of injection bugs. (Windows `.cmd`/`.bat` targets are spawned
   through the platform's standard mechanism with Rust std's argument
   escaping.)
4. **No overwriting.** `raven-pico init` refuses to overwrite an existing
   `raven-pico.toml` without explicit `--force`. `clean` removes only
   recognized generated state (`target/` via cargo, CMake build trees proven
   by `CMakeCache.txt`, Godot's `.godot` cache) — anything ambiguous is
   refused with an explanation.
5. **No secret leakage.** Environment variables are never dumped to logs or
   reports. `[env]` values from `raven-pico.toml` are applied to spawned
   processes but never serialized into JSON reports. Config file *contents*
   are never echoed on parse errors beyond the offending message.
6. **Bounded resources.** Captured output is capped at 10 MiB per stream
   (overflow drained and flagged, never stored). Version probes have
   timeouts. Directory scanning is single-level per directory plus a bounded
   ancestor walk — never a recursive crawl.

## What building/running a project means

Native build tools (cargo, cmake, dotnet, maven, ...) and configured
commands **execute arbitrary code** — that is what build systems do (build
scripts, code generators, test runners). Raven Pico makes this explicit:

* the README, `raven-pico init` template, and `raven-pico.toml` docs carry a
  standing notice,
* configured commands are executed verbatim, without a shell, only when the
  user invokes an operation,
* nothing runs at *detection* time — `detect` reads directory listings and
  file names only (it never parses or evaluates project files),
* network access is never initiated by Pico; package managers may fetch
  dependencies as part of their own normal operation (e.g. `cargo build` on
  a project with dependencies) — use your package manager's lockfile/offline
  flags via `--tool-arg` if you need reproducibility.

**Treat operating on an untrusted project like running a program from an
untrusted source.** Do it in a container or VM when the project is not
yours.

## Path and input handling

* User-provided paths are validated (exists, is a directory) and
  canonicalized before use; missing paths are usage errors, not panics.
* Program resolution distinguishes bare names (resolved from PATH) from
  paths (used as-is after an existence check) — a project cannot make Pico
  resolve a bare name from its own directory.
* Malformed project files (`raven-pico.toml` with syntax errors, unknown
  keys, future schema versions) produce structured errors; detection stays
  graceful and reports the status.
* Unknown project kinds never crash anything — they are a clean exit 3 with
  guidance.

## Reliability contract

* Ordinary failures are `PicoError` values with stable codes — no panics on
  missing tools, bad TOML, non-zero exits or killed processes.
* The process layer drains pipes concurrently (no pipe deadlocks), preserves
  exit codes, reports duration, and kills hung children on timeout or
  cancellation.
* Tool version probes are best-effort with a 4 s timeout — a tool that
  refuses `--version` still works for operations.

## Dependency policy

Runtime dependencies are limited to: `clap` (CLI), `serde` + `serde_json`
(JSON), `toml` (config), `thiserror` (errors), and `winresource`
(Windows icon embedding, build-time only). Everything else — `tempfile`,
`testkit` — is dev-only. New runtime dependencies require a deliberate
trade-off review.

## Reporting issues

Run with `--json` and attach the report (check it for anything you consider
sensitive first — captured stdout/stderr may contain your build output).
Internal errors (`internal_error`, exit 8) are bugs; please include the
command and the report.
