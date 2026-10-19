use std::borrow::Borrow;
use std::collections::HashSet;
use std::fmt;

use herdrkit::{PaneId, WorkspaceId};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{Error, Result};

fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

macro_rules! id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Opaque identity of a ", $what, ".")]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if !valid_id(&value) {
                    return Err(Error::InvalidIdentifier { kind: $what, value });
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub(crate) fn from_stored(value: String) -> Self {
                Self(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

id!(ReviewContextId, "review context");
id!(ObservationId, "review observation");
id!(RuntimeBindingId, "review runtime binding");
id!(ThreadId, "review thread");
id!(MessageId, "review message");
id!(AgentRequestId, "agent request");
id!(DispatchAttemptId, "dispatch attempt");

/// One operational Git endpoint supplied when capturing an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiffEndpoint {
    Commit { oid: String },
    Index,
    WorkingTree,
}

impl DiffEndpoint {
    pub(crate) fn validate(&self) -> Result<()> {
        if let Self::Commit { oid } = self {
            validate_oid(oid)?;
        }
        Ok(())
    }

    pub(crate) fn columns(&self) -> (&'static str, Option<&str>) {
        match self {
            Self::Commit { oid } => ("commit", Some(oid)),
            Self::Index => ("index", None),
            Self::WorkingTree => ("working_tree", None),
        }
    }

    pub(crate) fn from_columns(kind: &str, oid: Option<String>) -> Option<Self> {
        match (kind, oid) {
            ("commit", Some(oid)) => Some(Self::Commit { oid }),
            ("index", None) => Some(Self::Index),
            ("working_tree", None) => Some(Self::WorkingTree),
            _ => None,
        }
    }
}

/// An ordered pair of operational endpoints to capture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ComparisonSpec {
    pub base: DiffEndpoint,
    pub target: DiffEndpoint,
}

impl ComparisonSpec {
    pub fn new(base: DiffEndpoint, target: DiffEndpoint) -> Result<Self> {
        let comparison = Self { base, target };
        comparison.validate()?;
        Ok(comparison)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        self.base.validate()?;
        self.target.validate()?;
        let supported = matches!(
            (&self.base, &self.target),
            (
                DiffEndpoint::Commit { .. },
                DiffEndpoint::Commit { .. } | DiffEndpoint::Index | DiffEndpoint::WorkingTree
            ) | (DiffEndpoint::Index, DiffEndpoint::WorkingTree)
        );
        if !supported || self.base == self.target {
            return Err(Error::InvalidComparison);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ComparisonSpec {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            base: DiffEndpoint,
            target: DiffEndpoint,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.base, raw.target).map_err(serde::de::Error::custom)
    }
}

/// An immutable commit used to reproduce one endpoint of an observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedEndpoint {
    pub source: DiffEndpoint,
    pub oid: String,
}

