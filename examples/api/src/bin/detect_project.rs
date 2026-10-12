//! Example: use `raven-projects` to detect a project and print the result as
//! the same JSON payload the CLI produces.
//!
//! Run: `cargo run -p raven-api-examples --bin detect_project -- <path>`
//!
//! Note: `DetectOptions::default()` inspects ONLY the given directory.
//! Pass `--parents` as a second argument to also search ancestors (the
//! library equivalent of the CLI's `--search-parents`).

use raven_core::path_to_string;
use raven_projects::{DetectOptions, detect};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let search_parents = args.iter().any(|arg| arg == "--parents");
    let path = args
        .into_iter()
        .find(|arg| arg != "--parents")
        .unwrap_or_else(|| ".".to_string());
    let options = DetectOptions {
        search_parents,
        ..DetectOptions::default()
    };

    match detect(std::path::Path::new(&path), &options) {
        Ok(detection) => {
            let payload = detection.to_payload();
            let json = serde_json::to_string_pretty(&payload).expect("payload serializes");
            println!("{json}");
        }
        Err(error) => {
            eprintln!("detection failed: {error}");
            if let Some(hint) = error.hint() {
                eprintln!("hint: {hint}");
            }
            std::process::exit(error.exit_code());
        }
    }
    let _ = path_to_string(std::path::Path::new("."));
}
