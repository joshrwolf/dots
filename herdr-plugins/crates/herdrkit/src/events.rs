//! A long-lived subscription to herdr's change stream.
//!
//! This is the one place a connection outlives a single request. [`Client`]
//! opens a connection per call because the server answers one request and
//! closes; `events.subscribe` instead replies once and then writes unsolicited
//! frames until the connection drops.
//!
//! # Changes are a wake-up, not a log
//!
//! Subscribing replays a backlog. It is the same backlog on every subscribe,
//! it contains events for objects that no longer exist, and a frame carries no
//! timestamp or sequence number — so **nothing in a frame distinguishes a
//! stale event from a live one.**
//!
//! So the contract is deliberately weak: a [`Change`] means *something of this
//! kind moved, go re-read the truth*. It is not a record to act on directly. A
//! loop that spawned an agent per frame would fire on history; a loop that
//! re-reads [`crate::api::Snapshot`] and diffs it cannot, and pays only a
//! redundant read when a frame turns out to be uninteresting.
//!
//! That contract is also what makes the backlog a non-problem rather than
//! something to filter. **Reconcile once before the loop**, and the first
//! batch — history and all — costs one redundant read:
//!
//! ```no_run
//! # use herdrkit::{Client, events::{Events, Subscription}};
//! # use std::time::Duration;
//! # fn reconcile(_: &Client) {}
//! # fn main() -> herdrkit::Result<()> {
//! let client = Client::from_env()?;
//! let mut events = Events::subscribe(&client, &Subscription::workspace_topology())?;
//! reconcile(&client);
//! loop {
//!     // `None` is a quiet interval rather than a failure, so it doubles as
//!     // the tick for anything that also wants refreshing on a schedule.
//!     let _ = events.changes(Duration::from_secs(60))?;
//!     reconcile(&client);
//! }
//! # }
//! ```

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::{AgentStatus, ReadSource};
use crate::{Client, Error, PaneId, Result, TabId, WorkspaceId};

/// How long the stream must be silent before a burst counts as finished.
///
/// herdr coalesces on a roughly 100ms cadence — a measured 95ms from an action
/// to its frame, and backlog frames about 100ms apart — so this clears one
/// tick with margin. Too short and one user action reports as several changes;
/// too long and every reaction waits on it.
const COALESCE: Duration = Duration::from_millis(250);

/// Longest a single batch may keep collecting, however busy the stream is.
///
/// Without a cap, [`Events::changes`] returns only once the stream goes quiet,
/// and the replayed backlog does not: it arrives at roughly one frame per
/// 100ms, which is inside the coalescing window, so a busy session's backlog
/// held the first batch open for **13 seconds** before any work could start.
const MAX_BATCH: Duration = Duration::from_secs(2);

