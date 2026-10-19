fn main() -> std::process::ExitCode {
    match herdr_issue::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("herdr-issue: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
