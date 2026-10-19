use std::io::{self, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let json = args.iter().any(|arg| arg == "--json");
    match dots_cli::run(args) {
        Ok(output) => match io::stdout().lock().write_all(output.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("dots: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("dots: {error:#}");
            // Scripts in JSON mode get the same message as data. Argument errors
            // stay stderr-only: the request itself was not valid JSON-mode use.
            if json && error.downcast_ref::<clap::Error>().is_none() {
                let body = serde_json::json!({ "error": format!("{error:#}") });
                let _ = writeln!(io::stdout().lock(), "{body}");
            }
            ExitCode::FAILURE
        }
    }
}
