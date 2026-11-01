use std::io::{self, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    match dots_cli::run(std::env::args_os().skip(1), &mut io::stdout()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let error = failure.error;
            eprintln!("dots: {error:#}");
            // Parse failures stay stderr-only; runtime failures use parsed mode.
            if failure.json {
                let body = serde_json::json!({ "error": format!("{error:#}") });
                let _ = writeln!(io::stdout().lock(), "{body}");
            }
            ExitCode::FAILURE
        }
    }
}