impl CapturedEndpoint {
    pub(crate) fn validate(&self) -> Result<()> {
        self.source.validate()?;
        validate_oid(&self.oid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedComparison {
    pub base: CapturedEndpoint,
    pub target: CapturedEndpoint,
}

impl CapturedComparison {
    pub(crate) fn validate(&self) -> Result<()> {
        self.base.validate()?;
        self.target.validate()?;
        if self.base.oid == self.target.oid {
            return Err(Error::EmptyComparison);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ExternalReference {
    pub kind: String,
    pub locator: String,
}

impl ExternalReference {
    pub fn new(kind: impl Into<String>, locator: impl Into<String>) -> Result<Self> {
        let reference = Self {
            kind: kind.into(),
            locator: locator.into(),
        };
        reference.validate()?;
        Ok(reference)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_required("external reference kind", &self.kind)?;
        validate_required("external reference locator", &self.locator)
    }
}

impl<'de> Deserialize<'de> for ExternalReference {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            kind: String,
            locator: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.kind, raw.locator).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewContext {
    pub id: ReviewContextId,
    pub title: String,
    pub references: Vec<ExternalReference>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextListQuery {
    pub context_limit: u32,
    pub bindings_per_context: u32,
}

impl ContextListQuery {
    pub fn new(context_limit: u32, bindings_per_context: u32) -> Result<Self> {
        const MAX_CONTEXTS: u32 = 500;
        const MAX_BINDINGS_PER_CONTEXT: u32 = 50;
        if !(1..=MAX_CONTEXTS).contains(&context_limit) {
            return Err(Error::InvalidListLimit {
                field: "context limit",
                value: context_limit,
                maximum: MAX_CONTEXTS,
            });
        }
        if !(1..=MAX_BINDINGS_PER_CONTEXT).contains(&bindings_per_context) {
            return Err(Error::InvalidListLimit {
                field: "bindings per context",
                value: bindings_per_context,
                maximum: MAX_BINDINGS_PER_CONTEXT,
            });
        }
        Ok(Self {
            context_limit,
            bindings_per_context,
        })
    }
}

impl Default for ContextListQuery {
    fn default() -> Self {
        Self {
            context_limit: 100,
            bindings_per_context: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextListing {
    pub context: ReviewContext,
    pub runtime_bindings: Vec<RuntimeBinding>,
    pub open_threads: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub id: ObservationId,
    pub context_id: ReviewContextId,
    pub ordinal: u32,
    pub comparison: CapturedComparison,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutKind {
    Existing,
    ManagedWorktree,
}

impl CheckoutKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::ManagedWorktree => "managed_worktree",
        }
    }
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "existing" => Some(Self::Existing),
            "managed_worktree" => Some(Self::ManagedWorktree),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBinding {
    pub id: RuntimeBindingId,
    pub context_id: ReviewContextId,
    pub observation_id: ObservationId,
    pub checkout_token: String,
    pub checkout_root: String,
    pub checkout_kind: CheckoutKind,
    pub herdr_server_id: String,
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<PaneId>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSide {
    Base,
    Target,
}

impl DiffSide {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Target => "target",
        }
    }
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "base" => Some(Self::Base),
            "target" => Some(Self::Target),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorLocation {
    pub path: String,
    pub side: DiffSide,
    pub start_line: u32,
    pub end_line: u32,
}

impl AnchorLocation {
    pub fn new(
        path: impl Into<String>,
        side: DiffSide,
        start_line: u32,
        end_line: u32,
    ) -> Result<Self> {
        let location = Self {
            path: path.into(),
            side,
            start_line,
            end_line,
        };
        location.validate()?;
        Ok(location)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        validate_required("anchor path", &self.path)?;
        if self.start_line == 0 || self.end_line < self.start_line {
            return Err(Error::InvalidLineRange);
        }
        Ok(())
    }
    pub(crate) fn line_count(&self) -> u32 {
        self.end_line - self.start_line + 1
    }
    pub(crate) fn relocated(&self, path: String, start_line: u32, end_line: u32) -> Self {
        Self {
            path,
            side: self.side,
            start_line,
            end_line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    pub selected: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAnchor {
    pub original: AnchorLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<AnchorContext>,
}

impl ReviewAnchor {
    pub fn new(original: AnchorLocation, context: Option<AnchorContext>) -> Result<Self> {
        let anchor = Self { original, context };
        anchor.validate()?;
        Ok(anchor)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        self.original.validate()?;
        if let Some(context) = &self.context {
            if context.selected.is_empty() {
                return Err(Error::EmptyField {
                    field: "anchor selected lines",
                });
            }
            let expected = usize::try_from(self.original.line_count()).map_err(|_| {
                Error::AnchorSpanMismatch {
                    expected: u64::from(self.original.line_count()),
                    actual: context.selected.len(),
                }
            })?;
            if context.selected.len() != expected {
                return Err(Error::AnchorSpanMismatch {
                    expected: u64::from(self.original.line_count()),
                    actual: context.selected.len(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorStatus {
    Exact,
    Shifted,
    Modified,
    Deleted,
    Ambiguous,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorResolution {
    pub status: AnchorStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_location: Option<AnchorLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    Open,
    Resolved,
}

impl ThreadStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
        }
    }

    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "resolved" => Some(Self::Resolved),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewThread {
    pub anchor: ReviewAnchor,
    pub body: String,
    pub author: String,
}

impl NewThread {
    pub fn new(
        anchor: ReviewAnchor,
        body: impl Into<String>,
        author: impl Into<String>,
    ) -> Result<Self> {
        let value = Self {
            anchor,
            body: body.into(),
            author: author.into(),
        };
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        validate_input_size("new thread", self)?;
        self.anchor.validate()?;
        validate_required("message body", &self.body)?;
        validate_required("message author", &self.author)
    }
}

impl<'de> Deserialize<'de> for NewThread {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            anchor: ReviewAnchor,
            body: String,
            author: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.anchor, raw.body, raw.author).map_err(serde::de::Error::custom)
    }
}

/// Input for atomically creating the first bound view and discussion of a context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateBoundThread {
    pub title: String,
    pub references: Vec<ExternalReference>,
    pub comparison: ComparisonSpec,
    pub herdr_server_id: String,
    pub workspace_id: WorkspaceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<PaneId>,
    pub checkout_kind: CheckoutKind,
    pub thread: NewThread,
}

impl CreateBoundThread {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: impl Into<String>,
        references: Vec<ExternalReference>,
        comparison: ComparisonSpec,
        herdr_server_id: impl Into<String>,
        workspace_id: WorkspaceId,
        pane_id: Option<PaneId>,
        checkout_kind: CheckoutKind,
        thread: NewThread,
    ) -> Result<Self> {
        let value = Self {
            title: title.into(),
            references,
            comparison,
            herdr_server_id: herdr_server_id.into(),
            workspace_id,
            pane_id,
            checkout_kind,
            thread,
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_required("review title", &self.title)?;
        self.comparison.validate()?;
        validate_required("Herdr server id", &self.herdr_server_id)?;
        validate_required("review workspace id", self.workspace_id.as_str())?;
        if let Some(pane) = &self.pane_id {
            validate_required("review editor pane id", pane.as_str())?;
        }
        self.thread.validate()
    }
}

impl<'de> Deserialize<'de> for CreateBoundThread {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            title: String,
            references: Vec<ExternalReference>,
            comparison: ComparisonSpec,
            herdr_server_id: String,
            workspace_id: WorkspaceId,
            pane_id: Option<PaneId>,
            checkout_kind: CheckoutKind,
            thread: NewThread,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(
            raw.title,
            raw.references,
            raw.comparison,
            raw.herdr_server_id,
            raw.workspace_id,
            raw.pane_id,
            raw.checkout_kind,
            raw.thread,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewMessage {
    pub body: String,
    pub author: String,
}

impl NewMessage {
    pub fn new(body: impl Into<String>, author: impl Into<String>) -> Result<Self> {
        let value = Self {
            body: body.into(),
            author: author.into(),
        };
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        validate_input_size("new message", self)?;
        validate_required("message body", &self.body)?;
        validate_required("message author", &self.author)
    }
}

impl<'de> Deserialize<'de> for NewMessage {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            body: String,
            author: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.body, raw.author).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    Reviewer,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub author: String,
    pub body: String,
    pub origin: MessageOrigin,
    pub reply_to_request: Option<AgentRequestId>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub id: ThreadId,
    pub context_id: ReviewContextId,
    pub observation_id: ObservationId,
    pub anchor: Option<ReviewAnchor>,
    pub finding: Option<Finding>,
    pub status: ThreadStatus,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<AnchorResolution>,
    #[serde(skip)]
    pub(crate) anchor_source_id: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Finding,
    Question,
    Design,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Blocking,
    NonBlocking,
    Nit,
}

/// Private agent-authored assessment, never an outgoing review comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub key: String,
    pub kind: FindingKind,
    pub severity: Option<FindingSeverity>,
    pub title: String,
    pub evidence: String,
    #[serde(default)]
    pub related_locations: Vec<AnchorLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFinding {
    pub finding: Finding,
    /// None means an explicitly general review-level finding.
    pub location: Option<AnchorLocation>,
    pub body: String,
    pub author: String,
}

impl NewFinding {
    pub(crate) fn validate(&self) -> Result<()> {
        validate_input_size("new finding", self)?;
        validate_required("finding key", &self.finding.key)?;
        validate_required("finding title", &self.finding.title)?;
        validate_required("finding evidence", &self.finding.evidence)?;
        validate_required("message body", &self.body)?;
        validate_required("message author", &self.author)?;
        for location in self.location.iter().chain(&self.finding.related_locations) {
            location.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentAssignment {
    pub herdr_server_id: String,
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_agent_kind: Option<String>,
}

impl AgentAssignment {
    /// Match an observed Herdr agent without imposing tab/focus policy.
    pub fn matches_agent(&self, agent: &herdrkit::api::Agent) -> bool {
        self.matches_identity(
            &agent.workspace_id,
            &agent.pane_id,
            agent.name.as_ref(),
            agent.agent_kind.as_ref(),
        )
    }

    pub fn matches_assignment(&self, observed: &Self) -> bool {
        self.herdr_server_id == observed.herdr_server_id
            && self.matches_identity(
                &observed.workspace_id,
                &observed.pane_id,
                observed.expected_agent_name.as_ref(),
                observed.expected_agent_kind.as_ref(),
            )
    }

    fn matches_identity(
        &self,
        workspace: &WorkspaceId,
        pane: &PaneId,
        name: Option<&String>,
        kind: Option<&String>,
    ) -> bool {
        self.workspace_id == *workspace
            && self.pane_id == *pane
            && self
                .expected_agent_name
                .as_ref()
                .is_none_or(|expected| name == Some(expected))
            && self
                .expected_agent_kind
                .as_ref()
                .is_none_or(|expected| kind == Some(expected))
    }
    pub fn new(
        herdr_server_id: impl Into<String>,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        expected_agent_name: Option<String>,
        expected_agent_kind: Option<String>,
    ) -> Result<Self> {
        let value = Self {
            herdr_server_id: herdr_server_id.into(),
            workspace_id,
            pane_id,
            expected_agent_name,
            expected_agent_kind,
        };
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        validate_input_size("agent assignment", self)?;
        validate_required("Herdr server id", &self.herdr_server_id)?;
        validate_required("workspace id", self.workspace_id.as_str())?;
        validate_required("pane id", self.pane_id.as_str())?;
        validate_optional("expected agent name", self.expected_agent_name.as_deref())?;
        validate_optional("expected agent kind", self.expected_agent_kind.as_deref())?;
        if self.expected_agent_name.is_none() && self.expected_agent_kind.is_none() {
            return Err(Error::MissingExpectedAgentIdentity);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for AgentAssignment {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            herdr_server_id: String,
            workspace_id: WorkspaceId,
            pane_id: PaneId,
            #[serde(default)]
            expected_agent_name: Option<String>,
            #[serde(default)]
            expected_agent_kind: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(
            raw.herdr_server_id,
            raw.workspace_id,
            raw.pane_id,
            raw.expected_agent_name,
            raw.expected_agent_kind,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateAgentRequest {
    pub thread_ids: Vec<ThreadId>,
    pub assignment: AgentAssignment,
}

impl CreateAgentRequest {
    pub fn new(thread_ids: Vec<ThreadId>, assignment: AgentAssignment) -> Result<Self> {
        let value = Self {
            thread_ids,
            assignment,
        };
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        validate_input_size("agent request candidates", self)?;
        self.assignment.validate()?;
        if self.thread_ids.is_empty() {
            return Err(Error::EmptyThreadBatch);
        }
        if self.thread_ids.iter().collect::<HashSet<_>>().len() != self.thread_ids.len() {
            return Err(Error::DuplicateThreadInBatch);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for CreateAgentRequest {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            thread_ids: Vec<ThreadId>,
            assignment: AgentAssignment,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.thread_ids, raw.assignment).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRequestState {
    AwaitingDispatch,
    Dispatching,
    AwaitingRetry,
    Returned,
}

impl AgentRequestState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingDispatch => "awaiting_dispatch",
            Self::Dispatching => "dispatching",
            Self::AwaitingRetry => "awaiting_retry",
            Self::Returned => "returned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchAttemptState {
    Pending,
    Dispatching,
    Returned,
    Blocked,
    Rejected,
    Unknown,
    Superseded,
}

impl DispatchAttemptState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Dispatching => "dispatching",
            Self::Returned => "returned",
            Self::Blocked => "blocked",
            Self::Rejected => "rejected",
            Self::Unknown => "unknown",
            Self::Superseded => "superseded",
        }
    }
    pub(crate) fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "dispatching" => Some(Self::Dispatching),
            "returned" => Some(Self::Returned),
            "blocked" => Some(Self::Blocked),
            "rejected" => Some(Self::Rejected),
            "unknown" => Some(Self::Unknown),
            "superseded" => Some(Self::Superseded),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    Returned { detail: Option<String> },
    Blocked { detail: String },
    Rejected { detail: String },
    Unknown { detail: String },
}

impl DispatchOutcome {
    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Returned { detail } => {
                validate_input_size("dispatch detail", detail)?;
                validate_optional("dispatch detail", detail.as_deref())
            }
            Self::Blocked { detail } | Self::Rejected { detail } | Self::Unknown { detail } => {
                validate_input_size("dispatch detail", detail)?;
                validate_required("dispatch detail", detail)
            }
        }
    }
    pub(crate) fn columns(&self) -> (DispatchAttemptState, Option<&str>) {
        match self {
            Self::Returned { detail } => (DispatchAttemptState::Returned, detail.as_deref()),
            Self::Blocked { detail } => (DispatchAttemptState::Blocked, Some(detail)),
            Self::Rejected { detail } => (DispatchAttemptState::Rejected, Some(detail)),
            Self::Unknown { detail } => (DispatchAttemptState::Unknown, Some(detail)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchAttempt {
    pub id: DispatchAttemptId,
    pub request_id: AgentRequestId,
    pub runtime_binding_id: RuntimeBindingId,
    pub ordinal: u32,
    pub assignment: AgentAssignment,
    pub state: DispatchAttemptState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    pub id: AgentRequestId,
    pub context_id: ReviewContextId,
    pub observation_id: ObservationId,
    pub runtime_binding_id: RuntimeBindingId,
    pub ordinal: u32,
    pub state: AgentRequestState,
    pub thread_ids: Vec<ThreadId>,
    pub message_ids: Vec<MessageId>,
    pub answered_thread_ids: Vec<ThreadId>,
    pub recovery: Option<RequestRecovery>,
    pub attempts: Vec<DispatchAttempt>,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestRecovery {
    Retry,
    InspectBeforeRetry,
}

impl AgentRequest {
    /// Recovery is independent of the request's position in the context.
    pub fn recovery_for(&self, binding: &RuntimeBindingId) -> Option<RequestRecovery> {
        if self.answered_thread_ids.len() == self.thread_ids.len() {
            return None;
        }
        let attempt = self.attempts.last()?;
        match attempt.state {
            DispatchAttemptState::Rejected => Some(RequestRecovery::Retry),
            DispatchAttemptState::Blocked
            | DispatchAttemptState::Unknown
            | DispatchAttemptState::Superseded
            | DispatchAttemptState::Returned => Some(RequestRecovery::InspectBeforeRetry),
            DispatchAttemptState::Pending if attempt.runtime_binding_id != *binding => {
                Some(RequestRecovery::Retry)
            }
            DispatchAttemptState::Dispatching if attempt.runtime_binding_id != *binding => {
                Some(RequestRecovery::InspectBeforeRetry)
            }
            DispatchAttemptState::Pending | DispatchAttemptState::Dispatching => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchJob {
    pub comparison: CapturedComparison,
    pub request: AgentRequest,
    pub attempt: DispatchAttempt,
    pub threads: Vec<Thread>,
    /// Canonical bounded projection, ready to include verbatim in the prompt.
    pub projection_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingState {
    pub revision: u64,
    pub context: ReviewContext,
    pub observation: Observation,
    pub binding: RuntimeBinding,
    pub threads: Vec<Thread>,
    pub agent_requests: Vec<AgentRequest>,
    /// Continue loading this binding; repeated thread/request IDs are fragments.
    pub next_cursor: Option<BindingCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingCursor {
    pub revision: u64,
    pub observation_id: ObservationId,
    pub position: BindingPosition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BindingPosition {
    Threads {
        thread_id: ThreadId,
        after_message: u32,
    },
    Requests {
        request_id: AgentRequestId,
        after_attempt: u32,
    },
}

pub(crate) fn validate_required(field: &'static str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        Err(Error::EmptyField { field })
    } else {
        Ok(())
    }
}

pub(crate) fn validate_input_size(field: &'static str, value: &impl Serialize) -> Result<()> {
    const MAX_INPUT_BYTES: usize = 64 * 1024;
    let bytes = serde_json::to_vec(value).map_err(Error::EncodeRequestProjection)?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Error::InputTooLarge {
            field,
            limit: MAX_INPUT_BYTES,
        });
    }
    Ok(())
}

fn validate_optional(field: &'static str, value: Option<&str>) -> Result<()> {
    if value.is_some_and(|item| item.trim().is_empty()) {
        Err(Error::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_oid(oid: &str) -> Result<()> {
    let valid_length = oid.len() == 40 || oid.len() == 64;
    let lowercase_hex = oid
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid_length && lowercase_hex {
        Ok(())
    } else {
        Err(Error::InvalidCommitOid)
    }
}
