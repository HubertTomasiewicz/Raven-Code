# Adding a build adapter

Raven Pico is designed to grow one ecosystem at a time. An adapter is a
single module with no global state. This guide walks through the pattern —
use `crates/raven-build/src/adapters/godot.rs` (small) or `cmake.rs`
(thorough) as references.

## 1. Decide what you honestly support

Before writing code, fill this matrix for the ecosystem and implement only
what is genuinely true. "Honest" is a hard rule: Pico must never report
success for something it did not do, and must never claim support it does
not have.

| Operation | Options |
|-----------|---------|
| build / test / run / clean | `Support::Available` — you implement it natively |
| | `Support::ConfigRequired { hint }` — user must configure it in `raven-pico.toml` |
| | `Support::Unsupported { reason, hint }` — structurally impossible |

The operation dispatcher (`execute_operation`) already handles: config
section precedence, `--release` rejection for configured commands, and
`UnsupportedOperation` errors. Your adapter only implements the native path.

## 2. Add the project kind (if new)

If the ecosystem needs a new kind, extend `raven-core/src/kind.rs`
(`as_str`, `display_name`, aliases) and add marker rules in
`crates/raven-projects/src/lib.rs` (`MARKER_RULES`) with a **rank**. Ranks
decide primary selection; document yours in `docs/toolchains.md` and
`docs/architecture.md`. Add detection tests in
`crates/raven-projects/src/tests.rs`.

## 3. Implement the adapter

```rust
// crates/raven-build/src/adapters/mytool.rs
use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Support};
use super::support;

pub struct MyToolAdapter;

const HINT: &str = "install MyTool: https://example.com/download";

impl crate::BuildAdapter for MyToolAdapter {
    fn kind(&self) -> ProjectKind { ProjectKind::Custom /* your kind */ }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::Available,
            test: Support::ConfigRequired { hint: None },
            run:  Support::Available,
            clean: Support::Unsupported { reason: "no clean concept".into(), hint: None },
        }
    }

    fn build(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        // Resolve the tool (structured ToolMissing error with your hint).
        let (path, tool) = support::resolve(ctx, "mytool", &["--version"], HINT)?;
        // Build the command; ctx.profile for --release, ctx.extra_args for
        // --tool-arg, cwd = ctx.root. Never use a shell.
        let spec = support::with_extra_args(
            ProcessSpec::new(path.display().to_string()).arg("build"),
            ctx,
        ).cwd(ctx.root.clone());
        support::execute(ctx, tool, spec, /* artifacts */ Vec::new(), /* note */ None)
    }
    // test / run / clean follow the same shape; return
    // PicoError::UnsupportedOperation for the gaps you decided in step 1.
}
```

Rules that keep adapters consistent:

* Resolve tools with `support::resolve` — it produces the structured
  `ToolMissing` error (exit 4) with your hint and best-effort version info.
* Build argv, never shell strings. Paths with spaces need no quoting.
* Always `cwd(ctx.root.clone())` (or a config-relative subdir).
* Pass `ctx.env` through `support::execute` (it does this for you).
* Artifacts: report only paths you can honestly name.
* Guard destructive `clean` paths: verify generated-state markers before
  deleting anything (see `CmakeAdapter::clean`).

## 4. Register it

`crates/raven-build/src/adapters/mod.rs`:

```rust
pub mod mytool;
// in all_adapters():
&adapters::mytool::MyToolAdapter,
```

`all_adapters()` drives `adapter_for(kind)`; `execute_operation` and the
CLI pick it up automatically.

## 5. Test it — with fake tools

No real toolchain is needed. Use `testkit::FakeTool`, which records
invocations to `$FAKE_TOOL_LOG` and honours `FAKE_TOOL_EXIT`:

```rust
#[test]
fn mytool_build_constructs_expected_command() {
    let project = TempDir::new().unwrap();
    testkit::marker_project(project.path(), &[("mytool.toml", "")]).unwrap();
    let tool = FakeTool::new("mytool");
    let (log_key, log_value) = tool.log_env();
    let ctx = OpContext {
        root: project.path().to_path_buf(),
        kind: ProjectKind::Custom /* your kind */,
        config: None,
        profile: Profile::Debug,
        extra_args: vec![],
        env: [(log_key, log_value)].into_iter().collect(),
        search_paths: tool.search_paths(),   // isolated: no real PATH
        sink: Some(&SilentSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    };
    let outcome = execute_operation(&ctx, Op::Build).unwrap();
    assert!(outcome.success());
    assert_eq!(tool.log_lines(), vec!["fake mytool build"]);
}
```

Cover at minimum: command construction, `--release` handling, missing tool
(`ToolMissing`), unsupported operations (`UnsupportedOperation`), and any
destructive path.

## 6. Document it

* `docs/toolchains.md` — matrix entry, exact commands, limitations.
* `README.md` — one table row.
* `docs/cli-reference.md` — if any flag interacts differently.
* `docs/architecture.md` — rank of the new markers.

## 7. Check the gates

```console
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

That's it — the registry, doctor (`toolchain.rs` catalog if your tool should
appear in `doctor`/`toolchains`), detection, CLI and JSON schema pick the
rest up automatically.
