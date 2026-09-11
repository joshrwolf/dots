//! Newline-delimited JSON over herdr's Unix socket.
//!
//! The methods themselves live in [`crate::api`]; this module is only the
//! transport.
//!
//! Every call opens its own connection. The server answers one
//! non-subscription request and closes, so a client that keeps a connection to
//! pipeline a second request gets one answer and a hang.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Error, Result, env};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn parse_worktree_timeout(value: &str) -> Result<Duration> {
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| (1..=86400).contains(seconds))
        .map(Duration::from_secs)
        .ok_or_else(|| Error::InvalidWorktreeTimeout {
            value: value.to_owned(),
        })
}

/// A hung server would otherwise leave a popup blank with no way to tell
/// whether it is working. Requests that legitimately perform longer work
/// override this deadline.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(3);

/// A request body that names its own method.
pub(crate) trait Request: Serialize {
    const METHOD: &'static str;

    /// How long to wait for the reply.
    ///
    /// The default suits a call that answers immediately. A request that asks
    /// the server to *block* — for a status, for matching output — must
    /// override this to outlast its own `timeout_ms`, or the socket read
    /// deadline fires first and a wait that was working reports a timeout
    /// against the wrong clock.
    fn timeout(&self) -> Option<Duration> {
        Some(TIMEOUT)
    }
}

/// Slack added to a blocking request's own deadline, so the server's timeout
/// is always the one that fires and the caller gets its documented answer.
pub(crate) const SLACK: Duration = Duration::from_secs(2);

/// Read deadline for a request that blocks server-side for `timeout_ms`.
pub(crate) fn blocking_timeout(timeout_ms: Option<u64>) -> Option<Duration> {
    timeout_ms.map(|ms| Duration::from_millis(ms) + SLACK)
}

/// Largest accepted JSON frame, excluding its newline delimiter.
///
/// Responses are local control-plane data. Capping them prevents a faulty or
/// hostile peer from growing `pending` without bound while preserving ample
/// room for large snapshots and pane reads.
pub(crate) const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// A request whose reply is decoded.
///
/// `TAG` is the `type` discriminant herdr puts on the result. Pairing it with
/// the method and the reply type on one impl is what stops a caller from
/// matching `worktree.list` against the wrong tag — the failure that shape
/// produces is a runtime [`Error::WrongResult`], not a compile error.
pub(crate) trait Query: Request {
    const TAG: &'static str;
    type Reply: DeserializeOwned;
}

#[derive(Debug, Clone)]
pub struct Client {
    path: PathBuf,
    worktree_timeout: Duration,
}

