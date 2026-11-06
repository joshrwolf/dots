//! Own SSH/control fixture; native Git still owns both pack protocols.
use std::io::{Read as _, Write as _};
use std::os::unix::process::CommandExt as _;
use std::process::Command;

use dots_memory::{Error, PeerRequest, PeerResponse};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [ssh, host, remote] = args.as_slice() else {
        return Err("fixture needs ssh, host and one command".into());
    };
    if std::env::var("TERM").as_deref() != Ok("dumb") {
        return Err("native transport needs a headless terminal environment".into());
    }
    if ssh != "ssh" || host != "replica.tail.invalid" {
        return Err("unexpected fixture endpoint".into());
    }
    if remote.ends_with(" memory serve") {
        let mut input = Vec::new();
        std::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut input)?;
        if input.len() > 1024 * 1024 {
            return Err("fixture request exceeds input budget".into());
        }
        let request: PeerRequest = serde_json::from_slice(&input)?;
        let response = match dots_memory::serve_peer(request) {
            Ok(response) => response,
            Err(Error::Conflict { expected, actual }) => {
                PeerResponse::Conflict { expected, actual }
            }
            Err(error) => return Err(error.into()),
        };
        serde_json::to_writer(std::io::stdout(), &response)?;
        std::io::stdout().write_all(b"\n")?;
        return Ok(());
    }
    let mut command = Command::new("git");
    if let Some(path) = remote.strip_prefix("git-upload-pack ") {
        command.args(["upload-pack", unquote(path)?]);
    } else if let Some(path) =
        remote.strip_prefix("git -c receive.hideRefs=!refs/dots/transfer/ receive-pack ")
    {
        command.args([
            "-c",
            "receive.hideRefs=!refs/dots/transfer/",
            "receive-pack",
            unquote(path)?,
        ]);
    } else {
        return Err(format!("unexpected native fixture command: {remote}").into());
    }
    Err(command.exec().into())
}

fn unquote(path: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let path = path
        .strip_prefix('\'')
        .and_then(|path| path.strip_suffix('\''))
        .ok_or("native fixture path is not quoted")?;
    if path.contains('\'') {
        return Err("fixture intentionally uses paths without apostrophes".into());
    }
    Ok(path)
}
