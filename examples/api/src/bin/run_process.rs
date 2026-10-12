//! Example: run a process through the reusable execution layer with output
//! capture, a timeout and cancellation support.
//!
//! Run: `cargo run -p raven-api-examples --bin run_process`

use std::time::Duration;

use raven_build::proc::{CancelToken, ProcessSpec, run_process};

fn main() {
    let cancel = CancelToken::new();

    // Run `git --version` (or any tool) with a 5 second timeout.
    let spec = ProcessSpec::new("git")
        .arg("--version")
        .timeout(Duration::from_secs(5));

    let result = run_process(&spec, &cancel);

    match &result.spawn_error {
        Some(error) => println!("launch failed: {error}"),
        None => {
            println!("program:    {}", result.program);
            println!("exit code:  {:?}", result.exit_code);
            println!("duration:   {} ms", result.duration.as_millis());
            println!("stdout:     {}", result.stdout.trim());
        }
    }

    // Cancellation is cooperative: call `cancel.cancel()` from any thread
    // (e.g. a UI button) and the watchdog kills the child.
    drop(cancel);
}