impl Client {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            worktree_timeout: Duration::from_secs(10 * 60),
        }
    }

    /// The Herdr server endpoint this client is bound to.
    ///
    /// Long-lived per-session processes can use it as identity when they need
    /// to prevent duplicate instances without conflating separate servers.
    pub fn endpoint(&self) -> &std::path::Path {
        &self.path
    }

    /// Identity of the concrete Unix socket instance, not merely its reusable
    /// pathname. Persisted runtime bindings use this to reject workspace and
    /// pane IDs recycled by a later Herdr server process.
    pub fn server_id(&self) -> Result<String> {
        let metadata = std::fs::metadata(&self.path).map_err(|source| Error::InspectEndpoint {
            path: self.path.clone(),
            source,
        })?;
        Ok(format!(
            "{}@{}:{}",
            self.path.display(),
            metadata.dev(),
            metadata.ino()
        ))
    }

    /// The server this plugin was invoked by.
    pub fn from_env() -> Result<Self> {
        let mut client = Self::new(env::socket_path()?);
        if let Some(value) = env::optional_string("HERDR_WORKTREE_TIMEOUT_SECS")? {
            client.worktree_timeout = parse_worktree_timeout(&value)?;
        }
        Ok(client)
    }

    pub(crate) fn request_timeout<R: Request>(&self, request: &R) -> Option<Duration> {
        match R::METHOD {
            "worktree.create" | "worktree.open" | "worktree.remove" | "worktree.list" => {
                Some(self.worktree_timeout)
            }
            _ => request.timeout(),
        }
    }

    /// A framed connection for a caller that needs one to outlive a request.
    pub(crate) fn connection(&self) -> Result<Connection> {
        UnixStream::connect(&self.path)
            .map(Connection::new)
            .map_err(|source| Error::Connect {
                path: self.path.clone(),
                source,
            })
    }

    /// Sends `request` and decodes its reply, checking the result tag first.
    ///
    /// The tag check is what turns a herdr release that reshapes a result into
    /// a named error rather than a struct of defaulted fields.
    pub(crate) fn call<Q: Query>(&self, request: &Q) -> Result<Q::Reply> {
        let result = self.exchange(Q::METHOD, request, self.request_timeout(request))?;
        let actual = result
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if actual != Q::TAG {
            return Err(Error::WrongResult {
                method: Q::METHOD.to_owned(),
                expected: Q::TAG,
                actual: actual.to_owned(),
            });
        }
        serde_json::from_value(result).map_err(|source| Error::Decode {
            method: Q::METHOD.to_owned(),
            source,
        })
    }

    /// Sends `request` for its effect. A rejection still surfaces; the result
    /// shape does not have to be modelled.
    pub(crate) fn invoke<R: Request>(&self, request: &R) -> Result<()> {
        self.exchange(R::METHOD, request, request.timeout())
            .map(|_| ())
    }

    /// One request, one response, one connection — the `result` object with its
    /// tag intact.
    fn exchange<P: Serialize>(
        &self,
        method: &'static str,
        params: &P,
        timeout: Option<Duration>,
    ) -> Result<Value> {
        let id = format!(
            "{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        );
        let mut connection = self.connection()?;
        connection.prepare_read_timeout(method, timeout)?;
        connection.send_request(method, &id, params)?;
        let Some(response) = connection.frame_prepared(method, timeout)? else {
            let Some(timeout) = timeout else {
                return Err(Error::NoResponse {
                    method: method.to_owned(),
                });
            };
            return Err(Error::Timeout {
                method: method.to_owned(),
                timeout,
            });
        };
        decode_response(method, &id, response)
    }
}

/// A newline-framed socket, including bytes read past the last complete frame.
///
/// Calls use one instance for one request. Event subscriptions retain theirs
/// for the life of the stream. Keeping framing here gives both paths identical
/// encoding, deadline, partial-read, and hang-up behaviour.
#[derive(Debug)]
pub(crate) struct Connection {
    stream: UnixStream,
    pending: Vec<u8>,
}

