//! Process-lifecycle roles for this crate's tests; never installed.
//!
//! Each role is selected by the first argument. Descendant roles re-execute this
//! binary, so the tests need no shell or interpreter.

use std::error::Error;
use std::fs::File;
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Long enough to outlive any deadline under test, short enough not to leak.
const HOLD: Duration = Duration::from_secs(30);

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result {
    let mut args = std::env::args().skip(1);
    let role = args.next().unwrap_or_default();
    let mut arg = move || args.next().ok_or("missing argument");
    match role.as_str() {
        // emit STDOUT STDERR CODE: write both streams, then exit with CODE.
        "emit" => {
            let (stdout, stderr, code) = (arg()?, arg()?, arg()?.parse()?);
            std::io::stdout().write_all(stdout.as_bytes())?;
            std::io::stderr().write_all(stderr.as_bytes())?;
            std::process::exit(code);
        }
        // write STREAM BYTES exit|hold: write BYTES zeros, then exit or keep running.
        "write" => {
            let (stream, length, then) = (arg()?, arg()?.parse()?, arg()?);
            let bytes = vec![0_u8; length];
            match stream.as_str() {
                "stdout" => std::io::stdout().write_all(&bytes)?,
                _ => std::io::stderr().write_all(&bytes)?,
            }
            if then == "hold" {
                std::thread::sleep(HOLD);
            }
        }
        // spew: write both streams until killed; deadlocks unless both are drained.
        "spew" => loop {
            std::io::stdout().write_all(&[b'x'; 4096])?;
            std::io::stderr().write_all(&[b'x'; 4096])?;
        },
        // group READY LOCK: hold LOCK in a same-group descendant that keeps the
        // pipes, then record this leader's PID in READY and keep running.
        "group" => {
            let (ready, lock) = (arg()?, arg()?);
            let mut holder = Command::new(std::env::current_exe()?)
                .args(["hold-lock", &lock])
                .stdout(Stdio::piped())
                .spawn()?;
            let mut line = String::new();
            BufReader::new(holder.stdout.take().ok_or("holder stdout")?).read_line(&mut line)?;
            std::fs::write(ready, std::process::id().to_string())?;
            std::thread::sleep(HOLD);
        }
        "hold-lock" => {
            let lock = File::create(arg()?)?;
            lock.lock()?;
            println!("locked");
            std::thread::sleep(HOLD);
        }
        // detach PIDFILE: leave a pipe holder in a new session, then succeed.
        "detach" => {
            let mut holder = Command::new(std::env::current_exe()?)
                .args(["detached", &arg()?])
                .stdin(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            let mut line = String::new();
            BufReader::new(holder.stderr.take().ok_or("holder stderr")?).read_line(&mut line)?;
        }
        "detached" => {
            nix::unistd::setsid()?;
            std::fs::write(arg()?, std::process::id().to_string())?;
            eprintln!("detached");
            std::thread::sleep(HOLD);
        }
        // orphan FIFO: leave a same-group descendant with closed pipes, then succeed.
        "orphan" => {
            Command::new(std::env::current_exe()?)
                .args(["survive", &arg()?])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
        }
        // Opening a FIFO for writing blocks until the test opens it for reading.
        "survive" => File::options()
            .write(true)
            .open(arg()?)?
            .write_all(b"survived")?,
        _ => return Err(format!("unknown role {role:?}").into()),
    }
    Ok(())
}
