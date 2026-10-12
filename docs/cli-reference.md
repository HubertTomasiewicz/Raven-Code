# CLI reference

Binary: `raven-pico` (`.exe` on Windows). All paths accept spaces.
`PATH` positional arguments default to the current directory.

Global flags (usable before or after the subcommand):

| Flag | Effect |
|------|--------|
| `--json` | Print one JSON report (schema v1) on stdout; progress goes to stderr |
| `--color auto\|always\|never` | Colour control; `auto` respects `NO_COLOR` and terminal detection |
| `--quiet`, `-q` | Suppress progress output; errors still print |
| `--search-parents` | Opt in to parent-directory discovery (see below); default **off** |
| `--version`, `-V` | Print `raven-pico <version>` |
| `--help`, `-h` | Help with examples and the exit-code table |

## Discovery rules (0.2.0)

These rules are deliberate and identical for `detect`, `build`, `test`,
`run`, `clean` and `init`:

1. **Path errors are not detection results.** A missing path fails with
   `invalid_path` (exit 2, "path does not exist: …"); a file path fails with
   `not a directory` (exit 2).
2. **The supplied directory wins.** If it contains project markers or a
   `raven-pico.toml`, it is the project root.
3. **No silent ancestor projects.** A directory without markers is "not
   detected" (exit 3). Parent directories are inspected only when
   `--search-parents` is passed (or `DetectOptions::search_parents` is set
   by SDK clients). The walk is bounded (32 levels), stops at the first
   directory with markers or a config, and is reported as
   `"searched_upwards": true` in JSON.
4. Windows paths are serialized without the verbatim `\\?\` prefix whenever
   that cannot change what the path points at (long paths, reserved device
   names, trailing dots/spaces and `.`/`..` components keep the prefix).

---

## `raven-pico detect [PATH]`

Identifies the project type and root.

Behaviour: reports *all* matching candidates ranked; chooses a primary only
when the top rank is unique. Exit codes: 0 (primary chosen), 3 (nothing
detected), 5 (ambiguous — pass `--kind` or configure `raven-pico.toml`), 2
(invalid path). A malformed `raven-pico.toml` is reported in the payload but
does not fail `detect`.

## `raven-pico build [PATH]`

Invokes the project's build system.

| Flag | Effect |
|------|--------|
| `--release` | Release profile (adapter-dependent: cargo `--release`, cmake `-DCMAKE_BUILD_TYPE=Release` + `--config Release`, dotnet `-c Release`; rejected for kinds without a profile concept) |
| `--kind <KIND>` | Force the project kind: `rust`, `cmake`, `visual-studio`, `dotnet`, `python`, `godot`, `maven`, `gradle`, `lua`, `custom` |
| `--tool-arg <ARG>` | Extra argument appended to the tool invocation (repeatable) |

## `raven-pico test [PATH]`

Runs the project's test suite. Same flags as `build`. Adapters: cargo test,
ctest (`--output-on-failure`), dotnet test, mvn test, gradlew test, pytest
(only when installed), otherwise configure `[test]`.

## `raven-pico run [PATH]`

Runs the project. Same flags as `build`. In **text mode** the child inherits
your terminal (interactive programs work); in `--json` mode output is
captured into the report. Adapters: cargo run, dotnet run, godot `--path`,
python `main.py`, lua `main.lua`, configured `[run]` for everything else.

CMake projects: `run` requires a `[run]` section in `raven-pico.toml` —
CMake does not record which target is the program, and Pico will not invent
an executable path (exit 9 with guidance when unconfigured).

## `raven-pico clean [PATH]`

Native clean operation. cargo clean, CMake clean target (build tree removal
only when `CMakeCache.txt` proves it is a build tree), MSBuild `-t:Clean`,
dotnet clean, mvn clean, gradlew clean, Godot `.godot` cache removal,
otherwise configure `[clean]`.

## `raven-pico doctor`

Environment report — runtime facts, PATH sanity, and one check per known
tool. A tool is `ok` **only when its version probe launches and exits 0**;
installation is never inferred from a detected project type. Windows-only
tools report `n/a` on other platforms. Missing tools name the project kinds
that need them plus an actionable installation hint. Always exits 0 (it is
a report, not an assertion); `healthy` is true only when no check errored
**and** at least one toolchain is usable.

## `raven-pico toolchains`

Lists detected toolchains and versions. JSON payload separates `found` /
`missing` counts (platform-inapplicable tools are `n/a` and counted in
neither). Exit code 0.

## `raven-pico init [PATH]`

Writes a commented `raven-pico.toml` template.

| Flag | Effect |
|------|--------|
| `--kind <KIND>` | Pre-select the kind instead of auto-detecting |
| `--force` | Explicitly overwrite an existing configuration file |

Without `--force`, an existing file is never touched (exit 10). Only the
config file is created; all other files are left alone. The template embeds
a security notice: configured commands execute project code.

---

## Configuration

`raven-pico.toml` (schema version 1), usually at the project root. All
sections optional; unknown fields are rejected; unknown `schema_version`
values are refused with an upgrade hint.

```toml
schema_version = 1

[project]
name = "My Project"          # optional display name
kind = "rust"                # optional detection override

[build]                      # overrides the native build adapter
command = "make"             # program — executed directly, NO shell
args = ["all"]               # one argument per element
cwd = "sub/dir"              # optional, relative to this file

[test]
command = "python"
args = ["-m", "pytest", "-q"]

[run]
command = "./target/debug/app"
args = []

[clean]
command = "make"
args = ["clean"]

[cmake]
build_dir = "build"          # default: "build"

[env]                        # extra env for everything Pico spawns here
KEY = "value"
```

Notes:

* `command` accepts an absolute path, a path relative to the project root,
  or a bare name resolved from `PATH`.
* `[build]`/`[test]`/`[run]`/`[clean]` take precedence over the native
  adapter for that operation.
* A config that defines at least one command (without `kind`) makes Pico
  treat the project as `custom`.
* Configured commands do not accept `--release` (they have no profile
  concept); Pico refuses rather than guessing.
* Values in `[env]` are never included in logs or reports.
