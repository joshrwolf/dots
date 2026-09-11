//! Metadata attached to Herdr workspace and pane rows.

use std::num::NonZeroU64;
use std::time::Duration;

use crate::api::{PaneReportMetadata, WorkspaceReportMetadata};
use crate::{Client, Error, PaneId, Result, Tokens, WorkspaceId};

const MAX_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Reports one plugin's metadata with an explicit retention policy.
#[derive(Debug, Clone, Copy)]
pub struct MetadataReporter<'a> {
    client: &'a Client,
    source: &'a str,
    ttl_ms: Option<NonZeroU64>,
}

impl<'a> MetadataReporter<'a> {
    /// Retain values until explicitly replaced, cleared, or their workspace closes.
    pub fn retained(client: &'a Client, source: &'a str) -> Self {
        Self {
            client,
            source,
            ttl_ms: None,
        }
    }

    pub fn new(client: &'a Client, source: &'a str, ttl: Duration) -> Result<Self> {
        let ttl_ms = u64::try_from(ttl.as_millis()).map_err(|_| Error::DurationOverflow {
            field: "metadata TTL",
            duration: ttl,
        })?;
        let ttl_ms = NonZeroU64::new(ttl_ms).ok_or(Error::MetadataTtlTooShort { duration: ttl })?;
        if ttl > MAX_TTL {
            return Err(Error::MetadataTtlTooLong { duration: ttl });
        }
        Ok(Self {
            client,
            source,
            ttl_ms: Some(ttl_ms),
        })
    }

    pub fn report_workspace(&self, workspace_id: &WorkspaceId, tokens: &Tokens) -> Result<()> {
        self.send_workspace(workspace_id, tokens, None)
    }

    /// Reports with a caller-owned monotonic sequence number.
    ///
    /// Sequential reconcilers should use [`Self::report_workspace`]. Pass a sequence
    /// only when refreshes can overlap and an older result could arrive last.
    pub fn report_workspace_sequenced(
        &self,
        workspace_id: &WorkspaceId,
        tokens: &Tokens,
        sequence: u64,
    ) -> Result<()> {
        self.send_workspace(workspace_id, tokens, Some(sequence))
    }

    fn send_workspace(
        &self,
        workspace_id: &WorkspaceId,
        tokens: &Tokens,
        sequence: Option<u64>,
    ) -> Result<()> {
        self.client.invoke(&WorkspaceReportMetadata {
            workspace_id,
            source: self.source,
            tokens,
            ttl_ms: self.ttl_ms.map(NonZeroU64::get),
            seq: sequence,
        })
    }

    pub fn report_pane(&self, pane_id: &PaneId, tokens: &Tokens) -> Result<()> {
        self.send_pane(pane_id, tokens, None)
    }

    /// Pane equivalent of [`Self::report_workspace_sequenced`].
    pub fn report_pane_sequenced(
        &self,
        pane_id: &PaneId,
        tokens: &Tokens,
        sequence: u64,
    ) -> Result<()> {
        self.send_pane(pane_id, tokens, Some(sequence))
    }

    fn send_pane(&self, pane_id: &PaneId, tokens: &Tokens, sequence: Option<u64>) -> Result<()> {
        self.client.invoke(&PaneReportMetadata {
            pane_id,
            source: self.source,
            tokens,
            ttl_ms: self.ttl_ms.map(NonZeroU64::get),
            seq: sequence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{RecordedRequest, Server, Step};
    use serde_json::json;

    #[test]
    fn retained_metadata_omits_expiry() {
        let server = Server::start(vec![Step::reply(r#"{"id":"{id}","result":{"type":"ok"}}"#)]);
        MetadataReporter::retained(&server.client(), "test")
            .report_workspace(
                &WorkspaceId::new("w1"),
                &Tokens::new().set("state", "ready").unwrap(),
            )
            .unwrap();
        let requests = server.requests();
        let request = requests.first().unwrap();
        assert!(request.params.get("ttl_ms").is_none());
        assert_eq!(request.params.pointer("/tokens/state").unwrap(), "ready");
    }

    #[test]
    fn a_ttl_shorter_than_one_protocol_tick_is_refused() {
        let client = Client::new("/not-used");
        for ttl in [Duration::ZERO, Duration::from_nanos(1)] {
            assert!(matches!(
                MetadataReporter::new(&client, "test", ttl),
                Err(Error::MetadataTtlTooShort { duration }) if duration == ttl
            ));
        }
    }

    #[test]
    fn the_protocols_ttl_maximum_is_inclusive() {
        let client = Client::new("/not-used");
        assert!(MetadataReporter::new(&client, "test", MAX_TTL).is_ok());
        let too_long = MAX_TTL + Duration::from_nanos(1);
        assert!(matches!(
            MetadataReporter::new(&client, "test", too_long),
            Err(Error::MetadataTtlTooLong { duration }) if duration == too_long
        ));
    }

    #[test]
    fn reporter_always_sends_ttl_and_only_sends_sequence_when_requested() {
        const OK: &str = r#"{"id":"{id}","result":{"type":"ok"}}"#;
        let server = Server::start(vec![Step::reply(OK), Step::reply(OK)]);
        let client = server.client();
        let reporter = MetadataReporter::new(&client, "test", Duration::from_secs(15)).unwrap();
        let tokens = Tokens::new().set("ci", "green").unwrap();

        reporter
            .report_workspace(&WorkspaceId::new("w1"), &tokens)
            .unwrap();
        reporter
            .report_pane_sequenced(&PaneId::new("w1:p1"), &tokens, 7)
            .unwrap();

        assert_eq!(
            server.requests(),
            [
                RecordedRequest {
                    method: "workspace.report_metadata".to_owned(),
                    params: json!({
                        "workspace_id": "w1",
                        "source": "test",
                        "tokens": {"ci": "green"},
                        "ttl_ms": 15_000,
                    }),
                },
                RecordedRequest {
                    method: "pane.report_metadata".to_owned(),
                    params: json!({
                        "pane_id": "w1:p1",
                        "source": "test",
                        "tokens": {"ci": "green"},
                        "ttl_ms": 15_000,
                        "seq": 7,
                    }),
                },
            ]
        );
    }
}
