//! Durable intent for one editor. A recorded owner is historical data, never
//! proof of liveness or permission to spawn, adopt, renew, or kill a process.
//! Hold this ledger's OS lock across reservation, bounded RPC, and reconciliation.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use herdrkit::WorkspaceId;
use herdrkit::state_file::StateFile;
use serde::{Deserialize, Serialize};

use crate::protocol::{Context, LeaseId, OwnerControl, OwnerPhase, Reply, VERSION};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub server_id: String,
    pub plugin_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub scope: Scope,
    pub context: Context,
    pub socket: PathBuf,
    pub workspace: WorkspaceId,
    pub directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryIntent {
    pub expected: LeaseId,
    pub owner: String,
    pub not_after_ms: u64,
}
impl RecoveryIntent {
    pub fn control(&self) -> OwnerControl {
        OwnerControl::Recover {
            expected: self.expected.clone(),
            owner: self.owner.clone(),
            not_after_ms: self.not_after_ms,
        }
    }
    fn successor(&self) -> Result<LeaseId> {
        Ok(LeaseId {
            owner: self.owner.clone(),
            generation: self
                .expected
                .generation
                .checked_add(1)
                .ok_or(Error::Invalid("generation exhausted"))?,
        })
    }
    fn validate(&self) -> Result<()> {
        validate_lease(&self.expected)?;
        if !token(&self.owner)
            || self.owner == self.expected.owner
            || self.not_after_ms == 0
            || self.not_after_ms > 9_007_199_254_740_991
        {
            return Err(Error::Invalid("invalid recovery intent"));
        }
        self.successor()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    LaunchPending { lease: LeaseId },
    Observed { lease: LeaseId },
    RecoveryPending { intent: RecoveryIntent },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    schema: u32,
    pub binding: Binding,
    pub phase: Phase,
}
impl Record {
    fn validate(&self) -> Result<()> {
        let binding = &self.binding;
        if self.schema != 1
            || !token(&binding.context.registration)
            || binding.scope.server_id.is_empty()
            || binding.scope.server_id.chars().any(char::is_control)
            || binding.workspace.as_str().is_empty()
            || binding.workspace.as_str().chars().any(char::is_control)
            || !absolute(&binding.scope.plugin_root)
            || !absolute(&binding.directory)
            || !absolute(&binding.context.server)
            || !absolute(&binding.socket)
            || binding
                .context
                .server
                .to_string_lossy()
                .chars()
                .any(char::is_control)
            || binding.socket.file_name().and_then(|n| n.to_str())
                != Some(&format!("{}.sock", binding.context.registration))
        {
            return Err(Error::Invalid("invalid record identity, schema, or paths"));
        }
        match &self.phase {
            Phase::LaunchPending { lease } => {
                validate_lease(lease)?;
                if lease.generation != 1 {
                    return Err(Error::Invalid("launch generation must be one"));
                }
            }
            Phase::Observed { lease } => validate_lease(lease)?,
            Phase::RecoveryPending { intent } => intent.validate()?,
        }
        Ok(())
    }
    fn require_scope(&self, current: &Scope) -> Result<()> {
        if self.binding.scope != *current {
            return Err(Error::ScopeChanged);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("recovery record storage failed")]
    Storage(#[from] herdrkit::state_file::Error),
    #[error("reading operating-system randomness for editor identity")]
    Random(#[source] std::io::Error),
    #[error("invalid recovery record: {0}")]
    Invalid(&'static str),
    #[error("an editor is already recorded; inspect and reconcile it, do not respawn")]
    AlreadyRecorded,
    #[error("no editor is recorded")]
    Missing,
    #[error("Herdr server or plugin scope changed; automatic adoption is forbidden")]
    ScopeChanged,
    #[error("pending operation or recorded owner changed")]
    Conflict,
    #[error("observation does not identify the recorded managed editor")]
    ForeignObservation,
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciled {
    /// No reply is not evidence that the editor does not exist.
    Unreachable,
    /// The old lease is still observed. A queued recovery may still execute.
    RecoveryPending,
    /// Historical ownership has been confirmed, not reserved or renewed.
    Observed(OwnerPhase),
}

#[derive(Debug)]
pub struct Ledger {
    file: StateFile,
}
impl Ledger {
    pub fn lock(directory: &Path) -> Result<Self> {
        Ok(Self {
            file: StateFile::lock(directory)?,
        })
    }

    pub fn read(&self) -> Result<Option<Record>> {
        let record: Option<Record> = self.file.read()?;
        if let Some(record) = &record {
            record.validate()?;
        }
        Ok(record)
    }

    /// Persist before spawning anything. Existing records, including unfinished
    /// launches, can never be overwritten by another launch reservation.
    pub fn reserve_launch(&mut self, binding: Binding, owner: String) -> Result<Record> {
        let record = Record {
            schema: 1,
            binding,
            phase: Phase::LaunchPending {
                lease: LeaseId {
                    owner,
                    generation: 1,
                },
            },
        };
        record.validate()?;
        if self.read()?.is_some() {
            return Err(Error::AlreadyRecorded);
        }
        self.save(&record)?;
        Ok(record)
    }

    /// `current` must come from fresh controller-side owner/enablement checks,
    /// not simply be copied out of this record. This method does no Herdr I/O.
    pub fn reserve_recovery(&mut self, current: &Scope, intent: RecoveryIntent) -> Result<Record> {
        intent.validate()?;
        let mut record = self.required()?;
        record.require_scope(current)?;
        match &record.phase {
            Phase::Observed { lease } if *lease == intent.expected => {}
            Phase::RecoveryPending { intent: saved } if *saved == intent => return Ok(record),
            _ => return Err(Error::Conflict),
        }
        record.phase = Phase::RecoveryPending { intent };
        self.save(&record)?;
        Ok(record)
    }

    /// Explicitly confirmed retry only. Keep the exact expected/new-owner pair;
    /// changing to a different recovery would lose track of an uncertain effect.
    pub fn retry_recovery(
        &mut self,
        current: &Scope,
        expected: &RecoveryIntent,
        not_after_ms: u64,
    ) -> Result<Record> {
        let mut record = self.required()?;
        record.require_scope(current)?;
        let Phase::RecoveryPending { intent } = &mut record.phase else {
            return Err(Error::Conflict);
        };
        if intent != expected {
            return Err(Error::Conflict);
        }
        intent.not_after_ms = not_after_ms;
        self.save(&record)?;
        Ok(record)
    }

    /// No reply, the old lease, foreign identity, and a successor observation
    /// are distinct. Only an exact matching observation can advance the record.
    /// Reconciliation never sends a recovery, renews a lease, or removes a record.
    pub fn reconcile(&mut self, current: &Scope, reply: Option<&Reply>) -> Result<Reconciled> {
        let mut record = self.required()?;
        record.require_scope(current)?;
        let Some(reply) = reply else {
            return Ok(Reconciled::Unreachable);
        };
        let Reply::Ownership {
            identity,
            ownership: Some(ownership),
        } = reply
        else {
            return Err(Error::ForeignObservation);
        };
        if identity.protocol != VERSION
            || identity.pid == 0
            || identity.context != record.binding.context
        {
            return Err(Error::ForeignObservation);
        }
        validate_lease(&ownership.lease).map_err(|_| Error::ForeignObservation)?;
        if (ownership.phase == OwnerPhase::Orphaned) != (ownership.remaining_ms == 0) {
            return Err(Error::ForeignObservation);
        }
        let expected = match &record.phase {
            Phase::LaunchPending { lease } | Phase::Observed { lease } => lease.clone(),
            Phase::RecoveryPending { intent } => {
                if ownership.lease == intent.expected {
                    return Ok(Reconciled::RecoveryPending);
                }
                intent.successor()?
            }
        };
        if ownership.lease != expected {
            return Err(Error::ForeignObservation);
        }
        let phase = Phase::Observed { lease: expected };
        if record.phase != phase {
            record.phase = phase;
            self.save(&record)?;
        }
        Ok(Reconciled::Observed(ownership.phase))
    }

    fn required(&self) -> Result<Record> {
        self.read()?.ok_or(Error::Missing)
    }
    fn save(&mut self, record: &Record) -> Result<()> {
        record.validate()?;
        self.file.write(record)?;
        Ok(())
    }
}

/// Generate a fresh registration or controller-run ID before recording intent.
/// This plugin targets Unix; use OS randomness, never PIDs or timestamps.
pub fn fresh_id() -> Result<String> {
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(Error::Random)?;
    let mut id = String::with_capacity(32);
    for byte in bytes {
        // Formatting an integer into a String cannot fail.
        let _ = write!(id, "{byte:02x}");
    }
    Ok(id)
}

fn token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn validate_lease(lease: &LeaseId) -> Result<()> {
    if !token(&lease.owner) || lease.generation == 0 {
        return Err(Error::Invalid("invalid owner lease"));
    }
    Ok(())
}
fn absolute(path: &Path) -> bool {
    path.is_absolute() && path.to_str().is_some_and(|s| !s.contains('\0'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Identity, Ownership};
    use std::fs;
    use std::os::unix::fs::DirBuilderExt as _;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("editor-ledger-{}", fresh_id().unwrap()));
            fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
            Self(path)
        }
        fn ledger(&self) -> Ledger {
            Ledger::lock(&self.0.join("slot")).unwrap()
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn binding() -> Binding {
        Binding {
            scope: Scope {
                server_id: "example-server@1:1".into(),
                plugin_root: "/tmp/example-plugin".into(),
            },
            context: Context {
                registration: "a".repeat(32),
                server: "/tmp/example-herdr.sock".into(),
            },
            socket: PathBuf::from(format!("/tmp/example/{}.sock", "a".repeat(32))),
            workspace: WorkspaceId::new("w1"),
            directory: "/tmp/example-checkout".into(),
        }
    }
    fn lease() -> LeaseId {
        LeaseId {
            owner: "b".repeat(32),
            generation: 1,
        }
    }
    fn observation(lease: LeaseId, phase: OwnerPhase) -> Reply {
        Reply::Ownership {
            identity: Identity {
                protocol: VERSION,
                context: binding().context,
                pid: 123,
            },
            ownership: Some(Ownership {
                lease,
                phase,
                remaining_ms: if phase == OwnerPhase::Active { 3000 } else { 0 },
            }),
        }
    }
    fn intent() -> RecoveryIntent {
        RecoveryIntent {
            expected: lease(),
            owner: "c".repeat(32),
            not_after_ms: 2000,
        }
    }

    #[test]
    fn interrupted_launch_is_not_permission_to_spawn_again() {
        let root = Scratch::new();
        let mut slot = root.ledger();
        let record = slot.reserve_launch(binding(), lease().owner).unwrap();
        drop(slot);
        let mut slot = root.ledger();
        assert_eq!(slot.read().unwrap(), Some(record.clone()));
        assert_eq!(
            slot.reconcile(&binding().scope, None).unwrap(),
            Reconciled::Unreachable
        );
        assert!(matches!(
            slot.reserve_launch(binding(), lease().owner),
            Err(Error::AlreadyRecorded)
        ));
        assert_eq!(slot.read().unwrap(), Some(record));
    }

    #[test]
    fn lost_recovery_reply_reconciles_only_the_exact_successor() {
        let root = Scratch::new();
        let mut slot = root.ledger();
        let scope = binding().scope;
        slot.reserve_launch(binding(), lease().owner).unwrap();
        let old = observation(lease(), OwnerPhase::Orphaned);
        slot.reconcile(&scope, Some(&old)).unwrap();
        let pending = slot.reserve_recovery(&scope, intent()).unwrap();
        assert_eq!(slot.reserve_recovery(&scope, intent()).unwrap(), pending);
        drop(slot);
        let mut slot = root.ledger();
        assert_eq!(
            slot.reconcile(&scope, None).unwrap(),
            Reconciled::Unreachable
        );
        assert_eq!(
            slot.reconcile(&scope, Some(&old)).unwrap(),
            Reconciled::RecoveryPending
        );
        assert_eq!(slot.read().unwrap(), Some(pending));
        // Even an already-expired successor confirms which generation applied;
        // it does not authorize renewal or claim to be alive now.
        let next = observation(intent().successor().unwrap(), OwnerPhase::Orphaned);
        assert_eq!(
            slot.reconcile(&scope, Some(&next)).unwrap(),
            Reconciled::Observed(OwnerPhase::Orphaned)
        );
        assert!(
            matches!(slot.read().unwrap().unwrap().phase, Phase::Observed { lease } if lease.generation == 2)
        );
        assert!(matches!(
            slot.reserve_recovery(&scope, intent()),
            Err(Error::Conflict)
        ));
    }

    #[test]
    fn explicit_retry_keeps_the_same_recovery_and_checks_the_saved_intent() {
        let root = Scratch::new();
        let mut slot = root.ledger();
        let scope = binding().scope;
        slot.reserve_launch(binding(), lease().owner).unwrap();
        slot.reconcile(&scope, Some(&observation(lease(), OwnerPhase::Orphaned)))
            .unwrap();
        slot.reserve_recovery(&scope, intent()).unwrap();
        let mut competing = intent();
        competing.owner = "d".repeat(32);
        assert!(matches!(
            slot.reserve_recovery(&scope, competing),
            Err(Error::Conflict)
        ));
        let updated = slot.retry_recovery(&scope, &intent(), 3000).unwrap();
        assert!(matches!(
            slot.retry_recovery(&scope, &intent(), 4000),
            Err(Error::Conflict)
        ));
        let Phase::RecoveryPending { intent: current } = &updated.phase else {
            panic!("expected pending recovery")
        };
        assert_eq!(current.expected, intent().expected);
        assert_eq!(current.owner, intent().owner);
        assert_eq!(current.not_after_ms, 3000);
        assert_eq!(slot.read().unwrap(), Some(updated));
    }

    #[test]
    fn foreign_identity_owner_or_scope_never_changes_a_record() {
        let root = Scratch::new();
        let mut slot = root.ledger();
        let original = slot.reserve_launch(binding(), lease().owner).unwrap();
        let scope = binding().scope;
        let valid = observation(lease(), OwnerPhase::Active);
        let mut changed_scope = scope.clone();
        changed_scope.server_id.push_str("-replacement");
        assert!(matches!(
            slot.reconcile(&changed_scope, Some(&valid)),
            Err(Error::ScopeChanged)
        ));
        assert!(matches!(
            slot.reserve_recovery(&changed_scope, intent()),
            Err(Error::ScopeChanged)
        ));
        assert!(matches!(
            slot.retry_recovery(&changed_scope, &intent(), 3000),
            Err(Error::ScopeChanged)
        ));
        changed_scope = scope.clone();
        changed_scope.plugin_root = "/tmp/replacement-plugin".into();
        assert!(matches!(
            slot.reconcile(&changed_scope, None),
            Err(Error::ScopeChanged)
        ));
        for mutation in [
            "registration",
            "server",
            "protocol",
            "pid",
            "owner",
            "generation",
            "unmanaged",
            "malformed_lease",
            "inconsistent_phase",
        ] {
            let mut reply = valid.clone();
            let Reply::Ownership {
                identity,
                ownership,
            } = &mut reply
            else {
                panic!("fixture shape")
            };
            match mutation {
                "registration" => identity.context.registration = "d".repeat(32),
                "server" => identity.context.server = "/tmp/other-server.sock".into(),
                "protocol" => identity.protocol = 99,
                "pid" => identity.pid = 0,
                "owner" => ownership.as_mut().unwrap().lease.owner = "d".repeat(32),
                "generation" => ownership.as_mut().unwrap().lease.generation = 2,
                "unmanaged" => *ownership = None,
                "malformed_lease" => ownership.as_mut().unwrap().lease.generation = 0,
                "inconsistent_phase" => ownership.as_mut().unwrap().remaining_ms = 0,
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    slot.reconcile(&scope, Some(&reply)),
                    Err(Error::ForeignObservation)
                ),
                "{mutation}"
            );
            assert_eq!(slot.read().unwrap(), Some(original.clone()));
        }
    }

    #[test]
    fn corrupt_or_unknown_state_is_not_an_empty_slot() {
        let root = Scratch::new();
        let mut slot = root.ledger();
        let original = slot.reserve_launch(binding(), lease().owner).unwrap();
        for (pointer, value) in [
            ("/schema", serde_json::json!(99)),
            ("/phase/lease/generation", serde_json::json!(0)),
            ("/binding/socket", serde_json::json!("/tmp/wrong-name.sock")),
            (
                "/binding/context/registration",
                serde_json::json!("../escape"),
            ),
        ] {
            let mut raw = serde_json::to_value(&original).unwrap();
            *raw.pointer_mut(pointer).unwrap() = value;
            fs::write(root.0.join("slot/record.json"), raw.to_string()).unwrap();
            assert!(matches!(slot.read(), Err(Error::Invalid(_))), "{pointer}");
            assert!(slot.reserve_launch(binding(), lease().owner).is_err());
            assert_eq!(
                fs::read_to_string(root.0.join("slot/record.json")).unwrap(),
                raw.to_string()
            );
        }
        fs::write(root.0.join("slot/record.json"), "{broken").unwrap();
        assert!(matches!(slot.read(), Err(Error::Storage(_))));
    }

    #[test]
    fn invalid_reservations_have_no_record_side_effects() {
        let root = Scratch::new();
        let mut slot = root.ledger();
        let mut invalid = binding();
        invalid.socket = "/tmp/other.sock".into();
        assert!(matches!(
            slot.reserve_launch(invalid, lease().owner),
            Err(Error::Invalid(_))
        ));
        assert_eq!(slot.read().unwrap(), None);
        slot.reserve_launch(binding(), lease().owner).unwrap();
        slot.reconcile(
            &binding().scope,
            Some(&observation(lease(), OwnerPhase::Orphaned)),
        )
        .unwrap();
        let before = slot.read().unwrap();
        let mut invalid = intent();
        invalid.owner = invalid.expected.owner.clone();
        assert!(matches!(
            slot.reserve_recovery(&binding().scope, invalid),
            Err(Error::Invalid(_))
        ));
        let mut overflow = intent();
        overflow.expected.generation = u32::MAX;
        assert!(matches!(
            slot.reserve_recovery(&binding().scope, overflow),
            Err(Error::Invalid(_))
        ));
        assert_eq!(slot.read().unwrap(), before);
    }

    #[test]
    fn random_ids_fit_the_existing_endpoint_and_owner_contract() {
        let first = fresh_id().unwrap();
        let second = fresh_id().unwrap();
        assert!(token(&first) && token(&second));
        assert_ne!(first, second);
    }
}