/// What to be told about.
///
/// Every variant is one entry in the `subscriptions` array. Ask for the
/// coarsest set that covers what you re-read: an extra kind costs a redundant
/// read, a missing one costs a change you never hear about.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[non_exhaustive]
pub enum Subscription<'a> {
    #[serde(rename = "workspace.created")]
    WorkspaceCreated,
    #[serde(rename = "workspace.updated")]
    WorkspaceUpdated,
    #[serde(rename = "workspace.metadata_updated")]
    WorkspaceMetadataUpdated,
    #[serde(rename = "workspace.renamed")]
    WorkspaceRenamed,
    #[serde(rename = "workspace.moved")]
    WorkspaceMoved,
    #[serde(rename = "workspace.reordered")]
    WorkspaceReordered,
    #[serde(rename = "workspace.closed")]
    WorkspaceClosed,

    /// **Not edge-triggered.** Measured at roughly 8 frames a second on an idle
    /// session, indefinitely — herdr reports the current focus on its tick
    /// rather than on a change. Subscribing pins a watcher at that rate.
    /// Read the focus out of [`crate::api::Snapshot`] instead.
    #[serde(rename = "workspace.focused")]
    WorkspaceFocused,

    #[serde(rename = "worktree.created")]
    WorktreeCreated,
    #[serde(rename = "worktree.opened")]
    WorktreeOpened,
    #[serde(rename = "worktree.removed")]
    WorktreeRemoved,

    #[serde(rename = "tab.created")]
    TabCreated,
    #[serde(rename = "tab.closed")]
    TabClosed,

    /// **Not edge-triggered**, like [`Self::WorkspaceFocused`]: any activity
    /// starts a run of roughly ten frames a second.
    #[serde(rename = "tab.focused")]
    TabFocused,
    #[serde(rename = "tab.renamed")]
    TabRenamed,
    #[serde(rename = "tab.moved")]
    TabMoved,

    #[serde(rename = "pane.created")]
    PaneCreated,
    #[serde(rename = "pane.closed")]
    PaneClosed,
    #[serde(rename = "pane.updated")]
    PaneUpdated,
    #[serde(rename = "pane.focused")]
    PaneFocused,
    #[serde(rename = "pane.moved")]
    PaneMoved,
    #[serde(rename = "pane.exited")]
    PaneExited,
    #[serde(rename = "pane.agent_detected")]
    PaneAgentDetected,

    #[serde(rename = "layout.updated")]
    LayoutUpdated,

    /// Only for `pane_id`, and only when its status becomes `agent_status`.
    /// Omitting `agent_status` reports every transition of that pane.
    #[serde(rename = "pane.agent_status_changed")]
    PaneAgentStatusChanged {
        pane_id: &'a PaneId,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_status: Option<AgentStatus>,
    },

    /// Server-side matching against a pane's output, so the plugin is not
    /// reading and scanning a screenful per tick.
    #[serde(rename = "pane.output_matched")]
    PaneOutputMatched {
        pane_id: &'a PaneId,
        source: ReadSource,
        #[serde(rename = "match")]
        pattern: OutputMatch<'a>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lines: Option<u32>,
        strip_ansi: bool,
    },

    #[serde(rename = "pane.scroll_changed")]
    PaneScrollChanged { pane_id: &'a PaneId },
}

impl Subscription<'_> {
    /// Everything that changes which repos and branches are open — what a
    /// plugin tracking per-workspace state re-reads on.
    ///
    /// The focus subscriptions are deliberately absent. They are not
    /// edge-triggered, and a watcher that included them would reconcile several
    /// times a second forever on a session nobody is touching.
    #[must_use]
    pub fn workspace_topology() -> Vec<Self> {
        vec![
            Self::WorkspaceCreated,
            Self::WorkspaceUpdated,
            Self::WorkspaceRenamed,
            Self::WorkspaceClosed,
            Self::WorktreeCreated,
            Self::WorktreeOpened,
            Self::WorktreeRemoved,
        ]
    }

    /// Everything that changes the pane and agent topology, focus excepted for
    /// the same reason as [`Self::workspace_topology`].
    #[must_use]
    pub fn panes() -> Vec<Self> {
        vec![
            Self::PaneCreated,
            Self::PaneClosed,
            Self::PaneExited,
            Self::PaneAgentDetected,
            Self::TabCreated,
            Self::TabClosed,
        ]
    }
}

/// How the server should match a pane's output.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum OutputMatch<'a> {
    Substring(&'a str),
    Regex(&'a str),
}

/// What kind of thing moved.
///
/// The stream names bare subscriptions in `snake_case` (`tab_created`) and the
/// parameterised ones in dotted form (`pane.output_matched`), so these renames
/// are not uniform, and the one kind reachable both ways carries both
/// spellings so a caller matches it once. [`EventKind::Unknown`] absorbs a kind
/// added by a later herdr rather than failing the whole frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum EventKind {
    #[serde(rename = "workspace_created")]
    WorkspaceCreated,
    #[serde(rename = "workspace_updated")]
    WorkspaceUpdated,
    #[serde(rename = "workspace_metadata_updated")]
    WorkspaceMetadataUpdated,
    #[serde(rename = "workspace_renamed")]
    WorkspaceRenamed,
    #[serde(rename = "workspace_moved")]
    WorkspaceMoved,
    #[serde(rename = "workspace_reordered")]
    WorkspaceReordered,
    #[serde(rename = "workspace_closed")]
    WorkspaceClosed,
    #[serde(rename = "workspace_focused")]
    WorkspaceFocused,
    #[serde(rename = "worktree_created")]
    WorktreeCreated,
    #[serde(rename = "worktree_opened")]
    WorktreeOpened,
    #[serde(rename = "worktree_removed")]
    WorktreeRemoved,
    #[serde(rename = "tab_created")]
    TabCreated,
    #[serde(rename = "tab_closed")]
    TabClosed,
    #[serde(rename = "tab_renamed")]
    TabRenamed,
    #[serde(rename = "tab_moved")]
    TabMoved,
    #[serde(rename = "tab_focused")]
    TabFocused,
    #[serde(rename = "pane_created")]
    PaneCreated,
    #[serde(rename = "pane_closed")]
    PaneClosed,
    #[serde(rename = "pane_updated")]
    PaneUpdated,
    #[serde(rename = "pane_focused")]
    PaneFocused,
    #[serde(rename = "pane_moved")]
    PaneMoved,
    #[serde(rename = "pane_exited")]
    PaneExited,
    #[serde(rename = "pane_output_changed")]
    PaneOutputChanged,
    #[serde(rename = "pane_agent_detected")]
    PaneAgentDetected,
    #[serde(
        rename = "pane_agent_status_changed",
        alias = "pane.agent_status_changed"
    )]
    PaneAgentStatusChanged,
    #[serde(rename = "layout_updated")]
    LayoutUpdated,
    #[serde(rename = "pane.output_matched")]
    PaneOutputMatched,
    #[serde(rename = "pane.scroll_changed")]
    PaneScrollChanged,
    #[serde(other)]
    Unknown,
}