impl Connection {
    fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            pending: Vec::new(),
        }
    }

    pub(crate) fn send_request<P: Serialize>(
        &mut self,
        method: &'static str,
        id: &str,
        params: &P,
    ) -> Result<()> {
        let mut framed =
            serde_json::to_vec(&RequestEnvelope { id, method, params }).map_err(|source| {
                Error::Encode {
                    method: method.to_owned(),
                    source,
                }
            })?;
        framed.push(b'\n');
        self.stream
            .write_all(&framed)
            .and_then(|()| self.stream.flush())
            .map_err(|source| Error::Io {
                method: method.to_owned(),
                source,
            })
    }

    /// Installs the first read deadline before a fast one-shot server can
    /// answer and half-close the socket. Darwin may reject changing socket
    /// options after that half-close, even while the reply remains buffered.
    pub(crate) fn prepare_read_timeout(
        &self,
        method: &'static str,
        timeout: Option<Duration>,
    ) -> Result<()> {
        self.stream
            .set_read_timeout(timeout)
            .map_err(|source| Error::Io {
                method: method.to_owned(),
                source,
            })
    }

    /// Reads one complete frame, or `None` if the deadline passes first.
    ///
    /// The deadline covers the whole frame. In particular, a peer cannot keep
    /// extending the call by sending an unterminated trickle of bytes.
    pub(crate) fn frame(
        &mut self,
        method: &'static str,
        timeout: Option<Duration>,
    ) -> Result<Option<Value>> {
        self.frame_with_limit(method, timeout, MAX_FRAME_BYTES, false)
    }

    pub(crate) fn frame_prepared(
        &mut self,
        method: &'static str,
        timeout: Option<Duration>,
    ) -> Result<Option<Value>> {
        self.frame_with_limit(method, timeout, MAX_FRAME_BYTES, true)
    }

    fn frame_with_limit(
        &mut self,
        method: &'static str,
        timeout: Option<Duration>,
        maximum: usize,
        mut timeout_is_prepared: bool,
    ) -> Result<Option<Value>> {
        let started = std::time::Instant::now();
        loop {
            if let Some(at) = self.pending.iter().position(|byte| *byte == b'\n') {
                if at > maximum {
                    return Err(Error::FrameTooLarge {
                        method: method.to_owned(),
                        maximum,
                    });
                }
                let line: Vec<u8> = self.pending.drain(..=at).collect();
                let line = line.strip_suffix(b"\n").unwrap_or(&line);
                if line.is_empty() {
                    continue;
                }
                return serde_json::from_slice(line)
                    .map(Some)
                    .map_err(|source| Error::Decode {
                        method: method.to_owned(),
                        source,
                    });
            }

            if self.pending.len() > maximum {
                return Err(Error::FrameTooLarge {
                    method: method.to_owned(),
                    maximum,
                });
            }

            let left = timeout.map(|limit| limit.saturating_sub(started.elapsed()));
            if left.is_some_and(|duration| duration.is_zero()) {
                return Ok(None);
            }
            if !timeout_is_prepared && let Err(source) = self.stream.set_read_timeout(left) {
                // Darwin rejects setting a timeout on a peer that has already
                // hung up. Confirm that precise case with a nonblocking read;
                // every other setup failure is still surfaced instead of
                // being silently discarded.
                if self.stream.set_nonblocking(true).is_ok() {
                    let mut byte = [0_u8; 1];
                    if matches!(self.stream.read(&mut byte), Ok(0)) {
                        return Err(if self.pending.is_empty() {
                            Error::NoResponse {
                                method: method.to_owned(),
                            }
                        } else {
                            Error::TruncatedFrame {
                                method: method.to_owned(),
                            }
                        });
                    }
                }
                return Err(Error::Io {
                    method: method.to_owned(),
                    source,
                });
            }
            timeout_is_prepared = false;

            let mut chunk = [0_u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    return Err(if self.pending.is_empty() {
                        Error::NoResponse {
                            method: method.to_owned(),
                        }
                    } else {
                        Error::TruncatedFrame {
                            method: method.to_owned(),
                        }
                    });
                }
                Ok(read) => {
                    let Some(bytes) = chunk.get(..read) else {
                        return Err(Error::Io {
                            method: method.to_owned(),
                            source: std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "socket returned more bytes than its read buffer holds",
                            ),
                        });
                    };
                    self.pending.extend_from_slice(bytes);
                }
                Err(source)
                    if matches!(
                        source.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None);
                }
                Err(source) => {
                    return Err(Error::Io {
                        method: method.to_owned(),
                        source,
                    });
                }
            }
        }
    }
}

#[derive(Serialize)]
struct RequestEnvelope<'a, P> {
    id: &'a str,
    method: &'static str,
    params: &'a P,
}

/// Decodes the response envelope shared by calls and subscription startup.
///
/// A response belongs to this request only when its id matches, and it is an
/// envelope only when it contains exactly one of `result` and `error`.
pub(crate) fn decode_response(
    method: &'static str,
    expected_id: &str,
    response: Value,
) -> Result<Value> {
    let response: Response = serde_json::from_value(response).map_err(|source| Error::Decode {
        method: method.to_owned(),
        source,
    })?;
    if response.id != expected_id {
        return Err(Error::IdMismatch {
            expected: expected_id.to_owned(),
            actual: response.id,
        });
    }
    match (response.result, response.error) {
        (Some(result), None) => Ok(result),
        (None, Some(error)) => Err(Error::Rejected {
            method: method.to_owned(),
            code: error.code,
            message: error.message,
        }),
        (Some(_), Some(_)) | (None, None) => Err(Error::Malformed {
            method: method.to_owned(),
        }),
    }
}

/// A success carries `result` and a failure carries `error`; both carry `id`.
///
/// Two options rather than an enum: `#[serde(flatten)]` over an externally
/// tagged enum buffers the whole map and then fails to pick a variant, which
/// presents as `missing field result` on a response that has one.
#[derive(serde::Deserialize)]
struct Response {
    id: String,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<ErrorBody>,
}

