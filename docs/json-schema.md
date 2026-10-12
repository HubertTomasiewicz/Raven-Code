# JSON report schema — version 1

Every command supports `--json`. In that mode:

* exactly **one** JSON document is printed on **stdout** (pretty-printed),
* progress and warnings go to **stderr**,
* the exit code matches the documented table (README / cli-reference).

The envelope is a compatibility contract: `schema_version` stays `1` until a
breaking change forces a bump, and Pico consumers should treat unknown
fields as additive.

## Envelope

```jsonc
{
  "schema_version": 1,              // number, always 1 for this format
  "tool": "raven-pico",             // constant
  "tool_version": "0.2.0",          // semver of the CLI
  "command": "build",               // detect|build|test|run|clean|doctor|toolchains|init
  "success": true,                  // did the operation achieve its goal
  "exit_code": 0,                   // raven-pico's OWN exit code (0–10), not the tool's
  "duration_ms": 1234,              // wall-clock time of the whole command

  // Payload keys — only the one matching `command` is present:
  "detect":     { /* DetectPayload */ },
  "build":      { /* OpPayload */ },
  "test":       { /* OpPayload */ },
  "run":        { /* OpPayload */ },
  "clean":      { /* OpPayload */ },
  "doctor":     { /* DoctorPayload */ },
  "toolchains": { /* ToolchainsPayload */ },
  "init":       { /* InitPayload */ },

  // Present when success == false:
  "error": {
    "code": "tool_missing",         // stable machine code, see table below
    "message": "required tool 'cargo' was not found",
    "hint": "install Rust via rustup: https://rustup.rs"
  }
}
```

## Error codes

| `code` | Exit | Meaning |
|--------|------|---------|
| `invalid_path` | 2 | Path missing or not a directory |
| `not_detected` | 3 | No recognizable project |
| `ambiguous_project` | 5 | Top-ranked candidates tie; explicit choice required |
| `tool_missing` | 4 | Required tool not found on PATH |
| `tool_failed` | 1 | Invoked tool returned a non-zero exit code |
| `unsupported_operation` | 9 | Operation not supported for this kind |
| `config_invalid` | 6 | `raven-pico.toml` broken or unsupported version |
| `config_exists` | 10 | `init` conflict (needs `--force`) |
| `cancelled` | 7 | Operation cancelled |
| `io_error` | 8 | Filesystem failure |
| `internal_error` | 8 | Bug — please report |

## DetectPayload

```jsonc
{
  "searched": "/work/demo",            // absolute start directory (Windows: verbatim \\?\ prefix removed when safe)
  "root": "/work/demo",                // selected project root
  "primary": { /* CandidateDto or null */ },
  "candidates": [ /* CandidateDto, ranked best-first */ ],
  "ambiguous": false,                  // true => primary is null, choice required
  "config": {                          // optional raven-pico.toml report
    "path": "/work/demo/raven-pico.toml",
    "status": "ok",                    // ok | parse_error | unsupported_version
    "message": "…",                    // present on parse problems
    "schema_version": 1,
    "kind_override": "cmake",          // [project] kind, if any
    "defined_sections": ["build", "run"]
  },
  "searched_upwards": false            // root found via parent search (--search-parents; opt-in since 0.2.0)
}
```

CandidateDto:

```jsonc
{
  "kind": "rust",                      // canonical kind id
  "display": "Rust (Cargo)",           // human-readable
  "confidence": "high",                // explicit | high | medium
  "markers": ["Cargo.toml"],           // matched marker files
  "root": "/work/demo"
}
```

## OpPayload (build / test / run / clean)

```jsonc
{
  "kind": "rust",
  "root": "/work/demo",                // project root
  "profile": "debug",                  // debug | release | n/a
  "tool": {                            // absent for built-in ops (e.g. Godot cache removal)
    "name": "cargo",
    "version": "1.89.0",
    "path": "/usr/local/cargo/bin/cargo"
  },
  "command": ["/usr/local/cargo/bin/cargo", "build"],  // exact argv
  "cwd": "/work/demo",
  "exit_code": 0,                      // the TOOL's exit code; null if not launched/killed
  "duration_ms": 812,                  // tool duration
  "stdout": "…",                       // captured; null when empty or inherited
  "stderr": "…",
  "artifacts": ["/work/demo/target/debug"],  // known generated paths
  "cancelled": false,
  "timed_out": false,
  "truncated": false,                  // capture cap (10 MiB/stream) hit
  "note": "…"                          // adapter remark (e.g. "configured build directory first")
}
```

Note the distinction: envelope `exit_code` is raven-pico's own documented
code; `build.exit_code` is the invoked tool's. In text mode, `run` inherits
stdio, so `stdout`/`stderr` are null and only exit code + duration are
reported.

## DoctorPayload

```jsonc
{
  "os": "linux",                       // windows | macos | linux | …
  "arch": "x86_64",
  "healthy": true,                     // no check has status "error" AND at least one toolchain is usable
  "checks": [ /* CheckDto */ ]
}
```

## ToolchainsPayload

```jsonc
{
  "checks": [ /* CheckDto */ ],
  "found": 7,
  "missing": 6
}
```

CheckDto:

```jsonc
{
  "id": "cargo",
  "status": "ok",                      // ok | missing | n/a | error
  "version": "1.99.0",
  "path": "/usr/local/cargo/bin/cargo",
  "detail": "…",                       // n/a note, or "required for: …" when missing
  "hint": "…"                          // actionable install hint when missing
}
```

Status semantics (0.2.0):

* `ok` — the version probe launched the tool and it exited 0. Installation
  is **never** inferred from a detected project type.
* `missing` — no program of the tool's catalog could be probed
  successfully. `detail` names the project kinds that need the tool and
  `hint` gives installation guidance.
* `n/a` — Windows-only tools (`msbuild`) on non-Windows platforms: not
  applicable, not missing, counted in neither `found` nor `missing`.
* `error` — an unexpected probe failure; `healthy` becomes false.

## InitPayload

```jsonc
{
  "config_path": "/work/demo/raven-pico.toml",
  "created": true,
  "kind": "rust"                       // pre-selected kind, if any
}
```

## Stability rules for consumers

1. Always check `schema_version` before parsing payloads.
2. Treat unknown payload fields as additive; never fail on them.
3. Use `error.code` for branching, `error.message` for display.
4. Distinguish envelope `exit_code` (raven-pico) from payload `exit_code`
   (the tool that ran).
