//! Adapter for C/C++ projects driven by CMake (and CTest for tests).
//!
//! Behaviour:
//!
//! * The build directory defaults to `<root>/build` and can be changed with
//!   `[cmake] build_dir` in `raven-pico.toml`.
//! * If the build directory is not configured yet (`CMakeCache.txt` missing),
//!   `cmake -S <root> -B <build_dir>` runs first, then `cmake --build`.
//! * Tests use CTest against the configured build directory.
//! * `clean` only ever touches a directory that contains `CMakeCache.txt`;
//!   anything else is refused to protect unrelated user files.
//! * `run` has no universal meaning for CMake projects: configure `[run]` in
//!   `raven-pico.toml` (e.g. the executable under the build directory).

use std::path::{Path, PathBuf};

use raven_core::error::PicoError;
use raven_core::kind::ProjectKind;
use raven_core::report::ToolInfoDto;

use crate::proc::ProcessSpec;
use crate::{Capabilities, OpContext, OperationOutcome, Profile, Support};

use super::support;

pub struct CmakeAdapter;

/// Tool-missing hint: makes explicit that detection succeeded and only the
/// tool is absent — the operation cannot proceed, nothing was silently
/// mis-detected.
const CMAKE_HINT: &str = "this project was detected as C/C++ (CMake), but the build cannot \
proceed because its required tool is not installed or not on PATH. \
Install CMake from https://cmake.org/download/ (ctest ships with CMake), \
or define [build]/[test] commands in raven-pico.toml";

impl CmakeAdapter {
    fn cmake(&self, ctx: &OpContext) -> Result<(PathBuf, ToolInfoDto), PicoError> {
        support::resolve(ctx, "cmake", &["--version"], CMAKE_HINT)
    }

    fn build_dir(&self, ctx: &OpContext) -> PathBuf {
        let relative = ctx
            .config
            .and_then(|config| config.cmake.as_ref())
            .map(|section| section.build_dir.clone())
            .unwrap_or_else(|| "build".to_string());
        ctx.root.join(relative)
    }

    fn is_configured(build_dir: &Path) -> bool {
        build_dir.join("CMakeCache.txt").is_file()
    }

    fn configure_spec(&self, ctx: &OpContext, cmake_path: &Path, build_dir: &Path) -> ProcessSpec {
        let mut spec = ProcessSpec::new(cmake_path.display().to_string())
            .arg("-S")
            .arg(ctx.root.display().to_string())
            .arg("-B")
            .arg(build_dir.display().to_string());
        if ctx.profile == Profile::Release {
            spec = spec.arg("-DCMAKE_BUILD_TYPE=Release");
        }
        spec.cwd(ctx.root.clone())
    }

    fn compile_spec(&self, ctx: &OpContext, cmake_path: &Path, build_dir: &Path) -> ProcessSpec {
        let mut spec = ProcessSpec::new(cmake_path.display().to_string())
            .arg("--build")
            .arg(build_dir.display().to_string());
        if ctx.profile == Profile::Release {
            spec = spec.arg("--config").arg("Release");
        }
        support::with_extra_args(spec, ctx).cwd(ctx.root.clone())
    }
}

impl crate::BuildAdapter for CmakeAdapter {
    fn kind(&self) -> ProjectKind {
        ProjectKind::CMake
    }

    fn capabilities(&self, _ctx: &OpContext) -> Capabilities {
        Capabilities {
            build: Support::Available,
            test: Support::Available,
            run: Support::ConfigRequired {
                hint: Some(
                    "CMake does not record which target is the program to run; Raven Pico will \
                     not invent an executable path. Set [run] command in raven-pico.toml to the \
                     executable under your build directory (e.g. build/hello_cmake)"
                        .to_string(),
                ),
            },
            clean: Support::Available,
        }
    }

