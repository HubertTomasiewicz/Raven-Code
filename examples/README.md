# Raven Pico examples

This directory contains two kinds of examples:

## `projects/` — sample projects for trying the CLI

Each subdirectory is a *standalone* project (they are excluded from the
workspace on purpose). Try Raven Pico against them:

```console
$ raven-pico detect examples/projects/hello-cmake
$ raven-pico build  examples/projects/hello-cmake
$ raven-pico test   examples/projects/hello-cmake
$ raven-pico run    examples/projects/hello-cmake -- --help 2>/dev/null || raven-pico run examples/projects/hello-rust
```

| Sample project | Kind | Markers | Notes |
|----------------|------|---------|-------|
| `hello-rust` | `rust` | `Cargo.toml` | fully self-contained, builds offline |
| `hello-cmake` | `cmake` | `CMakeLists.txt` | C project with a CTest smoke test |
| `hello-python` | `python` | `pyproject.toml`, `requirements.txt` | `run` executes `main.py` |
| `hello-godot` | `godot` | `project.godot` | `run` launches the Godot editor binary |
| `hello-maven` | `maven` | `pom.xml` | requires a JDK + Maven |
| `hello-gradle` | `gradle` | `settings.gradle`, `build.gradle` | application plugin, `run` via config |
| `hello-dotnet` | `dotnet` | `Hello.csproj` | requires the .NET SDK |
| `hello-lua` | `lua` | `main.lua`, `*.rockspec` | requires a Lua interpreter |
| `mixed-signals` | ambiguous | `pom.xml` + `build.gradle` | demonstrates true ambiguity: detect exits 5, pass `--kind maven\|gradle` to resolve (markers with *different* ranks — e.g. `Cargo.toml` + `pyproject.toml` — pick a ranked primary with a note instead) |
| `unknown-project` | — | none | demonstrates the "not detected" error with guidance; since 0.2.0 this stays true **even inside a repository** — parent projects are only reported with `--search-parents` |

Every sample requires the corresponding real toolchain to be installed
(`raven-pico doctor` tells you what is missing). Detection alone never
requires any toolchain.

## `api/` — library usage examples (`raven-api-examples`)

Compiled as part of the workspace, these show how another Rust application
(e.g. a future Raven Studio) uses Pico as a reusable backend:

```console
$ cargo run -p raven-api-examples --bin detect_project  -- examples/projects/hello-cmake
$ cargo run -p raven-api-examples --bin build_project  -- examples/projects/hello-rust
$ cargo run -p raven-api-examples --bin run_process
```

* `detect_project` — project detection and the JSON payload
* `build_project`  — detect → select → configure → execute → report
* `run_process`    — the process-execution layer (capture, timeout, cancel)

See `docs/integration.md` for the full integration guide.
