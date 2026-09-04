//! A scriptable stand-in for the herdr server.
//!
//! Enabled by the `testing` feature, so a plugin's own tests can drive the
//! half of it that talks to herdr — the dispatch, the hand-off — against a
//! server that answers what the script says and records what it was asked.
//!
//! Live probing is what establishes the protocol; this is what pins the client
//! to it. The two are not interchangeable — a fake reproduces whatever its
//! author believed, so it can only catch a client that disagrees with the
//! script, never a script that disagrees with herdr.
//!
//! What it is for is the half a real server will not do on request: a frame
//! arriving in two pieces, a deadline landing mid-frame, a hang-up between
//! frames, a rejection. Those paths are unreachable from a healthy socket.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Client;

/// One action in a server's script.
#[derive(Debug, Clone)]
pub enum Step {
    /// Raw bytes, written as one `write`. Not newline-terminated for you: a
    /// step holding half a frame is the point.
    Bytes(Cow<'static, str>),
    /// A whole frame, newline included.
    Line(String),
    /// Reads one request and answers it, substituting `{id}` with the id the
    /// client generated.
    ///
    /// Without this a script cannot exercise any success path: the client
    /// derives a fresh id per call and refuses a response carrying another,
    /// so a fixed reply only ever tests the mismatch check.
    Reply(Cow<'static, str>),
    Wait(Duration),
    /// Hang up immediately.
    Close,
}

impl Step {
    pub fn bytes(text: impl Into<Cow<'static, str>>) -> Self {
        Self::Bytes(text.into())
    }

    pub fn line(text: impl Into<String>) -> Self {
        Self::Line(format!("{}\n", text.into()))
    }

    /// Answers one request from either borrowed fixture text or an owned,
    /// dynamically constructed response. `{id}` is replaced with the request
    /// id before the frame is sent.
    pub fn reply(text: impl Into<Cow<'static, str>>) -> Self {
        Self::Reply(text.into())
    }
}

/// A server running its script on a background thread.
///
/// Dropping it removes the socket. The thread is detached: a script that
/// blocks on a client that never reads would otherwise hang the test suite
/// rather than failing it.
///
/// A reply to a normal request ends that connection and the next reply accepts
/// the next one, matching herdr's one-request-per-connection transport. A
/// reply to `events.subscribe` instead retains the connection and runs the
/// remaining stream steps on it. When a script runs out on an open stream, the
/// connection stays alive until the `Server` is dropped.
///
/// Every request a [`Step::Reply`] answered is kept, so a test can assert on
/// what the code under test *sent* and not only on how it handled the answer.
#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    finished: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl Server {
    pub fn start(script: Vec<Step>) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "herdrkit-fake-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        #[expect(
            clippy::expect_used,
            reason = "test scaffolding: a fake that cannot bind has nothing to return but a panic, and the message names the step"
        )]
        let listener = UnixListener::bind(&path).expect("bind the fake socket");
        let finished = Arc::new(AtomicBool::new(false));
        let hung_up = Arc::clone(&finished);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);

        std::thread::spawn(move || {
            let mut script = VecDeque::from(script);
            if listener.set_nonblocking(true).is_err() {
                return;
            }
            while !hung_up.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Darwin can propagate O_NONBLOCK from the listener to
                        // an accepted Unix socket. The scripted connection is
                        // synchronous: an immediate WouldBlock while waiting
                        // for the client's request is not a hang-up.
                        if stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        run_connection(stream, &mut script, &seen, &hung_up);
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return,
                }
            }
        });

        Self {
            path,
            finished,
            requests,
        }
    }

    pub fn client(&self) -> Client {
        Client::new(&self.path)
    }

    /// The `method` and `params` of every request answered so far, in order.
    ///
    /// The id is left out because the client generates it; a test asserting on
    /// it would be asserting on a counter.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .map(|seen| {
                seen.iter()
                    .map(|value| RecordedRequest {
                        method: value
                            .get("method")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        params: value.get("params").cloned().unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn run_connection(
    mut stream: std::os::unix::net::UnixStream,
    script: &mut VecDeque<Step>,
    seen: &Mutex<Vec<serde_json::Value>>,
    finished: &AtomicBool,
) {
    while let Some(step) = script.pop_front() {
        let (wrote, close_after_reply) = match step {
            Step::Bytes(raw) => (stream.write_all(raw.as_bytes()), false),
            Step::Line(line) => (stream.write_all(line.as_bytes()), false),
            Step::Reply(template) => {
                let Some(request) = read_request(&mut stream) else {
                    return;
                };
                let parsed: serde_json::Value =
                    serde_json::from_slice(&request).unwrap_or_default();
                let id = parsed
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let subscription = parsed.get("method").and_then(serde_json::Value::as_str)
                    == Some("events.subscribe");
                if let Ok(mut seen) = seen.lock() {
                    seen.push(parsed);
                }
                let answer = template.replace("{id}", &id);
                (
                    stream.write_all(format!("{answer}\n").as_bytes()),
                    !subscription,
                )
            }
            Step::Wait(how_long) => {
                let started = std::time::Instant::now();
                while !finished.load(Ordering::Relaxed) {
                    let left = how_long.saturating_sub(started.elapsed());
                    if left.is_zero() {
                        break;
                    }
                    std::thread::sleep(left.min(Duration::from_millis(10)));
                }
                if finished.load(Ordering::Relaxed) {
                    return;
                }
                (Ok(()), false)
            }
            Step::Close => {
                close_after_flushing(&mut stream);
                return;
            }
        };
        if wrote.and_then(|()| stream.flush()).is_err() {
            return;
        }
        if close_after_reply {
            close_after_flushing(&mut stream);
            return;
        }
    }

    while !finished.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Reads the single request a fake connection carries without cloning the
/// socket. A descriptor clone can fail under a highly parallel nextest run,
/// closing an otherwise valid accepted connection and surfacing as a random
/// client-side `BrokenPipe`.
fn read_request(stream: &mut std::os::unix::net::UnixStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    loop {
        let mut byte = [0_u8; 1];
        let read = match stream.read(&mut byte) {
            Err(source) if source.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
            Ok(read) => read,
        };
        match read {
            0 => return None,
            _ if byte[0] == b'\n' => return Some(request),
            _ if request.len() < crate::socket::MAX_FRAME_BYTES => request.push(byte[0]),
            _ => return None,
        }
    }
}

/// Half-close first so the client can read the complete reply without racing
/// a full close, then retain the read side until the client is finished.
fn close_after_flushing(stream: &mut std::os::unix::net::UnixStream) {
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = std::io::copy(stream, &mut std::io::sink());
}

/// One request the fake answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    pub method: String,
    pub params: serde_json::Value,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A subscribe acknowledgement, which every event script has to open with.
///
/// A [`Step::Reply`] rather than a bare line so that the server reads the
/// request before answering. Answering blind races the client's write: a script
/// ending in [`Step::Close`] could hang up first, and the client's `write` then
/// fails with `BrokenPipe` instead of reaching the behaviour under test.
pub fn subscribed() -> Step {
    Step::reply(r#"{"id":"{id}","result":{"type":"subscription_started"}}"#)
}

/// An event frame for `kind` naming `pane`.
pub fn pane_event(kind: &str, pane: &str) -> Step {
    Step::line(format!(
        r#"{{"event":"{kind}","data":{{"type":"{kind}","pane_id":"{pane}"}}}}"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PONG: &str = r#"{"id":"{id}","result":{"type":"pong","version":"0.8.2","protocol":20}}"#;

    #[test]
    fn sequential_replies_accept_one_connection_per_request() {
        let server = Server::start(vec![Step::reply(PONG), Step::reply(PONG)]);
        let client = server.client();

        client.ping().unwrap();
        client.ping().unwrap();

        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|request| request.method == "ping"));
    }
}