    fn build(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.cmake(ctx)?;
        let build_dir = self.build_dir(ctx);
        let mut note: Option<String> = None;
        if !Self::is_configured(&build_dir) {
            ctx.emit(raven_core::progress::ProgressEvent::Stage {
                message: "configuring CMake build directory".to_string(),
            });
            let configured = support::execute(
                ctx,
                tool.clone(),
                self.configure_spec(ctx, &path, &build_dir),
                Vec::new(),
                None,
            )?;
            if !configured.success() {
                return Ok(configured);
            }
            note = Some(format!(
                "configured build directory first ({}); it did not exist",
                build_dir.display()
            ));
        }
        support::execute(
            ctx,
            tool,
            self.compile_spec(ctx, &path, &build_dir),
            vec![build_dir],
            note,
        )
    }

    fn test(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let (path, tool) = self.cmake(ctx)?;
        let build_dir = self.build_dir(ctx);
        if !Self::is_configured(&build_dir) {
            // Configure (without building) so CTest has something to query;
            // if the project has not been built yet, CTest reports 0 tests —
            // which is honest, not an error.
            ctx.emit(raven_core::progress::ProgressEvent::Stage {
                message: "configuring CMake build directory for CTest".to_string(),
            });
            let configured = support::execute(
                ctx,
                tool.clone(),
                self.configure_spec(ctx, &path, &build_dir),
                Vec::new(),
                None,
            )?;
            if !configured.success() {
                return Ok(configured);
            }
        }
        let (ctest_path, ctest_tool) =
            super::support::resolve(ctx, "ctest", &["--version"], CMAKE_HINT)?;
        let mut spec = ProcessSpec::new(ctest_path.display().to_string())
            .arg("--test-dir")
            .arg(build_dir.display().to_string())
            .arg("--output-on-failure");
        if ctx.profile == Profile::Release {
            spec = spec.arg("-C").arg("Release");
        }
        let spec = support::with_extra_args(spec, ctx).cwd(ctx.root.clone());
        support::execute(ctx, ctest_tool, spec, Vec::new(), None)
    }

    fn run(&self, _ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        // execute_operation already handled a configured [run] section; a
        // native run has no honest default for CMake: CMakeCache.txt names no
        // single runnable program, and guessing a binary path would be a
        // lie. Require an explicit configuration instead.
        Err(PicoError::UnsupportedOperation {
            operation: "run".to_string(),
            kind: ProjectKind::CMake.display_name().to_string(),
            reason: "CMake does not record which target to run and Raven Pico will not \
                     invent an executable path; configure [run] in raven-pico.toml"
                .to_string(),
        })
    }

    fn clean(&self, ctx: &OpContext) -> Result<OperationOutcome, PicoError> {
        let build_dir = self.build_dir(ctx);
        if !build_dir.exists() {
            return Ok(OperationOutcome {
                tool: None,
                command: None,
                cwd: Some(ctx.root.clone()),
                result: None,
                artifacts: Vec::new(),
                note: Some(format!(
                    "nothing to clean: build directory {} does not exist",
                    build_dir.display()
                )),
            });
        }
        if !Self::is_configured(&build_dir) {
            return Err(PicoError::UnsupportedOperation {
                operation: "clean".to_string(),
                kind: ProjectKind::CMake.display_name().to_string(),
                reason: format!(
                    "refusing to remove {}: it exists but is not a CMake build tree \
                     (no CMakeCache.txt); remove it manually if intended",
                    build_dir.display()
                ),
            });
        }
        // Prefer the native clean target; fall back to removing the build
        // tree when the generator lacks a clean target.
        let (path, tool) = self.cmake(ctx)?;
        let spec = ProcessSpec::new(path.display().to_string())
            .arg("--build")
            .arg(build_dir.display().to_string())
            .arg("--target")
            .arg("clean")
            .cwd(ctx.root.clone());
        let outcome = support::execute(ctx, tool, spec, Vec::new(), None)?;
        if outcome.success() {
            return Ok(outcome);
        }
        // Some configurations (missing generator) fail; removing the build
        // tree is still safe because we verified CMakeCache.txt above.
        std::fs::remove_dir_all(&build_dir)?;
        Ok(OperationOutcome {
            tool: None,
            command: outcome.command,
            cwd: Some(ctx.root.clone()),
            result: None,
            artifacts: Vec::new(),
            note: Some(format!("removed CMake build tree {}", build_dir.display())),
        })
    }
}
