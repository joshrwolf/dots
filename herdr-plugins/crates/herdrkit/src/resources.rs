//! Typed optional host capability for plugin-owned workspace child rows.
use crate::socket::{Query, Request};
use crate::{Client, Result, WorkspaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub label: String,
    pub status: String,
    pub tone: Tone,
    pub completed: bool,
    pub action: String,
    pub reference: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Normal,
    Success,
    Warning,
    Failure,
    Muted,
}
#[derive(Debug, Clone, Deserialize)]
pub struct Collection {
    pub revision: u64,
    pub resources: Vec<Resource>,
}
#[derive(Debug, Deserialize)]
pub struct WorkspaceResources {
    pub workspace_id: WorkspaceId,
    pub identity: String,
    pub collections: BTreeMap<String, Collection>,
}
#[derive(Serialize)]
struct Get<'a> {
    workspace_id: &'a WorkspaceId,
}
impl Request for Get<'_> {
    const METHOD: &'static str = "workspace.resources.get";
}
#[derive(Deserialize)]
pub(crate) struct GetReply {
    resources: WorkspaceResources,
}
impl Query for Get<'_> {
    const TAG: &'static str = "workspace_resources";
    type Reply = GetReply;
}
#[derive(Debug, Serialize)]
pub struct Replace<'a> {
    pub workspace_id: &'a WorkspaceId,
    pub identity: &'a str,
    pub plugin_id: &'a str,
    pub expected_revision: u64,
    pub ttl_ms: u64,
    pub resources: &'a [Resource],
}
impl Request for Replace<'_> {
    const METHOD: &'static str = "workspace.resources.replace";
}
#[derive(Deserialize)]
pub(crate) struct ReplaceReply {
    collection: Collection,
}
impl Query for Replace<'_> {
    const TAG: &'static str = "workspace_resources_replaced";
    type Reply = ReplaceReply;
}
impl Client {
    pub fn workspace_resources(&self, workspace_id: &WorkspaceId) -> Result<WorkspaceResources> {
        self.call(&Get { workspace_id })
            .map(|reply| reply.resources)
    }
    pub fn replace_workspace_resources(&self, request: &Replace<'_>) -> Result<Collection> {
        self.call(request).map(|reply| reply.collection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Server, Step};
    use serde_json::json;
    #[test]
    fn typed_collection_calls_preserve_exact_scope_and_revision() {
        let server = Server::start(vec![
            Step::reply(
                r#"{"id":"{id}","result":{"type":"workspace_resources","resources":{"workspace_id":"w1","identity":"uuid","collections":{}}}}"#,
            ),
            Step::reply(
                r#"{"id":"{id}","result":{"type":"workspace_resources_replaced","collection":{"revision":1,"resources":[]}}}"#,
            ),
        ]);
        let client = server.client();
        let id = WorkspaceId::new("w1");
        assert_eq!(client.workspace_resources(&id).unwrap().identity, "uuid");
        client
            .replace_workspace_resources(&Replace {
                workspace_id: &id,
                identity: "uuid",
                plugin_id: "example",
                expected_revision: 0,
                ttl_ms: 120_000,
                resources: &[],
            })
            .unwrap();
        let requests = server.requests();
        assert_eq!(
            requests.get(1).unwrap().method,
            "workspace.resources.replace"
        );
        assert_eq!(
            requests.get(1).unwrap().params.get("expected_revision"),
            Some(&json!(0))
        );
    }
}
