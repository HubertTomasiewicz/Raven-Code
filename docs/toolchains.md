# Supported toolchains and ecosystems

Pico invokes existing tools; it never installs, downloads or updates them.
This document lists what each adapter runs, how tools are discovered, and the
honest limitations of version 0.2.

## Tool discovery

* Bare program names are resolved from `PATH` in order. On Windows the probe
  tries `name`, `name.exe`, `name.cmd`, `name.bat` per directory.
* `doctor` and `toolchains` probe every known tool with a 4-second timeout;
  version parsers tolerate stdout/stderr quirks (`java -version` prints to
  stderr, `gradle --version` embeds the version in a line, ...).
* **A probe only counts when the tool exits 0** (0.2.0). Stubs that exist on
  disk but fail their version probe — e.g. the Windows Store `python3.exe`
  alias — are skipped instead of being reported as installed.
* MSBuild is special: located via `vswhere` (the supported mechanism), then
  by scanning `Program Files\Microsoft Visual Studio\<year>\<edition>\...`;
  the located binary is version-probed with `-version -nologo`. If neither
  finds it and the .NET SDK exists, SDK-style solutions fall back to
  `dotnet msbuild`.
* Windows-only tools report `n/a` on other platforms (not a fake `ok`).
* Missing tools always produce an actionable hint — never a silent failure,
  never an automatic installation. The hint names the project kinds that
  need the tool (`required for: C/C++ (CMake) projects`), and tool
  availability is never inferred from a detected project type.

## Ecosystem matrix

### Rust (Cargo) — kind `rust`

* Markers: `Cargo.toml`.
* build `cargo build [--release]` · test `cargo test [--release]` ·
  run `cargo run [--release]` · clean `cargo clean`.
* Artifacts reported: `target/debug` or `target/release`.
* Note: cargo may fetch dependencies from the network — that is the
  project's own Cargo behaviour, not something Pico initiates. Use `--locked`
  via `--tool-arg` for reproducibility.

### C/C++ (CMake) — kind `cmake`

* Markers: `CMakeLists.txt`.
* Build directory: `<root>/build` (change with `[cmake] build_dir`).
* build: configures first (`cmake -S <root> -B <build_dir>
  [-DCMAKE_BUILD_TYPE=Release]`) when `CMakeCache.txt` is missing, then
  `cmake --build <dir> [--config Release]`.
* test: `ctest --test-dir <dir> --output-on-failure [-C Release]`
  (configures first when needed).
* clean: `cmake --build <dir> --target clean`; if that fails (or the
  generator lacks a clean target) the build tree is removed — but **only**
  when `CMakeCache.txt` proves it really is a CMake tree. A `build` directory
  that is not a CMake tree is never touched.
* run: no universal default — CMake does not record which target is the
  program, and Pico will not invent an executable path. Configure `[run]`
  with your executable path; unconfigured `run` exits 9 with guidance.
* Missing CMake: detection still succeeds (it is marker-based and needs no
  toolchain); build/test then fail with `tool_missing` (exit 4) and a hint
  that names the detected kind and the installation steps.
* Limitations: Makefile-only C projects are not detected; multi-config
  generators get `--config` only on Windows-style invocations.

### Visual Studio / MSBuild — kind `visual-studio`

* Markers: `.sln`, `.slnx`, `.vcxproj`.
* build: `MSBuild.exe <file> [-p:Configuration=Debug|Release] -m -nologo`
  (solution preferred over bare project).
* clean: `-t:Clean`.
* test / run: require configuration (vstest is not integrated in 0.1;
  solutions have no single runnable output).
* Non-Windows: `dotnet msbuild` handles SDK-style solutions when the .NET
  SDK is installed; otherwise `tool_missing` with a hint.

### .NET — kind `dotnet`

* Markers: `.csproj`, `.fsproj`, `.vbproj`.
* build/test/run/clean: `dotnet build|test|run|clean -c Debug|Release`.
* Requires the .NET SDK. Global.json, workloads and NuGet restores are the
  project's own concern.

### Python — kind `python`

* Markers: `pyproject.toml`, `setup.py`, `requirements.txt`.
* build: intentionally unsupported — Python has no build step and Pico
  refuses to invent one.
* test: runs `pytest` **only when it is installed**; otherwise configure
  `[test]`.
* run: `python main.py` (prefers `python3` on Unix, `python`/`py` on
  Windows); otherwise configure `[run]`.
* clean: unsupported (recursively deleting `__pycache__` is too easy to get
  wrong); configure `[clean]` if needed.

### Godot — kind `godot`

* Markers: `project.godot`.
* run: `godot --path <root>` (needs a display or `--headless` via `[run]`).
* test: configure `[test]` (GUT / gdUnit are not assumed).
* build: configure `[build]` — exports need a preset name and output path
  from `export_presets.cfg`, which 0.1 does not parse. Example:
  `command = "godot", args = ["--headless", "--export-release", "Windows Desktop", "build/game.exe"]`.
* clean: removes the generated `.godot` cache directory only.

### Java (Maven) — kind `maven`

* Markers: `pom.xml`.
* build: `mvn package` — note the standard Maven lifecycle runs unit tests
  as part of `package`; that is Maven behaviour, kept unmodified.
* test: `mvn test` · clean: `mvn clean`.
* run: configure `[run]` (typically `mvn exec:java` when the project uses
  exec-maven-plugin).

### Java (Gradle) — kind `gradle`

* Markers: `build.gradle`, `build.gradle.kts`, `settings.gradle`,
  `settings.gradle.kts`.
* Wrapper first: `gradlew.bat` (Windows) / `gradlew` (Unix, executed via
  `sh` if it lost its executable bit); global `gradle` otherwise.
* build: `assemble` (compile + package **without** tests, so `build` and
  `test` stay orthogonal) · test: `test` · clean: `clean`.
* run: configure `[run]` (`gradle run` only exists with the application
  plugin — being explicit avoids surprises).

### Lua — kind `lua`

* Markers: `main.lua`, `conf.lua`, `*.rockspec`.
* run: `lua main.lua` (probes `lua`, `lua5.4`, `lua5.3`, `lua5.2`,
  `luajit`); LÖVE projects (`conf.lua`) get a hint to configure
  `[run] command = "love", args = ["."]`.
* build / test / clean: configure (e.g. busted for tests).

### Custom — kind `custom`

* Marker: `raven-pico.toml` defining at least one of `[build]` `[test]`
  `[run]` `[clean]`.
* Every operation maps to its configured command. Nothing else is assumed.

## Explicitly out of scope for 0.1

* Compiling, debugging, or language services (Pico is an orchestrator).
* Installing or upgrading toolchains (permanently out of scope).
* Node.js/npm, Makefile-only, .NET Framework (non-SDK), Bazel, Meson — not
  detected yet; see `docs/adding-adapters.md` to add one, or use
  `raven-pico.toml` commands today.
* Parsing `export_presets.cfg` (Godot) or `vstest` (Visual Studio).

## Verifying your setup

```console
$ raven-pico doctor
$ raven-pico toolchains
```