#[derive(serde::Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[cfg(test)]
mod tests {
    #[test]
    fn worktree_deadline_configuration_is_bounded() {
        assert_eq!(
            super::parse_worktree_timeout("1200").unwrap(),
            std::time::Duration::from_secs(1200)
        );
        for value in ["0", "-1", "86401", "forever", "1.5", ""] {
            assert!(super::parse_worktree_timeout(value).is_err(), "{value}");
        }
    }
    use super::*;

    fn decode(line: &str) -> Response {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn a_success_envelope_yields_the_result_object() {
        let response = decode(r#"{"id":"7","result":{"type":"pong","version":"0.8.2"}}"#);
        assert_eq!(response.id, "7");
        assert_eq!(
            response.result.as_ref().and_then(|r| r.get("type")),
            Some(&Value::String("pong".to_owned()))
        );
        assert!(response.error.is_none());
    }

    #[test]
    fn an_error_envelope_yields_the_code_and_message() {
        let response = decode(r#"{"id":"7","error":{"code":"ui_busy","message":"modal open"}}"#);
        assert!(response.result.is_none());
        let error = response.error.unwrap();
        assert_eq!(error.code, "ui_busy");
        assert_eq!(error.message, "modal open");
    }

    #[test]
    fn a_rejection_with_no_id_decodes_before_validation() {
        let response = decode(
            r#"{"id":"","error":{"code":"invalid_request","message":"expected a sequence"}}"#,
        );
        assert!(response.id.is_empty());
        assert_eq!(
            response.error.map(|e| e.message),
            Some("expected a sequence".to_owned())
        );
    }

    #[test]
    fn a_large_result_still_parses() {
        let big: String = std::iter::repeat_n("x", 8000).collect();
        let response = decode(&format!(
            r#"{{"id":"7","result":{{"type":"t","v":"{big}"}}}}"#
        ));
        assert!(response.result.is_some());
    }

    mod wire {
        use super::TIMEOUT;
        use crate::api::{
            AgentPrompt, AgentPromptWait, AgentRef, AgentStart, AgentStatus, AgentWait, Ping,
        };
        use crate::testing::{Server, Step};
        use crate::{Error, socket::Request};

        const PONG: &str =
            r#"{"id":"{id}","result":{"type":"pong","version":"0.9.0","protocol":22}}"#;

        /// One round trip end to end: id generated, request framed, reply
        /// matched to it, tag checked, body decoded. Every unit test above this
        /// exercises only the last step.
        #[test]
        fn a_matching_reply_is_decoded() {
            let server = Server::start(vec![Step::reply(PONG)]);
            let first = server.client().server_id().expect("server identity");
            let second = server.client().server_id().expect("stable server identity");
            assert_eq!(first, second);
            let pong = server.client().ping().expect("a pong");
            assert_eq!(pong.version, "0.9.0");
            assert_eq!(pong.protocol, crate::api::PROTOCOL);
        }

        #[test]
        fn a_reply_carrying_the_wrong_tag_is_refused_before_decoding() {
            let server = Server::start(vec![Step::reply(
                r#"{"id":"{id}","result":{"type":"worktree_list","worktrees":[]}}"#,
            )]);
            let result = server.client().ping();
            assert!(
                matches!(
                    &result,
                    Err(Error::WrongResult { expected: "pong", actual, .. }) if actual == "worktree_list"
                ),
                "got {result:?}"
            );
        }

        #[test]
        fn an_answer_with_neither_result_nor_error_is_named() {
            let server = Server::start(vec![Step::reply(r#"{"id":"{id}"}"#)]);
            assert!(matches!(
                server.client().ping(),
                Err(Error::Malformed { .. })
            ));
        }

        #[test]
        fn a_response_for_a_different_request_is_refused() {
            let server = Server::start(vec![Step::line(
                r#"{"id":"someone-else","result":{"type":"pong","version":"0.9.0","protocol":22}}"#,
            )]);
            assert!(matches!(
                server.client().ping(),
                Err(Error::IdMismatch { actual, .. }) if actual == "someone-else"
            ));
        }

        #[test]
        fn a_rejection_with_no_id_is_refused_as_a_different_response() {
            let server = Server::start(vec![Step::line(
                r#"{"id":"","error":{"code":"invalid_request","message":"expected a sequence"}}"#,
            )]);
            assert!(matches!(
                server.client().ping(),
                Err(Error::IdMismatch { actual, .. }) if actual.is_empty()
            ));
        }

        #[test]
        fn an_answer_with_both_result_and_error_is_malformed() {
            let server = Server::start(vec![Step::reply(
                r#"{"id":"{id}","result":{"type":"pong","version":"0.9.0","protocol":22},"error":{"code":"bad","message":"also an error"}}"#,
            )]);
            assert!(matches!(
                server.client().ping(),
                Err(Error::Malformed { .. })
            ));
        }

        #[test]
        fn a_server_that_answers_nothing_times_out_rather_than_hanging() {
            let server = Server::start(vec![Step::Wait(std::time::Duration::from_secs(30))]);
            let started = std::time::Instant::now();
            assert!(matches!(server.client().ping(), Err(Error::Timeout { .. })));
            assert!(
                started.elapsed() < TIMEOUT * 2,
                "waited {:?}",
                started.elapsed()
            );
        }

        #[test]
        fn a_server_that_hangs_up_without_answering_says_so() {
            let server = Server::start(vec![Step::Close]);
            assert!(matches!(
                server.client().ping(),
                Err(Error::NoResponse { .. })
            ));
        }

        #[test]
        fn a_missing_socket_names_the_path_it_tried() {
            let client = super::Client::new("/nonexistent/herdr.sock");
            assert!(matches!(
                client.server_id(),
                Err(Error::InspectEndpoint { .. })
            ));
            assert!(
                matches!(client.ping(), Err(Error::Connect { path, .. }) if path.ends_with("herdr.sock"))
            );
        }

        /// A blocking request must set the socket deadline from its own
        /// `timeout_ms`. The default would cut a long wait short and report a
        /// timeout against the transport rather than the answer being waited on.
        #[test]
        fn a_blocking_request_outlasts_the_default_deadline() {
            let pane = crate::PaneId::new("w1:p1");
            let wait = AgentWait {
                target: AgentRef::Pane(&pane),
                until: &[AgentStatus::Done],
                timeout_ms: Some(60_000),
            };
            assert!(
                wait.timeout() > Some(TIMEOUT),
                "a 60s wait would be cut off at {TIMEOUT:?}"
            );
            assert_eq!(wait.timeout(), Some(std::time::Duration::from_secs(62)));
            assert_eq!(
                Ping {}.timeout(),
                Some(TIMEOUT),
                "an immediate call is unchanged"
            );
        }

        #[test]
        fn omitted_server_timeouts_have_operation_specific_transport_deadlines() {
            let pane = crate::PaneId::new("w1:p1");
            let wait = AgentWait {
                target: AgentRef::Pane(&pane),
                until: &[],
                timeout_ms: None,
            };
            assert_eq!(wait.timeout(), None, "agent.wait is indefinite");

            let prompt = AgentPrompt {
                target: AgentRef::Pane(&pane),
                text: "continue",
                wait: Some(AgentPromptWait {
                    until: &[],
                    timeout_ms: None,
                }),
            };
            assert_eq!(prompt.timeout(), None, "prompt-and-wait is indefinite");

            let start = AgentStart {
                name: "worker",
                agent_kind: "codex",
                pane_id: &pane,
                timeout_ms: None,
            };
            assert_eq!(
                start.timeout(),
                Some(std::time::Duration::from_secs(32)),
                "agent.start uses the server's 30s default plus transport slack"
            );
        }

        #[test]
        fn an_oversized_unterminated_frame_is_refused() {
            const LIMIT: usize = 64;
            let oversized = "x".repeat(LIMIT + 1);
            let server = Server::start(vec![Step::bytes(oversized)]);
            let mut connection = server.client().connection().unwrap();
            connection.send_request("ping", "test", &Ping {}).unwrap();
            let result = connection.frame_with_limit("ping", Some(TIMEOUT), LIMIT, false);
            assert!(
                matches!(
                    &result,
                    Err(Error::FrameTooLarge { maximum, .. }) if *maximum == LIMIT
                ),
                "got {result:?}"
            );
        }
    }
}
