//! Read-only review-thread discovery and exact, bounded conversation evidence.
#[cfg(test)]
mod tests;
mod wire;

use crate::target::{self, Resource};
use crate::{Error, ObservationSource, Provider, Repository, Result, SnapshotRef};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::HashSet, fmt, str::FromStr};
use unicode_segmentation::UnicodeSegmentation as _;
use wire::{Connection, IndexPr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ThreadRef {
    repository: Repository,
    pr: u64,
    head: String,
    node: String,
}
pub(crate) fn valid_node(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
impl FromStr for ThreadRef {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let fields: Vec<_> = value.split(':').collect();
        let ["thread", repo, pr, head, node] = fields.as_slice() else {
            return Err(target::invalid());
        };
        if !valid_node(node) {
            return Err(target::invalid());
        }
        Ok(Self {
            repository: repo.parse()?,
            pr: target::id(pr)?,
            head: target::sha(head)?,
            node: (*node).into(),
        })
    }
}
impl fmt::Display for ThreadRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "thread:{}:{}:{}:{}",
            self.repository, self.pr, self.head, self.node
        )
    }
}
impl From<ThreadRef> for String {
    fn from(value: ThreadRef) -> Self {
        value.to_string()
    }
}
impl TryFrom<String> for ThreadRef {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl ThreadRef {
    pub fn command(&self) -> String {
        format!("dots github reviews thread {self}")
    }
    fn url(&self) -> String {
        format!(
            "{}/pull/{}/review-threads/{}",
            self.repository.url(),
            self.pr,
            self.node
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Location {
    pub path: String,
    pub line: Option<u64>,
    #[serde(alias = "startLine")]
    pub start_line: Option<u64>,
    #[serde(alias = "originalLine")]
    pub original_line: Option<u64>,
    #[serde(alias = "originalStartLine")]
    pub original_start_line: Option<u64>,
    #[serde(alias = "diffSide")]
    pub diff_side: String,
    #[serde(alias = "startDiffSide")]
    pub start_diff_side: Option<String>,
    #[serde(alias = "subjectType")]
    pub subject_type: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub truncated: bool,
}
fn bounded(value: &str, limit: usize) -> Text {
    let mut text = String::new();
    for grapheme in value.graphemes(true) {
        if text.len() + grapheme.len() > limit {
            break;
        }
        text.push_str(grapheme);
    }
    Text {
        truncated: text.len() != value.len(),
        text,
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: Option<String>,
    pub body: Text,
    pub url: String,
    pub created_at: String,
    pub reply_to: Option<String>,
    pub full_comment_command: Option<String>,
}
impl Comment {
    fn from_wire(value: wire::Comment, repo: &Repository, limit: usize) -> Self {
        Self {
            id: value.id,
            author: value.author.map(|a| a.login),
            body: bounded(&value.body, limit),
            url: value.url,
            created_at: value.created_at,
            reply_to: value.reply_to.map(|n| n.id),
            full_comment_command: full_comment_command(value.database_id, repo),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preview {
    pub reference: ThreadRef,
    pub resolved: bool,
    pub outdated: bool,
    pub location: Location,
    pub comments_total: usize,
    pub preview: Option<Comment>,
    pub conversation_complete: bool,
    pub next_command: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub total: usize,
    pub unresolved: usize,
    pub unresolved_outdated: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subject {
    pub repository: Repository,
    pub pr: u64,
    pub head: String,
    pub url: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Index {
    subject: Subject,
    summary: Summary,
    threads: Vec<Preview>,
}
#[derive(Debug, Serialize)]
pub struct Listing {
    pub subject: Subject,
    pub source: ObservationSource,
    pub observed_at: u64,
    pub snapshot: Option<SnapshotRef>,
    pub summary: Summary,
    pub matching: usize,
    pub offset: usize,
    pub returned: usize,
    /// Whether this page contains the entire matching collection.
    pub complete: bool,
    /// Whether another page follows this one.
    pub has_more: bool,
    pub threads: Vec<Preview>,
    pub next_command: Option<String>,
    pub refresh_command: String,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Copy)]
pub struct ListOptions<'a> {
    pub pr: Option<&'a str>,
    pub repository: Option<&'a Repository>,
    pub snapshot: Option<&'a SnapshotRef>,
    pub all: bool,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub url: String,
    pub full_diff_hunk_command: Option<String>,
    pub original_commit_id: Option<String>,
    pub commit_id: Option<String>,
    pub diff_hunk: Text,
}
#[derive(Debug, Serialize, Deserialize)]
struct Conversation {
    reference: ThreadRef,
    current_head: String,
    resolved: bool,
    outdated: bool,
    location: Location,
    context: Option<Context>,
    comments: Vec<Comment>,
}
#[derive(Debug, Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "thread resolution and age are independent of page coverage and continuation"
)]
pub struct Thread {
    pub reference: ThreadRef,
    pub subject: Subject,
    pub observed_head: String,
    pub source: ObservationSource,
    pub observed_at: u64,
    pub snapshot: Option<SnapshotRef>,
    pub resolved: bool,
    pub outdated: bool,
    pub location: Location,
    pub context: Option<Context>,
    /// Recover the opening page and diff context without repeating it on reply pages.
    pub context_command: Option<String>,
    pub comments_total: usize,
    pub offset: usize,
    pub returned: usize,
    /// Whether this page contains the entire matching collection.
    pub complete: bool,
    /// Whether another page follows this one.
    pub has_more: bool,
    pub comments: Vec<Comment>,
    pub next_command: Option<String>,
    pub refresh_command: String,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Copy)]
pub struct ThreadOptions<'a> {
    pub snapshot: Option<&'a SnapshotRef>,
    pub offset: usize,
    pub limit: usize,
}

pub fn list(provider: &impl Provider, options: ListOptions<'_>) -> Result<Listing> {
    window(options.offset, options.limit)?;
    let (index, observed_at, snapshot, source) = if let Some(snapshot) = options.snapshot {
        if options.pr.is_some() || options.repository.is_some() {
            return Err(target::invalid());
        }
        let (repo, Resource::ReviewThreads(pr)) = Resource::parse(&snapshot.url())? else {
            return Err(target::invalid());
        };
        let refresh = list_refresh(&format!("{}/pull/{pr}", repo.url()), &options);
        let (index, time): (Index, _) = load(provider, snapshot, &snapshot.url(), &refresh)?;
        if index.subject.repository != repo || index.subject.pr != pr {
            return Err(target::invalid());
        }
        (
            index,
            time,
            Some(snapshot.clone()),
            ObservationSource::Snapshot,
        )
    } else {
        let (repo, pr) = resolve(provider, options.pr, options.repository)?;
        let index = collect_index(provider, &repo, pr)?;
        let time = crate::cache::now();
        let snapshot = save(
            provider,
            &format!("{}/pull/{pr}/review-threads", repo.url()),
            &index,
            time,
        );
        (index, time, snapshot, ObservationSource::Network)
    };
    let refresh = list_refresh(&index.subject.url, &options);
    let mut command = snapshot.as_ref().map_or_else(
        || format!("dots github reviews --pr {}", index.subject.url),
        |s| format!("dots github reviews --snapshot {s}"),
    );
    if options.all {
        command.push_str(" --all");
    }
    let matching = index
        .threads
        .iter()
        .filter(|t| options.all || !t.resolved)
        .count();
    let threads: Vec<_> = index
        .threads
        .into_iter()
        .filter(|t| options.all || !t.resolved)
        .skip(options.offset)
        .take(options.limit)
        .collect();
    let end = options.offset.saturating_add(threads.len());
    let warnings = cache_warning(snapshot.is_some());
    Ok(Listing {
        subject: index.subject,
        summary: index.summary,
        source,
        observed_at,
        snapshot,
        matching,
        offset: options.offset,
        returned: threads.len(),
        complete: options.offset == 0 && threads.len() == matching,
        has_more: end < matching,
        threads,
        next_command: (end < matching)
            .then(|| format!("{command} --offset {end} --limit {}", options.limit)),
        refresh_command: refresh,
        warnings,
    })
}
pub fn thread(
    provider: &impl Provider,
    reference: &ThreadRef,
    options: ThreadOptions<'_>,
) -> Result<Thread> {
    window(options.offset, options.limit)?;
    let refresh = format!("{} --limit {}", reference.command(), options.limit);
    let (conversation, observed_at, snapshot, source) = if let Some(snapshot) = options.snapshot {
        let (saved, time): (Conversation, _) =
            load(provider, snapshot, &reference.url(), &refresh)?;
        if saved.reference != *reference {
            return Err(target::invalid());
        }
        (
            saved,
            time,
            Some(snapshot.clone()),
            ObservationSource::Snapshot,
        )
    } else {
        let saved = collect_thread(provider, reference)?;
        let time = crate::cache::now();
        let snapshot = save(provider, &reference.url(), &saved, time);
        (saved, time, snapshot, ObservationSource::Network)
    };
    let command = snapshot
        .as_ref()
        .map_or_else(|| refresh.clone(), |s| format!("{refresh} --snapshot {s}"));
    let total = conversation.comments.len();
    let comments: Vec<_> = conversation
        .comments
        .into_iter()
        .skip(options.offset)
        .take(options.limit)
        .collect();
    let end = options.offset.saturating_add(comments.len());
    let mut warnings = cache_warning(snapshot.is_some());
    if conversation.current_head != reference.head {
        warnings.push("PR head changed since thread discovery; original reference and reviewed context are preserved.".into());
    }
    Ok(Thread {
        reference: reference.clone(),
        subject: Subject {
            repository: reference.repository.clone(),
            pr: reference.pr,
            head: conversation.current_head,
            url: format!("{}/pull/{}", reference.repository.url(), reference.pr),
        },
        observed_head: reference.head.clone(),
        source,
        observed_at,
        snapshot,
        resolved: conversation.resolved,
        outdated: conversation.outdated,
        location: conversation.location,
        context_command: conversation
            .context
            .as_ref()
            .filter(|_| options.offset > 0)
            .map(|_| format!("{command} --offset 0")),
        context: conversation.context.filter(|_| options.offset == 0),
        comments_total: total,
        offset: options.offset,
        returned: comments.len(),
        complete: options.offset == 0 && comments.len() == total,
        has_more: end < total,
        comments,
        next_command: (end < total).then(|| format!("{command} --offset {end}")),
        refresh_command: refresh,
        warnings,
    })
}
fn list_refresh(url: &str, options: &ListOptions<'_>) -> String {
    format!(
        "dots github reviews --pr {url}{} --limit {}",
        if options.all { " --all" } else { "" },
        options.limit
    )
}
fn window(offset: usize, limit: usize) -> Result<()> {
    if !(1..=20).contains(&limit) || offset > 100_000 {
        return Err(Error::InvalidResponse(
            "limit must be 1..=20 and offset at most 100000".into(),
        ));
    }
    Ok(())
}
#[derive(Deserialize)]
struct Current {
    url: String,
}
fn resolve(
    provider: &impl Provider,
    value: Option<&str>,
    explicit: Option<&Repository>,
) -> Result<(Repository, u64)> {
    if let Some(value) = value {
        if value.starts_with("https://") {
            let (repo, Resource::Pr(pr)) = Resource::parse(value)? else {
                return Err(target::invalid());
            };
            return Ok((target::repository(provider, explicit, Some(repo))?, pr));
        }
        return Ok((
            target::repository(provider, explicit, None)?,
            target::id(value)?,
        ));
    }
    if explicit.is_some() {
        return Err(Error::InvalidReference("--repo requires --pr".into()));
    }
    let current: Current = serde_json::from_slice(&crate::provider::checked(
        provider,
        &["pr", "view", "--json", "url"],
    )?)?;
    resolve(provider, Some(&current.url), None)
}
#[derive(Deserialize)]
struct Response<T> {
    data: Option<T>,
    errors: Option<Vec<serde_json::Value>>,
}
fn query<T: DeserializeOwned>(
    provider: &impl Provider,
    repo: &Repository,
    query: &str,
    variables: &[String],
) -> Result<T> {
    let mut args = vec!["api", "graphql", "--hostname", &repo.host, "-f", query];
    for variable in variables {
        let flag = if variable.starts_with("number=") || variable == "cursor=null" {
            "-F"
        } else {
            "-f"
        };
        args.extend([flag, variable]);
    }
    let bytes = crate::provider::checked(provider, &args)?;
    let response: Response<T> = serde_json::from_slice(&bytes)?;
    if response
        .errors
        .as_ref()
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err(Error::InvalidResponse(format!(
            "GitHub review query failed: {}",
            serde_json::to_string(&response.errors)?
        )));
    }
    response.data.ok_or_else(|| {
        Error::InvalidResponse("GitHub omitted review data; check repository access".into())
    })
}
#[derive(Default)]
struct Pages {
    cursor: Option<String>,
    cursors: HashSet<String>,
    ids: HashSet<String>,
    total: Option<usize>,
}
impl Pages {
    fn accept<T>(
        &mut self,
        page: &Connection<T>,
        ids: impl Iterator<Item = String>,
    ) -> Result<bool> {
        if self.total.is_some_and(|total| total != page.total_count) {
            return Err(changed());
        }
        self.total = Some(page.total_count);
        for id in ids {
            if !valid_node(&id) || !self.ids.insert(id) {
                return Err(changed());
            }
        }
        if !page.page_info.has_next_page {
            if self.ids.len() != page.total_count {
                return Err(changed());
            }
            return Ok(false);
        }
        let cursor = page
            .page_info
            .end_cursor
            .as_ref()
            .filter(|s| !s.is_empty())
            .ok_or_else(changed)?;
        if page.nodes.is_empty()
            || self.ids.len() >= page.total_count
            || !self.cursors.insert(cursor.clone())
        {
            return Err(changed());
        }
        self.cursor = Some(cursor.clone());
        Ok(true)
    }
    fn cursor(&self) -> String {
        format!("cursor={}", self.cursor.as_deref().unwrap_or("null"))
    }
}
fn changed() -> Error {
    Error::InvalidResponse(
        "Review traversal incomplete or changed while paging; refresh the query".into(),
    )
}

fn collect_index(provider: &impl Provider, repo: &Repository, number: u64) -> Result<Index> {
    #[derive(Deserialize)]
    struct Data {
        repository: Option<Repo>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repo {
        pull_request: Option<IndexPr>,
    }
    let mut pages = Pages::default();
    let mut threads = Vec::new();
    let mut head = None;
    for _ in 0..20 {
        let data: Data = query(
            provider,
            repo,
            &format!("query={}", wire::INDEX),
            &[
                format!("owner={}", repo.owner),
                format!("repo={}", repo.repo),
                format!("number={number}"),
                pages.cursor(),
            ],
        )?;
        let pr = data
            .repository
            .and_then(|r| r.pull_request)
            .ok_or_else(changed)?;
        let current = target::sha(&pr.head_ref_oid)?;
        if !matches!(Resource::parse(&pr.url), Ok((actual, Resource::Pr(id))) if actual == *repo && id == number)
            || head.as_ref().is_some_and(|old| old != &current)
        {
            return Err(changed());
        }
        head = Some(current.clone());
        let more = pages.accept(
            &pr.review_threads,
            pr.review_threads.nodes.iter().map(|t| t.id.clone()),
        )?;
        for item in pr.review_threads.nodes {
            if item.comments.nodes.len() != item.comments.total_count.min(1) {
                return Err(changed());
            }
            let reference = ThreadRef {
                repository: repo.clone(),
                pr: number,
                head: current.clone(),
                node: item.id,
            };
            threads.push(Preview {
                next_command: reference.command(),
                reference,
                resolved: item.is_resolved,
                outdated: item.is_outdated,
                location: item.location,
                comments_total: item.comments.total_count,
                preview: item
                    .comments
                    .nodes
                    .into_iter()
                    .next()
                    .map(|c| Comment::from_wire(c, repo, 512)),
                conversation_complete: item.comments.total_count <= 1,
            });
        }
        if !more {
            return Ok(Index {
                subject: Subject {
                    repository: repo.clone(),
                    pr: number,
                    head: current,
                    url: format!("{}/pull/{number}", repo.url()),
                },
                summary: Summary {
                    total: threads.len(),
                    unresolved: threads.iter().filter(|t| !t.resolved).count(),
                    unresolved_outdated: threads
                        .iter()
                        .filter(|t| !t.resolved && t.outdated)
                        .count(),
                },
                threads,
            });
        }
    }
    Err(Error::InvalidResponse(
        "Review index exceeds the 1000-thread query budget; use gh pr view --web".into(),
    ))
}
fn collect_thread(provider: &impl Provider, reference: &ThreadRef) -> Result<Conversation> {
    #[derive(Deserialize)]
    struct Data {
        node: Option<wire::Thread>,
    }
    let mut pages = Pages::default();
    let mut saved: Option<Conversation> = None;
    for _ in 0..8 {
        let data: Data = query(
            provider,
            &reference.repository,
            &format!("query={}", wire::THREAD),
            &[format!("id={}", reference.node), pages.cursor()],
        )?;
        let value = data.node.ok_or_else(changed)?;
        let current_head = target::sha(&value.pull_request.head_ref_oid)?;
        if value.id != reference.node
            || !matches!(Resource::parse(&value.pull_request.url), Ok((repo, Resource::Pr(id))) if repo == reference.repository && id == reference.pr)
        {
            return Err(changed());
        }
        let more = pages.accept(
            &value.comments,
            value.comments.nodes.iter().map(|c| c.id.clone()),
        )?;
        if let Some(saved) = &saved
            && (saved.current_head != current_head
                || saved.location != value.location
                || saved.resolved != value.is_resolved
                || saved.outdated != value.is_outdated)
        {
            return Err(changed());
        }
        let conversation = saved.get_or_insert_with(|| Conversation {
            reference: reference.clone(),
            current_head,
            resolved: value.is_resolved,
            outdated: value.is_outdated,
            location: value.location,
            context: value.anchor.nodes.into_iter().next().map(|a| Context {
                url: a.url,
                full_diff_hunk_command: full_comment_command(a.database_id, &reference.repository),
                original_commit_id: a.original_commit.map(|c| c.oid),
                commit_id: a.commit.map(|c| c.oid),
                diff_hunk: bounded(&a.diff_hunk, 4096),
            }),
            comments: Vec::new(),
        });
        conversation.comments.extend(
            value
                .comments
                .nodes
                .into_iter()
                .map(|c| Comment::from_wire(c, &reference.repository, 4096)),
        );
        if !more {
            return saved.ok_or_else(changed);
        }
    }
    Err(Error::InvalidResponse(
        "Conversation exceeds the 200-comment query budget; open the thread on GitHub".into(),
    ))
}
#[derive(Serialize, Deserialize)]
struct Saved<T> {
    observed_at: u64,
    value: T,
}
fn save<T: Serialize>(
    provider: &impl Provider,
    url: &str,
    value: &T,
    time: u64,
) -> Option<SnapshotRef> {
    let bytes = serde_json::to_vec(&Saved {
        observed_at: time,
        value,
    })
    .ok()?;
    let reference = SnapshotRef::create(url, &bytes)?;
    provider
        .save_snapshot(&reference, &bytes, time)
        .then_some(reference)
}
fn load<T: DeserializeOwned>(
    provider: &impl Provider,
    reference: &SnapshotRef,
    url: &str,
    refresh: &str,
) -> Result<(T, u64)> {
    let invalid = || {
        Error::InvalidResponse(format!(
            "Saved reviews missing, expired or invalid; refresh with {refresh}"
        ))
    };
    let cached = provider.read_snapshot(reference).map_err(|e| {
        Error::InvalidResponse(format!("{}; refresh with {refresh}", e.diagnostic()))
    })?;
    if SnapshotRef::create(url, &cached.bytes)
        .is_none_or(|actual| actual.to_string() != reference.to_string())
    {
        return Err(invalid());
    }
    let saved: Saved<T> = serde_json::from_slice(&cached.bytes).map_err(|_| invalid())?;
    if crate::cache::now()
        .checked_sub(saved.observed_at)
        .is_none_or(|age| age >= 300)
    {
        return Err(invalid());
    }
    Ok((saved.value, saved.observed_at))
}

fn cache_warning(saved: bool) -> Vec<String> {
    if saved {
        vec![]
    } else {
        vec!["Observation not saved; follow-up paging will query GitHub again.".into()]
    }
}

fn full_comment_command(id: Option<u64>, repo: &Repository) -> Option<String> {
    id.filter(|id| *id > 0).map(|id| {
        format!(
            "gh api --hostname {} {}",
            repo.host,
            repo.endpoint(&format!("pulls/comments/{id}"))
        )
    })
}
