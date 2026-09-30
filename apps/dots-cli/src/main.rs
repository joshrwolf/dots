use std::io::{self, Write as _};
use std::process::ExitCode;

fn main() -> ExitCode {
    match dots_cli::run(std::env::args_os().skip(1)) {
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
            ExitCode::FAILURE
        }
    }
}
