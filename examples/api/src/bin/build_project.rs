//! Example: drive a full build operation through the backend exactly like the
//! CLI does — detect, select, load config, execute, and inspect the outcome.
//!
//! This is the skeleton a future Raven Studio / Raven Code integration would
//! reuse (with a ProgressSink instead of a terminal).
//!
//! Run: `cargo run -p raven-api-examples --bin build_project -- <path>`

use raven_build::proc::path_dirs;
use raven_build::{CancelToken, Op, OpContext, Profile, StdioMode, execute_operation};
use raven_core::progress::SilentSink;
use raven_core::report::Report;
use raven_projects::{DetectOptions, detect, select};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());

    // 1. Detect the project (searches parents by default).
    let detection = match detect(std::path::Path::new(&path), &DetectOptions::default()) {
        Ok(detection) => detection,
        Err(error) => {
            eprintln!("detection failed: {error}");
            std::process::exit(error.exit_code());
        }
    };
    let (kind, root) = match select(&detection, None) {
        Ok(selected) => selected,
        Err(error) => {
            eprintln!("selection failed: {error}");
            eprintln!("hint: {}", error.hint().unwrap_or_default());
            std::process::exit(error.exit_code());
        }
    };
    println!("detected {kind} at {}", root.display());

    // 2. Load the optional raven-pico.toml (broken configs are hard errors
    //    for operations).
    let config = detection
        .config
        .as_ref()
        .map(|loaded| loaded.require().expect("valid config").clone());

    // 3. Build the operation context. Replace SilentSink with your own
    //    ProgressSink implementation to stream events into a UI.
    let ctx = OpContext {
        root: root.clone(),
        kind,
        config: config.as_ref(),
        profile: Profile::Debug,
        extra_args: Vec::new(),
        env: config
            .as_ref()
            .map(|config| config.env.clone())
            .unwrap_or_default(),
        search_paths: path_dirs(),
        sink: Some(&SilentSink),
        cancel: CancelToken::new(),
        stdio: StdioMode::Capture,
    };

    // 4. Execute and inspect.
    match execute_operation(&ctx, Op::Build) {
        Ok(outcome) => {
            let success = outcome.success();
            let payload = outcome.to_payload(kind, Profile::Debug, &root);
            let report = Report::new("build", env!("CARGO_PKG_VERSION"));
            let report = if success {
                report.with_success(0)
            } else {
                report.with_failure(
                    1,
                    raven_core::report::ErrorInfo {
                        code: "tool_failed".to_string(),
                        message: "the build tool returned a non-zero exit code".to_string(),
                        hint: None,
                    },
                )
            };
            let json = serde_json::to_string_pretty(&report).expect("report serializes");
            println!("{json}");
            let _ = payload; // payload is also available standalone
        }
        Err(error) => {
            eprintln!("build failed: {error}");
            if let Some(hint) = error.hint() {
                eprintln!("hint: {hint}");
            }
            std::process::exit(error.exit_code());
        }
    }
}
