//! Terminal presentation: colored status labels, the progress sink and the
//! human-readable rendering of reports.
//!
//! JSON mode guarantees machine-readable stdout: the report is the only thing
//! printed there, and progress goes to stderr (suppressed unless `--verbose`
//! behaviour is requested; warnings always shown).

use std::io::IsTerminal;

use raven_core::progress::{ProgressEvent, ProgressSink};
use raven_core::report::{CheckDto, OpPayload, Report};

use crate::cli::ColorChoice;

/// Output mode derived from CLI flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputMode {
    pub json: bool,
    pub quiet: bool,
    pub color: bool,
}

impl OutputMode {
    pub fn from_flags(json: bool, quiet: bool, color: Option<ColorChoice>) -> OutputMode {
        let color = match color {
            Some(ColorChoice::Always) => true,
            Some(ColorChoice::Never) => false,
            Some(ColorChoice::Auto) | None => {
                let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
                !no_color && std::io::stdout().is_terminal()
            }
        };
        OutputMode { json, quiet, color }
    }
}

fn paint(enabled: bool, code: &str, text: &str) -> String {
    if enabled {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn green(enabled: bool, text: &str) -> String {
    paint(enabled, "32", text)
}

fn red(enabled: bool, text: &str) -> String {
    paint(enabled, "31", text)
}

fn yellow(enabled: bool, text: &str) -> String {
    paint(enabled, "33", text)
}

fn dim(enabled: bool, text: &str) -> String {
    paint(enabled, "2", text)
}

fn bold(enabled: bool, text: &str) -> String {
    paint(enabled, "1", text)
}

/// Progress sink bound to the terminal.
pub struct TerminalSink {
    mode: OutputMode,
}

impl TerminalSink {
    pub fn new(mode: OutputMode) -> TerminalSink {
        TerminalSink { mode }
    }
}

impl ProgressSink for TerminalSink {
    fn on_event(&self, event: &ProgressEvent) {
        if self.mode.quiet {
            return;
        }
        if self.mode.json {
            // In JSON mode stdout must stay pure; only warnings reach stderr.
            if let ProgressEvent::Warning { message } = event {
                eprintln!("raven-pico: warning: {message}");
            }
            return;
        }
        eprintln!("  {}", dim(self.mode.color, &event.to_line()));
    }
}

/// Renders reports for humans.
pub struct Reporter {
    mode: OutputMode,
}

impl Reporter {
    pub fn new(mode: OutputMode) -> Reporter {
        Reporter { mode }
    }

    /// Prints the final report: JSON on stdout, or a human summary.
    pub fn emit(&self, report: &Report) {
        if self.mode.json {
            let text = serde_json::to_string_pretty(report).unwrap_or_else(|_| "{}".to_string());
            println!("{text}");
            return;
        }
        if self.mode.quiet {
            return;
        }
        self.render_text(report);
    }

    fn render_text(&self, report: &Report) {
        let c = self.mode.color;
        println!("{} {}", bold(c, "Raven Pico"), dim(c, &report.tool_version));
        match report.command.as_str() {
            "detect" => self.render_detect(report),
            "build" | "test" | "run" | "clean" => self.render_op(report),
            "doctor" => self.render_doctor(report),
            "toolchains" => self.render_toolchains(report),
            "init" => self.render_init(report),
            _ => {}
        }
        if let Some(error) = &report.error {
            println!("  {} {}", red(c, "error"), error.message);
            if let Some(hint) = &error.hint {
                println!("  {} {}", yellow(c, "hint"), hint);
            }
        }
    }

    fn render_detect(&self, report: &Report) {
        let c = self.mode.color;
        let Some(payload) = &report.detect else {
            return;
        };
        println!("  {} {}", dim(c, "searched"), payload.searched);
        match (&payload.primary, payload.ambiguous) {
            (Some(primary), _) => {
                println!(
                    "  {} {} {}",
                    dim(c, "kind"),
                    primary.display,
                    dim(c, &format!("[{} confidence]", primary.confidence))
                );
                println!("  {} {}", dim(c, "root"), primary.root);
                if !primary.markers.is_empty() {
                    println!("  {} {}", dim(c, "markers"), primary.markers.join(", "));
                }
                if payload.searched_upwards {
                    println!(
                        "  {} project root found by searching parent directories",
                        dim(c, "note")
                    );
                }
                if payload.candidates.len() > 1 {
                    let others: Vec<&str> = payload
                        .candidates
                        .iter()
                        .filter(|candidate| Some(*candidate) != payload.primary.as_ref())
                        .map(|candidate| candidate.kind.as_str())
                        .collect();
                    println!(
                        "  {} other candidates: {} (use --kind or raven-pico.toml to choose)",
                        yellow(c, "note"),
                        others.join(", ")
                    );
                }
            }
            (None, true) => {
                let kinds: Vec<&str> = payload.candidates.iter().map(|c| c.kind.as_str()).collect();
                println!(
                    "  {} multiple project types detected: {}",
                    yellow(c, "ambiguous"),
                    kinds.join(", ")
                );
                println!(
                    "  {} pass --kind <KIND> or set [project] kind in raven-pico.toml",
                    dim(c, "hint")
                );
            }
            (None, false) => {
                // A path error (exit 2) already explains itself in the error
                // block below — printing "no project detected" here would
                // misdescribe a missing/invalid path as a detection result.
                let path_error = report
                    .error
                    .as_ref()
                    .is_some_and(|error| error.code == "invalid_path");
                if !path_error {
                    println!(
                        "  {} no recognizable project detected",
                        red(c, "no project")
                    );
                }
            }
        }
        if let Some(config) = &payload.config {
            println!("  {} {} [{}]", dim(c, "config"), config.path, config.status);
            if let Some(message) = &config.message {
                println!("  {} {}", yellow(c, "config"), message);
            }
        }
        let status = status_label(report, c);
        println!("  {} {}", dim(c, "status"), status);
    }

    fn render_op(&self, report: &Report) {
        let c = self.mode.color;
        let payload = op_payload(report);
        let Some(payload) = payload else { return };
        println!(
            "  {} {} {}",
            dim(c, "project"),
            payload.kind,
            dim(c, &payload.root)
        );
        if let Some(tool) = &payload.tool {
            let version = tool
                .version
                .as_deref()
                .map(|version| format!(" {version}"))
                .unwrap_or_default();
            println!(
                "  {} {}{}",
                dim(c, "tool"),
                tool.name,
                dim(c, version.as_str())
            );
        }
        if let Some(command) = &payload.command {
            println!("  {} {}", dim(c, "command"), command.join(" "));
        }
        if report.success {
            let duration = payload
                .duration_ms
                .map(|ms| format!(" in {} ms", ms))
                .unwrap_or_default();
            println!(
                "  {} {}",
                dim(c, "status"),
                green(c, &format!("OK{}", duration))
            );
        } else {
            let reason = match (payload.cancelled, payload.timed_out, payload.exit_code) {
                (true, _, _) => "CANCELLED".to_string(),
                (_, true, _) => "TIMED OUT".to_string(),
                (_, _, Some(code)) => format!("FAILED (exit code {code})"),
                _ => "FAILED".to_string(),
            };
            println!("  {} {}", dim(c, "status"), red(c, &reason));
            print_output_tail(c, "stderr", &payload.stderr);
            print_output_tail(c, "stdout", &payload.stdout);
        }
        if let Some(note) = &payload.note {
            println!("  {} {}", yellow(c, "note"), note);
        }
    }

    fn render_doctor(&self, report: &Report) {
        let c = self.mode.color;
        let Some(payload) = &report.doctor else {
            return;
        };
        println!("  {} {} ({})", dim(c, "system"), payload.os, payload.arch);
        render_checks(self.mode, &payload.checks);
        let health = if payload.healthy {
            green(c, "healthy")
        } else {
            yellow(c, "attention needed")
        };
        println!("  {} {}", dim(c, "overall"), health);
    }

    fn render_toolchains(&self, report: &Report) {
        let c = self.mode.color;
        let Some(payload) = &report.toolchains else {
            return;
        };
        render_checks(self.mode, &payload.checks);
        println!(
            "  {} {} found, {} missing",
            dim(c, "summary"),
            payload.found,
            payload.missing
        );
    }

    fn render_init(&self, report: &Report) {
        let c = self.mode.color;
        let Some(payload) = &report.init else { return };
        println!("  {} {}", dim(c, "created"), payload.config_path);
        if let Some(kind) = &payload.kind {
            println!("  {} pre-selected project kind: {}", dim(c, "kind"), kind);
        }
        println!(
            "  {} commands in raven-pico.toml execute project code when you run \
             raven-pico build/test/run/clean — review the file",
            yellow(c, "security")
        );
    }
}

fn op_payload(report: &Report) -> Option<&OpPayload> {
    report
        .build
        .as_ref()
        .or(report.test.as_ref())
        .or(report.run.as_ref())
        .or(report.clean.as_ref())
}

fn status_label(report: &Report, color: bool) -> String {
    if report.success {
        green(color, "OK")
    } else {
        red(color, "FAILED")
    }
}

/// Prints the last 30 lines of a captured stream (failure diagnostics).
fn print_output_tail(color: bool, label: &str, output: &Option<String>) {
    let Some(output) = output else { return };
    if output.trim().is_empty() {
        return;
    }
    println!("  {} (last lines):", dim(color, label));
    for line in output
        .lines()
        .rev()
        .take(30)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        println!("    {line}");
    }
}

/// Renders doctor / toolchain checks.
fn render_checks(mode: OutputMode, checks: &[CheckDto]) {
    let c = mode.color;
    for check in checks {
        let painted = match check.status.as_str() {
            "ok" => green(c, "ok"),
            "missing" => yellow(c, "missing"),
            "n/a" => dim(c, "n/a "),
            _ => red(c, "error"),
        };
        let version = check
            .version
            .as_deref()
            .map(|version| format!(" {version}"))
            .unwrap_or_default();
        let path = check
            .path
            .as_deref()
            .map(|path| format!(" ({path})"))
            .unwrap_or_default();
        println!("  {} {}{}{}", dim(c, "·"), painted, version, dim(c, &path));
        if let Some(detail) = &check.detail {
            println!("      {}", dim(c, detail));
        }
        if let Some(hint) = &check.hint {
            println!("      {} {}", yellow(c, "hint"), hint);
        }
    }
}