/// One reported change: what kind, and whatever it named.
///
/// The ids are hoisted out of wherever the frame put them — `data.tab_id` on a
/// close, `data.tab.tab_id` on a create — so a caller does not branch on the
/// kind to find out which thing moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: EventKind,
    pub workspace_id: Option<WorkspaceId>,
    pub tab_id: Option<TabId>,
    pub pane_id: Option<PaneId>,
    pub agent_status: Option<AgentStatus>,
}

/// A subscription and the connection it lives on.
#[derive(Debug)]
pub struct Events {
    connection: crate::socket::Connection,
}

#[derive(Serialize)]
struct SubscribeParams<'a> {
    subscriptions: &'a [Subscription<'a>],
}

impl Events {
    /// Opens the subscription. Returns as soon as the server acknowledges it.
    ///
    /// The replay backlog is left in the stream rather than drained, so this is
    /// fast and the first [`Self::changes`] is where the history shows up.
    /// Draining it here instead cost 13 seconds of startup and bought nothing:
    /// a caller that reconciles cannot tell a stale frame from a live one, and
    /// does not need to.
    pub fn subscribe(client: &Client, subscriptions: &[Subscription<'_>]) -> Result<Self> {
        const METHOD: &str = "events.subscribe";
        let mut events = Self {
            connection: client.connection()?,
        };
        events
            .connection
            .prepare_read_timeout(METHOD, Some(crate::socket::TIMEOUT))?;
        events
            .connection
            .send_request(METHOD, "subscribe", &SubscribeParams { subscriptions })?;

        let started = events
            .connection
            .frame_prepared(METHOD, Some(crate::socket::TIMEOUT))?
            .ok_or_else(|| Error::Timeout {
                method: METHOD.to_owned(),
                timeout: crate::socket::TIMEOUT,
            })?;
        let result = crate::socket::decode_response(METHOD, "subscribe", started)?;
        if result.get("type").and_then(Value::as_str) != Some("subscription_started") {
            return Err(Error::WrongResult {
                method: METHOD.to_owned(),
                expected: "subscription_started",
                actual: result
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            });
        }

        Ok(events)
    }

    /// Blocks until something changes, then collects the rest of the burst.
    ///
    /// `Ok(None)` means `timeout` passed with nothing to report. That is an
    /// ordinary outcome, not an error, so a caller that also refreshes on a
    /// schedule uses it as its tick rather than needing a second timer.
    ///
    /// One user action can produce several frames — closing a tab reports the
    /// tab, its panes and a focus change — so the return is a batch. Collapse
    /// it before acting; treating each frame as separate work does the same job
    /// three times.
    ///
    /// A batch is cut off at `MAX_BATCH` however much is still arriving, so a
    /// stream that never falls quiet cannot starve the caller.
    pub fn changes(&mut self, timeout: Duration) -> Result<Option<Vec<Change>>> {
        const METHOD: &str = "events.subscribe";
        let Some(first) = self.connection.frame(METHOD, Some(timeout))? else {
            return Ok(None);
        };
        let cutoff = Instant::now() + MAX_BATCH;
        let mut batch = vec![change(first)?];
        loop {
            let quiet = COALESCE.min(cutoff.saturating_duration_since(Instant::now()));
            if quiet.is_zero() {
                break;
            }
            match self.connection.frame(METHOD, Some(quiet))? {
                Some(value) => batch.push(change(value)?),
                None => break,
            }
        }
        Ok(Some(batch))
    }
}

/// An event frame received after the subscription acknowledgement.
#[derive(Deserialize)]
struct Frame {
    event: EventKind,
    data: Ids,
}

/// The ids a frame might carry, at either of the two depths herdr puts them.
#[derive(Debug, Default, Deserialize)]
struct Ids {
    #[serde(default)]
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    tab_id: Option<TabId>,
    #[serde(default)]
    pane_id: Option<PaneId>,
    #[serde(default)]
    agent_status: Option<AgentStatus>,
    #[serde(default)]
    workspace: Option<Box<Ids>>,
    #[serde(default)]
    tab: Option<Box<Ids>>,
    #[serde(default)]
    pane: Option<Box<Ids>>,
    #[serde(default)]
    read: Option<Box<Ids>>,
}

impl Ids {
    fn flatten(self) -> Change {
        let nested = [self.workspace, self.tab, self.pane, self.read];
        let mut change = Change {
            kind: EventKind::Unknown,
            workspace_id: self.workspace_id,
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            agent_status: self.agent_status,
        };
        for inner in nested.into_iter().flatten() {
            change.workspace_id = change.workspace_id.or(inner.workspace_id);
            change.tab_id = change.tab_id.or(inner.tab_id);
            change.pane_id = change.pane_id.or(inner.pane_id);
            change.agent_status = change.agent_status.or(inner.agent_status);
        }
        change
    }
}

/// Decode one event frame without discarding malformed protocol data.
fn change(frame: Value) -> Result<Change> {
    let frame: Frame = serde_json::from_value(frame).map_err(|source| Error::Decode {
        method: "events.subscribe".to_owned(),
        source,
    })?;
    let mut change = frame.data.flatten();
    change.kind = frame.event;
    Ok(change)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> Option<Change> {
        change(serde_json::from_str(line).unwrap()).ok()
    }

    #[test]
    fn a_subscription_serialises_as_a_tagged_object() {
        let json = serde_json::to_string(&Subscription::WorkspaceFocused).unwrap();
        assert_eq!(json, r#"{"type":"workspace.focused"}"#);
    }

    #[test]
    fn a_parameterised_subscription_carries_its_pane_and_pattern() {
        let pane = PaneId::new("w2:p1");
        let json = serde_json::to_string(&Subscription::PaneOutputMatched {
            pane_id: &pane,
            source: ReadSource::RecentUnwrapped,
            pattern: OutputMatch::Regex("FAIL"),
            lines: None,
            strip_ansi: true,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"type":"pane.output_matched","pane_id":"w2:p1","source":"recent_unwrapped","match":{"type":"regex","value":"FAIL"},"strip_ansi":true}"#,
            "an absent `lines` must be omitted, not sent as null"
        );
    }

    /// A create nests the id under the object; a close puts it at the top.
    /// Callers must not have to know which, so both land in the same field.
    #[test]
    fn an_id_is_found_at_either_depth() {
        let created = parsed(
            r#"{"event":"tab_created","data":{"type":"tab_created","tab":{"tab_id":"w2:t9","workspace_id":"w2"}}}"#,
        )
        .unwrap();
        assert_eq!(created.kind, EventKind::TabCreated);
        assert_eq!(created.tab_id, Some(TabId::new("w2:t9")));
        assert_eq!(created.workspace_id, Some(WorkspaceId::new("w2")));

        let closed =
            parsed(r#"{"event":"tab_closed","data":{"type":"tab_closed","tab_id":"w2:t9"}}"#)
                .unwrap();
        assert_eq!(closed.tab_id, Some(TabId::new("w2:t9")));
        assert_eq!(closed.workspace_id, None);
    }

    #[test]
    fn a_status_change_carries_the_status() {
        let change = parsed(
            r#"{"event":"pane_agent_status_changed","data":{"type":"pane_agent_status_changed","pane_id":"w2:p8","agent_status":"working"}}"#,
        )
        .unwrap();
        assert_eq!(change.kind, EventKind::PaneAgentStatusChanged);
        assert_eq!(change.agent_status, Some(AgentStatus::Working));
        // The parameterised subscription reports the same transition in dotted
        // form, and a caller must not have to match two variants for it.
        let dotted = parsed(
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w2:p8","agent_status":"done"}}"#,
        )
        .unwrap();
        assert_eq!(dotted.kind, EventKind::PaneAgentStatusChanged);
    }

    /// The parameterised subscriptions are reported in dotted form while the
    /// rest are `snake_case`, so both spellings have to resolve.
    #[test]
    fn both_naming_conventions_for_a_kind_resolve() {
        assert_eq!(
            parsed(r#"{"event":"pane.output_matched","data":{"pane_id":"w2:p8"}}"#).map(|c| c.kind),
            Some(EventKind::PaneOutputMatched)
        );
        assert_eq!(
            parsed(r#"{"event":"pane_output_changed","data":{"pane_id":"w2:p8"}}"#).map(|c| c.kind),
            Some(EventKind::PaneOutputChanged)
        );
    }

    /// A herdr that grows an event kind must not break a running plugin.
    #[test]
    fn an_unrecognised_kind_is_reported_rather_than_failing_the_frame() {
        let change = parsed(r#"{"event":"quantum_entangled","data":{"pane_id":"w2:p8"}}"#).unwrap();
        assert_eq!(change.kind, EventKind::Unknown);
        assert_eq!(change.pane_id, Some(PaneId::new("w2:p8")));
    }

    #[test]
    fn the_acknowledgement_is_not_a_change() {
        assert!(parsed(r#"{"id":"subscribe","result":{"type":"subscription_started"}}"#).is_none());
    }

    #[test]
    fn a_frame_with_no_ids_still_parses() {
        let change = parsed(r#"{"event":"layout_updated","data":{"type":"layout_updated"}}"#);
        assert_eq!(change.map(|c| c.kind), Some(EventKind::LayoutUpdated));
    }

    #[test]
    fn an_event_frame_must_carry_its_required_data_object() {
        assert!(parsed(r#"{"event":"layout_updated"}"#).is_none());
    }

    #[test]
    fn output_match_ids_are_hoisted_from_the_nested_read() {
        let change = parsed(
            r#"{"event":"pane.output_matched","data":{"pane_id":"w2:p8","read":{"pane_id":"w2:p8","workspace_id":"w2","tab_id":"w2:t3"}}}"#,
        )
        .unwrap();
        assert_eq!(change.pane_id, Some(PaneId::new("w2:p8")));
        assert_eq!(change.workspace_id, Some(WorkspaceId::new("w2")));
        assert_eq!(change.tab_id, Some(TabId::new("w2:t3")));
    }

    // ------------------------------------------------- against a fake server
    //
    // Everything below is a path a healthy herdr will not produce on request.

    mod wire {
        use super::*;
        use crate::testing::{Server, Step, pane_event, subscribed};

        fn subscribe(script: Vec<Step>) -> Result<(Server, Events)> {
            let server = Server::start(script);
            let events = Events::subscribe(&server.client(), &[Subscription::PaneCreated])?;
            Ok((server, events))
        }

        #[test]
        fn an_acknowledged_subscription_yields_its_events() {
            let (_server, mut events) = subscribe(vec![
                subscribed(),
                pane_event("pane_created", "w1:p1"),
                pane_event("pane_closed", "w1:p1"),
            ])
            .unwrap();
            let batch = events.changes(Duration::from_secs(1)).unwrap().unwrap();
            assert_eq!(
                batch.iter().map(|c| c.kind).collect::<Vec<_>>(),
                [EventKind::PaneCreated, EventKind::PaneClosed],
                "one burst arrives as one batch"
            );
        }

        /// The reason this module does its own framing instead of `read_line`:
        /// a deadline can land between the halves of a frame, and `read_line`
        /// leaves its buffer unspecified when it errors, so the first half
        /// would be lost.
        #[test]
        fn a_frame_split_across_two_writes_is_reassembled() {
            let (_server, mut events) = subscribe(vec![
                subscribed(),
                Step::bytes(r#"{"event":"pane_created","data":{"pane_id":"#),
                Step::Wait(Duration::from_millis(400)),
                Step::bytes("\"w1:p1\"}}\n"),
            ])
            .unwrap();
            let batch = events.changes(Duration::from_secs(2)).unwrap().unwrap();
            assert_eq!(batch.len(), 1);
            assert_eq!(
                batch.first().and_then(|c| c.pane_id.clone()),
                Some(PaneId::new("w1:p1")),
                "the half held across the pause must survive"
            );
        }

        #[test]
        fn a_quiet_stream_reports_nothing_rather_than_failing() {
            let (_server, mut events) =
                subscribe(vec![subscribed(), Step::Wait(Duration::from_secs(5))]).unwrap();
            assert_eq!(events.changes(Duration::from_millis(200)).unwrap(), None);
        }

        /// A subscription cannot be recovered by reading harder, so a hang-up
        /// has to be an error and not another idle tick — a watcher that
        /// treated it as quiet would spin forever against a dead socket.
        #[test]
        fn a_hang_up_is_an_error_not_a_quiet_interval() {
            let (_server, mut events) = subscribe(vec![subscribed(), Step::Close]).unwrap();
            assert!(
                matches!(
                    events.changes(Duration::from_secs(1)),
                    Err(Error::NoResponse { .. })
                ),
                "a closed stream must not look idle"
            );
        }

        #[test]
        fn a_rejected_subscription_reports_the_servers_reason() {
            let server = Server::start(vec![Step::line(
                r#"{"id":"subscribe","error":{"code":"invalid_request","message":"missing field `pane_id`"}}"#,
            )]);
            let failed = Events::subscribe(&server.client(), &[Subscription::PaneCreated]);
            match failed {
                Err(Error::Rejected { code, message, .. }) => {
                    assert_eq!(code, "invalid_request");
                    assert_eq!(message, "missing field `pane_id`");
                }
                other => panic!("expected a rejection, got {other:?}"),
            }
        }

        #[test]
        fn an_unexpected_reply_to_subscribe_is_named_rather_than_ignored() {
            let server = Server::start(vec![Step::line(
                r#"{"id":"subscribe","result":{"type":"pong"}}"#,
            )]);
            assert!(matches!(
                Events::subscribe(&server.client(), &[Subscription::PaneCreated]),
                Err(Error::WrongResult { actual, .. }) if actual == "pong"
            ));
        }

        #[test]
        fn a_subscription_reply_for_a_different_request_is_refused() {
            let server = Server::start(vec![Step::line(
                r#"{"id":"someone-else","result":{"type":"subscription_started"}}"#,
            )]);
            assert!(matches!(
                Events::subscribe(&server.client(), &[Subscription::PaneCreated]),
                Err(Error::IdMismatch { actual, .. }) if actual == "someone-else"
            ));
        }

        #[test]
        fn a_subscription_reply_with_a_result_and_error_is_malformed() {
            let server = Server::start(vec![Step::line(
                r#"{"id":"subscribe","result":{"type":"subscription_started"},"error":{"code":"bad","message":"also an error"}}"#,
            )]);
            assert!(matches!(
                Events::subscribe(&server.client(), &[Subscription::PaneCreated]),
                Err(Error::Malformed { .. })
            ));
        }

        #[test]
        fn a_malformed_event_is_reported() {
            let (_server, mut events) = subscribe(vec![
                subscribed(),
                Step::line(r#"{"event":7,"data":{"pane_id":"w1:p1"}}"#),
            ])
            .unwrap();
            assert!(matches!(
                events.changes(Duration::from_secs(1)),
                Err(Error::Decode { .. })
            ));
        }

        /// A stream that never falls quiet must not hold the batch open
        /// forever. The replayed backlog is exactly that stream: frames about
        /// 100ms apart, which is inside the coalescing window.
        #[test]
        fn a_stream_that_never_falls_quiet_is_cut_off_at_the_cap() {
            let mut script = vec![subscribed()];
            for _ in 0..60 {
                script.push(pane_event("pane_created", "w1:p1"));
                script.push(Step::Wait(Duration::from_millis(100)));
            }
            let (_server, mut events) = subscribe(script).unwrap();

            let started = Instant::now();
            let batch = events.changes(Duration::from_secs(1)).unwrap().unwrap();
            let took = started.elapsed();

            assert!(
                took < MAX_BATCH + COALESCE,
                "held open for {took:?}, cap is {MAX_BATCH:?}"
            );
            assert!(
                batch.len() < 60,
                "the cap must cut the batch, got all {} frames",
                batch.len()
            );
        }
    }
}
