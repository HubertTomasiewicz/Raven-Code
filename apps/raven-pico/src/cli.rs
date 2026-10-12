//! Command-line interface definition (clap).

use clap::ValueEnum;

/// When to colorize terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Colorize only when the target stream is a terminal and NO_COLOR is
    /// unset (default).
    Auto,
    /// Always colorize.
    Always,
    /// Never colorize (CI friendly).
    Never,
}

#[derive(Debug, clap::Parser)]
#[command(
    name = "raven-pico",
    version,
    about = "Raven Pico — the Raven ecosystem's lightweight build backend and CLI",
    long_about = "\
Raven Pico detects projects, invokes existing compilers and build systems, \
runs tests and programs, and returns structured results.

It is the reusable backend of the Raven ecosystem (Raven Studio, Raven Code, \
Raven Hub) and a useful terminal tool on its own. Pico never installs \
toolchains, never modifies system settings and always works with the native \
project files already in your repository.",
    after_help = EXAMPLES,
)]
pub struct Cli {
    /// Print a versioned JSON report on stdout (progress goes to stderr).
    #[arg(long, global = true)]
    pub json: bool,

    /// When to colorize output: auto, always, never.
    #[arg(long, global = true, value_enum)]
    pub color: Option<ColorChoice>,

    /// Suppress progress output (errors still print).
    #[arg(short = 'q', long, global = true)]
    pub quiet: bool,

    /// When the given directory has no project markers, search parent
    /// directories (bounded) for a project root. Default: off — a
    /// markerless directory is reported as "not detected" instead of
    /// silently resolving to an ancestor project.
    #[arg(long, global = true)]
    pub search_parents: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Identify the project type and root directory.
    Detect {
        /// Directory to inspect (defaults to the current directory).
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
    },
    /// Invoke the project's build system.
    Build {
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
        /// Build in release mode (adapter-dependent).
        #[arg(long)]
        release: bool,
        /// Force the project kind instead of relying on detection.
        #[arg(long)]
        kind: Option<String>,
        /// Extra argument passed through to the build tool (repeatable).
        #[arg(long = "tool-arg")]
        tool_args: Vec<String>,
    },
    /// Run the project's test suite (adapter-dependent).
    Test {
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long = "tool-arg")]
        tool_args: Vec<String>,
    },
    /// Run a supported executable or project command.
    Run {
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long = "tool-arg")]
        tool_args: Vec<String>,
    },
    /// Invoke the native clean operation where supported.
    Clean {
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
        #[arg(long)]
        release: bool,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long = "tool-arg")]
        tool_args: Vec<String>,
    },
    /// Check the environment and report missing or misconfigured tools.
    Doctor,
    /// List detected toolchains and their versions.
    Toolchains,
    /// Create a minimal raven-pico.toml configuration (never overwrites
    /// without --force).
    Init {
        /// Directory for the new configuration (defaults to the current
        /// directory).
        #[arg(default_value = ".")]
        path: std::path::PathBuf,
        /// Pre-select the project kind instead of auto-detecting.
        #[arg(long)]
        kind: Option<String>,
        /// Explicitly overwrite an existing raven-pico.toml.
        #[arg(long)]
        force: bool,
    },
}

const EXAMPLES: &str = "\
Examples:
  raven-pico detect .            identify the project in the current directory
  raven-pico build . --release   build with the detected build system
  raven-pico test                run the project's tests
  raven-pico run                 run the project
  raven-pico build . --json      machine-readable report (schema version 1)
  raven-pico detect . --kind rust   force a project kind
  raven-pico detect src --search-parents   find the project root above src
  raven-pico doctor              verify installed toolchains
  raven-pico toolchains          list detected toolchains and versions
  raven-pico init                create a commented raven-pico.toml

Discovery: only the given directory is inspected unless --search-parents is
passed; a missing path is a usage error (exit 2), a directory without
project markers is 'not detected' (exit 3).

Exit codes: 0 success, 1 tool failed, 2 usage, 3 not detected, 4 tool missing,
5 ambiguous project, 6 invalid config, 7 cancelled, 8 internal, 9 unsupported,
10 conflict. See docs/cli-reference.md.";
