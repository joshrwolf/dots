//! One-shot control messages over the enrolled Tailscale SSH connection.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{Delivery, DeliveryReceipt, Identity, Result, Snapshot, Store};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerStatus {
    pub identity: Identity,
    pub snapshot: Snapshot,
    pub root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub source: Identity,
    pub snapshot: Snapshot,
    pub reference: String,
    pub repository: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PeerRequest {
    Status {
        root: PathBuf,
    },
    Export {
        root: PathBuf,
        recipient: Identity,
        snapshot: Snapshot,
    },
    Accept {
        root: PathBuf,
        delivery: Delivery,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PeerResponse {
    Conflict {
        expected: Snapshot,
        actual: Snapshot,
    },
    Status {
        status: PeerStatus,
    },
    Export {
        export: Export,
    },
    Accepted {
        receipt: DeliveryReceipt,
    },
    NeedsTransfer {
        recipient: Identity,
        expected: Snapshot,
        reference: String,
    },
}

/// Handle one request without a listener, scheduler or implicit store initialization.
pub fn serve_peer(request: PeerRequest) -> Result<PeerResponse> {
    match request {
        PeerRequest::Status { root } => {
            let store = Store::open(&root)?;
            Ok(PeerResponse::Status {
                status: PeerStatus {
                    identity: store.identity(),
                    snapshot: store.snapshot()?,
                    root: store.root().to_owned(),
                },
            })
        }
        PeerRequest::Export {
            root,
            recipient,
            snapshot,
        } => {
            let store = Store::open(&root)?;
            if store.identity() != recipient {
                return Err(crate::invalid("export request targets another node"));
            }
            Ok(PeerResponse::Export {
                export: store.prepare_export(&snapshot)?,
            })
        }
        PeerRequest::Accept { root, delivery } => {
            let store = Store::open(&root)?;
            delivery.validate_recipient(&store)?;
            let reference = delivery.candidate.transfer_reference(&delivery.recipient);
            if store
                .git()
                .reference(&delivery.candidate.reference())?
                .is_none()
                && store.git().reference(&reference)?.is_none()
            {
                store.check_expected(&delivery.expected)?;
                Ok(PeerResponse::NeedsTransfer {
                    recipient: store.identity(),
                    expected: delivery.expected,
                    reference,
                })
            } else {
                Ok(PeerResponse::Accepted {
                    receipt: store.accept_transfer(&delivery)?,
                })
            }
        }
    }
}
