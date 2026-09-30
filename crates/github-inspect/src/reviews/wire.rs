use serde::{Deserialize, Serialize};

pub(super) const INDEX: &str = r"query($owner:String!,$repo:String!,$number:Int!,$cursor:String){
 repository(owner:$owner,name:$repo){pullRequest(number:$number){
 url headRefOid reviewThreads(first:50,after:$cursor){
 totalCount nodes{id isResolved isOutdated path line startLine originalLine originalStartLine diffSide startDiffSide subjectType
 comments(first:1){totalCount nodes{id databaseId author{login} body url createdAt replyTo{id}}}}
 pageInfo{hasNextPage endCursor}}}}}";
pub(super) const THREAD: &str = r"query($id:ID!,$cursor:String){
 node(id:$id){... on PullRequestReviewThread{
 id isResolved isOutdated path line startLine originalLine originalStartLine diffSide startDiffSide subjectType
 pullRequest{url headRefOid}
 anchor:comments(first:1){nodes{url databaseId diffHunk originalCommit{oid} commit{oid}}}
 comments(first:25,after:$cursor){totalCount nodes{id databaseId author{login} body url createdAt replyTo{id}}
 pageInfo{hasNextPage endCursor}}}}}";

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Connection<T> {
    pub total_count: usize,
    pub nodes: Vec<T>,
    pub page_info: PageInfo,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct Pr {
    pub url: String,
    pub head_ref_oid: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IndexPr {
    pub url: String,
    pub head_ref_oid: String,
    pub review_threads: Connection<IndexThread>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IndexThread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    #[serde(flatten)]
    pub location: super::Location,
    pub comments: PreviewComments,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PreviewComments {
    pub total_count: usize,
    pub nodes: Vec<Comment>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Thread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    #[serde(flatten)]
    pub location: super::Location,
    pub pull_request: Pr,
    pub anchor: AnchorConnection,
    pub comments: Connection<Comment>,
}
#[derive(Debug, Deserialize)]
pub(super) struct AnchorConnection {
    pub nodes: Vec<Anchor>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Anchor {
    pub url: String,
    pub database_id: Option<u64>,
    pub diff_hunk: String,
    pub original_commit: Option<Commit>,
    pub commit: Option<Commit>,
}
#[derive(Debug, Deserialize)]
pub(super) struct Commit {
    pub oid: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Comment {
    pub id: String,
    pub database_id: Option<u64>,
    pub author: Option<Author>,
    pub body: String,
    pub url: String,
    pub created_at: String,
    pub reply_to: Option<Node>,
}
#[derive(Debug, Deserialize)]
pub(super) struct Author {
    pub login: String,
}
#[derive(Debug, Deserialize)]
pub(super) struct Node {
    pub id: String,
}
