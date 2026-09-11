use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use herdrkit::api::{AgentRef, AgentStatus, Repository, Snapshot, Sound};
use herdrkit::{Client, TabId, WorkspaceId};
use serde::{Deserialize, Serialize};

use crate::ci::brief;
use crate::github::{self, CiState, CiVerdict, Lookup, PullRequestState};
use crate::lock::{FileLock, stable_hash};

const SETTLED: &[AgentStatus] = &[AgentStatus::Idle, AgentStatus::Done, AgentStatus::Blocked];

#[derive(Debug, Clone, Copy)]
pub struct Config {
    max_attempts: u64,
    fix_timeout: Duration,
    watch_interval: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            max_attempts: number("HERDR_GITHUB_CI_MAX_ATTEMPTS", 3)?,
            fix_timeout: Duration::from_secs(number("HERDR_GITHUB_CI_FIX_TIMEOUT", 1_800)?),
            watch_interval: Duration::from_secs(number("HERDR_GITHUB_CI_WATCH_INTERVAL", 20)?),
        })
    }
}

pub fn fix_now(client: &Client, dir: &Path, state_dir: &Path, config: Config) -> Result<()> {
    let Some(initial) = actionable(&ProductionCiSource, dir)? else {
        println!("checks are not failed; nothing to fix");
        return Ok(());
    };
    let started = start(
        client,
        &ProductionCiSource,
        dir,
        state_dir,
        &initial,
        config,
        LaunchMode::Detached,
    )?;
    if started.new {
        println!(
            "{} started in {} on {} (attempt {}/{})",
            started.agent,
            started.workspace,
            short(&started.head_oid),
            started.attempt,
            config.max_attempts
        );
    }
    Ok(())
}

pub fn arm(client: &Client, dir: &Path, state_dir: &Path, config: Config) -> Result<()> {
    loop {
        let lookup = github::lookup(dir)?;
        let Lookup::Found(state) = lookup else {
            println!("no pull request for this branch; nothing to watch");
            return Ok(());
        };
        if state.pull_request_state != PullRequestState::Open {
            println!(
                "PR #{} is {}; nothing to watch",
                state.number,
                pull_request_state_name(&state)
            );
            return Ok(());
        }
        match state.verdict {
            CiVerdict::Green => {
                notify(
                    client,
                    "GitHub CI green",
                    &format!("PR #{} passed", state.number),
                    Sound::Done,
                );
                println!("PR #{} is green", state.number);
                return Ok(());
            }
            CiVerdict::Pending => {
                github::wait_for_checks(dir, config.watch_interval, state.has_required_checks())?;
            }
            CiVerdict::Failed => {
                let started = start(
                    client,
                    &ProductionCiSource,
                    dir,
                    state_dir,
                    &state,
                    config,
                    LaunchMode::WaitForSettlement,
                )?;
                let after = github::pull_request(dir)?
                    .context("the pull request disappeared while its fixer was running")?;
                if after.number != state.number {
                    bail!(
                        "the branch changed from PR #{} to PR #{} while its fixer was running",
                        state.number,
                        after.number
                    );
                }
                if after.head_oid == started.head_oid {
                    notify(
                        client,
                        "GitHub CI autofix stalled",
                        &format!("PR #{} — fixer did not push", state.number),
                        Sound::Request,
                    );
                    bail!("fixer {} settled without pushing", started.agent);
                }
            }
        }
    }
}

trait CiSource {
    fn lookup(&self, dir: &Path) -> Result<Lookup>;
    fn build_brief(&self, dir: &Path, state: &CiState) -> Result<String>;
}

#[derive(Debug, Clone, Copy)]
struct ProductionCiSource;

impl CiSource for ProductionCiSource {
    fn lookup(&self, dir: &Path) -> Result<Lookup> {
        github::lookup(dir)
    }

    fn build_brief(&self, dir: &Path, state: &CiState) -> Result<String> {
        brief::build_for_state(dir, state)
    }
}

fn actionable(source: &impl CiSource, dir: &Path) -> Result<Option<CiState>> {
    match source.lookup(dir)? {
        Lookup::Found(state)
            if state.pull_request_state == PullRequestState::Open
                && state.verdict == CiVerdict::Failed =>
        {
            Ok(Some(state))
        }
        Lookup::Found(_) | Lookup::NoPullRequest => Ok(None),
    }
}

#[derive(Debug, Clone, Copy)]
enum LaunchMode {
    Detached,
    WaitForSettlement,
}

#[derive(Debug)]
struct Started {
    agent: String,
    workspace: WorkspaceId,
    attempt: u64,
    head_oid: String,
    new: bool,
}

fn start(
    client: &Client,
    source: &impl CiSource,
    dir: &Path,
    state_dir: &Path,
    initial: &CiState,
    config: Config,
    mode: LaunchMode,
) -> Result<Started> {
    let snapshot = client.snapshot().context("reading the session")?;
    let (_, repository) = checkout_identity(&snapshot, dir)?;
    let state_key = state_key(&repository);
    let _lock = FileLock::acquire(
        &state_dir.join(format!("{state_key}-{}.lock", initial.number)),
        Duration::from_secs(5),
    )?;

    let snapshot = client.snapshot().context("re-reading the session")?;
    let (workspace, current_repository) = checkout_identity(&snapshot, dir)?;
    if current_repository.key != repository.key {
        bail!(
            "the repository at {} changed from {:?} to {:?} while waiting for its lock",
            dir.display(),
            repository.key,
            current_repository.key
        );
    }

    // GitHub and the checkout can both change while lock acquisition blocks.
    // This is the authoritative state for every subsequent side effect and for
    // the brief handed to the fixer.
    let current = source
        .lookup(dir)
        .context("revalidating GitHub CI under the autofix lock")?;
    let Lookup::Found(current) = current else {
        bail!("the pull request disappeared while waiting for its autofix lock");
    };
    validate_current(initial, &current)?;

    let transaction = StartTransaction {
        client,
        source,
        dir,
        state_dir,
        current: &current,
        snapshot: &snapshot,
        workspace: &workspace,
        repository: &repository,
        state_key: &state_key,
        config,
        mode,
    };
    if let Some(existing) = transaction.recover_existing()? {
        return Ok(existing);
    }
    transaction.launch()
}

struct StartTransaction<'a, S> {
    client: &'a Client,
    source: &'a S,
    dir: &'a Path,
    state_dir: &'a Path,
    current: &'a CiState,
    snapshot: &'a Snapshot,
    workspace: &'a WorkspaceId,
    repository: &'a Repository,
    state_key: &'a str,
    config: Config,
    mode: LaunchMode,
}

impl<S: CiSource> StartTransaction<'_, S> {
    fn recover_existing(&self) -> Result<Option<Started>> {
        let path = journal_path(self.state_dir, self.state_key, self.current.number);
        let mut journal = read_journal(&path)?;
        if let Some(pending) = journal.pending.clone()
            && let Some(running) = self
                .snapshot
                .agents
                .iter()
                .find(|running| running.name.as_deref() == Some(pending.agent.as_str()))
        {
            commit_attempt(&path, &pending)?;
            journal = read_journal(&path)?;
            if is_running(running.agent_status) {
                self.wait_for_existing(&pending.agent)?;
                return Ok(Some(self.existing(pending.agent, pending.attempt)));
            }
        }

        let agent = self
            .snapshot
            .agents
            .iter()
            .filter(|running| is_running(running.agent_status))
            .filter_map(|running| running.name.as_deref())
            .find(|name| is_agent_for(name, self.repository, self.current.number));
        let Some(agent) = agent else {
            return Ok(None);
        };
        self.wait_for_existing(agent)?;
        Ok(Some(self.existing(agent.to_owned(), journal.committed)))
    }

    fn launch(&self) -> Result<Started> {
        let brief = self.source.build_brief(self.dir, self.current)?;
        let confirmed = self
            .source
            .lookup(self.dir)
            .context("confirming GitHub CI immediately before autofix launch")?;
        let Lookup::Found(confirmed) = confirmed else {
            bail!("the pull request disappeared while its CI brief was being built");
        };
        validate_current(self.current, &confirmed)?;
        if confirmed != *self.current {
            bail!("GitHub CI changed while its autofix brief was being built");
        }

        let journal_path = journal_path(self.state_dir, self.state_key, self.current.number);
        let pending = reserve_attempt(
            &journal_path,
            self.repository,
            self.current.number,
            self.config.max_attempts,
        )?;
        let brief_path = self.state_dir.join(format!(
            "{}-{}-{}.md",
            self.state_key,
            self.current.number,
            short(&self.current.local_head_oid)
        ));
        fs::write(&brief_path, brief)
            .with_context(|| format!("writing {}", brief_path.display()))?;

        let created =
            match self
                .client
                .tab_create(self.workspace, self.dir, Some("github-ci-fix"), false)
            {
                Ok(created) => created,
                Err(error) => {
                    clear_pending(&journal_path, &pending)?;
                    remove_brief(&brief_path);
                    return Err(error)
                        .with_context(|| format!("creating a fixer tab in {}", self.workspace));
                }
            };
        let prompt_text = self.prompt_text(&brief_path, pending.attempt);
        let launch = self
            .client
            .agent_start(
                &pending.agent,
                "claude",
                &created.root_pane.pane_id,
                Some(Duration::from_secs(30)),
            )
            .with_context(|| format!("starting {}", pending.agent))
            .and_then(|_| {
                prompt(
                    self.client,
                    &pending.agent,
                    &prompt_text,
                    self.config.fix_timeout,
                    self.mode,
                )
            });
        if let Err(error) = launch {
            if close_after_failure(self.client, &created.tab.tab_id) {
                clear_pending(&journal_path, &pending)?;
            }
            remove_brief(&brief_path);
            return Err(error);
        }
        commit_attempt(&journal_path, &pending)?;
        notify(
            self.client,
            "GitHub CI autofix started",
            &format!(
                "PR #{} — attempt {}/{}",
                self.current.number, pending.attempt, self.config.max_attempts
            ),
            Sound::Request,
        );
        Ok(Started {
            agent: pending.agent,
            workspace: self.workspace.clone(),
            attempt: pending.attempt,
            head_oid: self.current.remote_head_oid.clone(),
            new: true,
        })
    }

    fn existing(&self, agent: String, attempt: u64) -> Started {
        Started {
            agent,
            workspace: self.workspace.clone(),
            attempt,
            head_oid: self.current.remote_head_oid.clone(),
            new: false,
        }
    }

    fn wait_for_existing(&self, agent: &str) -> Result<()> {
        wait_for_existing_if_requested(self.client, agent, self.config.fix_timeout, self.mode)
    }

    fn prompt_text(&self, brief_path: &Path, attempt: u64) -> String {
        format!(
            "Read {} and fix the failing GitHub CI checks it describes, here in this worktree. Commit and push when the fix is complete. This is attempt {attempt} of {}. If a check is not a GitHub Actions check, do not guess at the cause — report its URL and stop.",
            brief_path.display(),
            self.config.max_attempts
        )
    }
}

fn validate_current(initial: &CiState, current: &CiState) -> Result<()> {
    if current.number != initial.number {
        bail!(
            "the branch changed from PR #{} to PR #{} while waiting for its autofix lock",
            initial.number,
            current.number
        );
    }
    if current.pull_request_state != PullRequestState::Open {
        bail!("PR #{} is no longer open", current.number);
    }
    if current.verdict != CiVerdict::Failed {
        bail!(
            "PR #{} no longer has actionable CI failures",
            current.number
        );
    }
    if current.remote_head_oid != initial.remote_head_oid {
        bail!(
            "PR #{} advanced while waiting for its autofix lock",
            current.number
        );
    }
    if current.local_head_oid != initial.local_head_oid {
        bail!("the local checkout changed while waiting for its autofix lock");
    }
    if current.local_head_oid != current.remote_head_oid {
        bail!(
            "the local checkout is not at PR #{}'s head; refusing to launch a stale fixer",
            current.number
        );
    }
    Ok(())
}

fn prompt(
    client: &Client,
    agent: &str,
    text: &str,
    timeout: Duration,
    mode: LaunchMode,
) -> Result<()> {
    match mode {
        LaunchMode::Detached => client
            .agent_prompt(AgentRef::Name(agent), text)
            .with_context(|| format!("prompting {agent}"))
            .map(|_| ()),
        LaunchMode::WaitForSettlement => client
            .agent_prompt_and_wait(AgentRef::Name(agent), text, SETTLED, Some(timeout))
            .with_context(|| format!("prompting and waiting for {agent}"))
            .map(|_| ()),
    }
}

fn wait_for_existing_if_requested(
    client: &Client,
    agent: &str,
    timeout: Duration,
    mode: LaunchMode,
) -> Result<()> {
    if matches!(mode, LaunchMode::WaitForSettlement) {
        client
            .agent_wait(AgentRef::Name(agent), SETTLED, Some(timeout))
            .with_context(|| format!("waiting for existing fixer {agent}"))?;
    }
    Ok(())
}

fn is_running(status: AgentStatus) -> bool {
    matches!(status, AgentStatus::Working | AgentStatus::Unknown)
}

fn checkout_identity(snapshot: &Snapshot, dir: &Path) -> Result<(WorkspaceId, Repository)> {
    let workspace = snapshot
        .workspace_for_checkout(dir)
        .with_context(|| format!("no herdr workspace is rooted at {}", dir.display()))?;
    let repository = workspace
        .worktree
        .as_ref()
        .map(|worktree| worktree.repository.clone())
        .context("the matching herdr workspace has no repository identity")?;
    Ok((workspace.workspace_id.clone(), repository))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PendingAttempt {
    attempt: u64,
    agent: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct AttemptJournal {
    committed: u64,
    pending: Option<PendingAttempt>,
}

fn journal_path(state_dir: &Path, repository: &str, pr: u64) -> PathBuf {
    state_dir.join(format!("{repository}-{pr}.attempts.json"))
}

fn reserve_attempt(
    path: &Path,
    repository: &Repository,
    pr: u64,
    max: u64,
) -> Result<PendingAttempt> {
    let mut journal = read_journal(path)?;
    if let Some(pending) = journal.pending {
        return Ok(pending);
    }
    let attempt = journal.committed.saturating_add(1);
    if attempt > max {
        bail!(
            "attempt cap ({max}) reached for PR #{pr}; reset {} to retry",
            path.display()
        );
    }
    let pending = PendingAttempt {
        attempt,
        agent: agent_name(repository, pr, attempt),
    };
    journal.pending = Some(pending.clone());
    write_journal(path, &journal)?;
    Ok(pending)
}

fn commit_attempt(path: &Path, pending: &PendingAttempt) -> Result<()> {
    let mut journal = read_journal(path)?;
    if journal.pending.as_ref() != Some(pending) {
        bail!(
            "attempt journal changed while committing {}",
            path.display()
        );
    }
    journal.committed = journal.committed.max(pending.attempt);
    journal.pending = None;
    write_journal(path, &journal)
}

fn clear_pending(path: &Path, pending: &PendingAttempt) -> Result<()> {
    let mut journal = read_journal(path)?;
    if journal.pending.as_ref() == Some(pending) {
        journal.pending = None;
        write_journal(path, &journal)?;
    }
    Ok(())
}

fn read_journal(path: &Path) -> Result<AttemptJournal> {
    match fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AttemptJournal::default()),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_journal(path: &Path, journal: &AttemptJournal) -> Result<()> {
    let parent = path
        .parent()
        .context("attempt journal path has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let temporary = path.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .with_context(|| format!("opening {}", temporary.display()))?;
    serde_json::to_writer(&mut file, journal)
        .with_context(|| format!("writing {}", temporary.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("writing {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("syncing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("committing {}", path.display()))?;
    File::open(parent)
        .with_context(|| format!("opening {} for sync", parent.display()))?
        .sync_all()
        .with_context(|| format!("syncing {}", parent.display()))
}

fn close_after_failure(client: &Client, tab: &TabId) -> bool {
    if let Err(error) = client.tab_close(tab) {
        eprintln!("github: could not close failed fixer tab {tab}: {error:#}");
        return false;
    }
    true
}

fn remove_brief(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "github: could not remove failed fixer brief {}: {error:#}",
            path.display()
        );
    }
}

fn notify(client: &Client, title: &str, body: &str, sound: Sound) {
    match client.notify(title, Some(body), sound) {
        Ok(notification) if notification.shown => {}
        Ok(notification) => eprintln!(
            "github: notification was not shown: {:?}",
            notification.reason
        ),
        Err(error) => eprintln!("github: could not show notification: {error:#}"),
    }
}

fn agent_prefix(repository: &Repository, pr: u64) -> String {
    let repo: String = repository_slug(&repository.name).chars().take(4).collect();
    format!(
        "ghci-{repo}-{:012x}-",
        stable_hash(format!("{}#{pr}", repository.key).as_bytes()) & 0x0000_ffff_ffff_ffff
    )
}

fn agent_name(repository: &Repository, pr: u64, attempt: u64) -> String {
    format!(
        "{}{:08x}",
        agent_prefix(repository, pr),
        stable_hash(attempt.to_string().as_bytes()) & 0xffff_ffff
    )
}

fn is_agent_for(name: &str, repository: &Repository, pr: u64) -> bool {
    name.starts_with(&agent_prefix(repository, pr))
}

fn state_key(repository: &Repository) -> String {
    const MAX_SLUG_LEN: usize = 64;
    let slug: String = repository_slug(&repository.name)
        .chars()
        .take(MAX_SLUG_LEN)
        .collect();
    format!("{}-{:016x}", slug, stable_hash(repository.key.as_bytes()))
}

fn repository_slug(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "repo".to_owned()
    } else {
        slug.to_owned()
    }
}

fn short(head: &str) -> &str {
    head.get(..9).unwrap_or(head)
}

fn pull_request_state_name(state: &CiState) -> &'static str {
    match state.pull_request_state {
        PullRequestState::Open => "open",
        PullRequestState::Closed => "closed",
        PullRequestState::Merged => "merged",
    }
}

fn number(name: &str, default: u64) -> Result<u64> {
    let value = match std::env::var(name) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("{name} must be a positive integer"))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(std::env::VarError::NotUnicode(value)) => {
            bail!("{name} is not valid Unicode: {}", value.display())
        }
    };
    if value == 0 {
        bail!("{name} must be a positive integer");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    use herdrkit::testing::{Server, Step};

    use super::*;
    use crate::github::{Check, CheckBucket};

    const CHECKOUT: &str = "/tmp/herdr-github-transaction-repo";
    const SNAPSHOT: &str = r#"{"id":"{id}","result":{"type":"session_snapshot","snapshot":{"version":"0.9.0","protocol":22,"workspaces":[{"workspace_id":"w1","label":"service","focused":true,"agent_status":"idle","worktree":{"repo_key":"gh:example/service","repo_name":"service","repo_root":"/tmp/herdr-github-transaction-repo","checkout_path":"/tmp/herdr-github-transaction-repo","is_linked_worktree":false}}],"tabs":[],"panes":[],"agents":[],"focused_workspace_id":"w1","focused_tab_id":null,"focused_pane_id":null}}}"#;
    const CHANGED_REPOSITORY: &str = r#"{"id":"{id}","result":{"type":"session_snapshot","snapshot":{"version":"0.9.0","protocol":22,"workspaces":[{"workspace_id":"w2","label":"replacement","focused":true,"agent_status":"idle","worktree":{"repo_key":"gh:example/replacement","repo_name":"replacement","repo_root":"/tmp/herdr-github-transaction-repo","checkout_path":"/tmp/herdr-github-transaction-repo","is_linked_worktree":false}}],"tabs":[],"panes":[],"agents":[],"focused_workspace_id":"w2","focused_tab_id":null,"focused_pane_id":null}}}"#;
    const TAB_CREATED: &str = r#"{"id":"{id}","result":{"type":"tab_created","tab":{"tab_id":"w1:t2","workspace_id":"w1"},"root_pane":{"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t2"}}}"#;
    const AGENT_STARTED: &str = r#"{"id":"{id}","result":{"type":"agent_started","agent":{"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t2","agent_status":"working","name":"fixer"},"argv":["claude"]}}"#;
    const AGENT_PROMPTED: &str = r#"{"id":"{id}","result":{"type":"agent_prompted","agent":{"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t2","agent_status":"working","name":"fixer"}}}"#;
    const NOTIFIED: &str =
        r#"{"id":"{id}","result":{"type":"notification_show","shown":true,"reason":"shown"}}"#;
    const REJECTED: &str =
        r#"{"id":"{id}","error":{"code":"invalid_request","message":"injected failure"}}"#;
    const CLOSED: &str = r#"{"id":"{id}","result":{"type":"tab_closed"}}"#;

    fn reply(value: &'static str) -> Step {
        Step::reply(value.to_owned())
    }

    #[derive(Debug)]
    struct FakeCiSource {
        states: Mutex<VecDeque<CiState>>,
        brief_heads: Mutex<Vec<String>>,
    }

    impl FakeCiSource {
        fn new(states: Vec<CiState>) -> Self {
            Self {
                states: Mutex::new(states.into()),
                brief_heads: Mutex::new(Vec::new()),
            }
        }
    }

    impl CiSource for FakeCiSource {
        fn lookup(&self, _dir: &Path) -> Result<Lookup> {
            self.states
                .lock()
                .map_err(|_| anyhow::anyhow!("state lock poisoned"))?
                .pop_front()
                .map(Lookup::Found)
                .context("fake CI state exhausted")
        }

        fn build_brief(&self, _dir: &Path, state: &CiState) -> Result<String> {
            self.brief_heads
                .lock()
                .map_err(|_| anyhow::anyhow!("brief lock poisoned"))?
                .push(state.local_head_oid.clone());
            Ok(format!("# synthetic brief for {}\n", state.local_head_oid))
        }
    }

    fn repository(key: &str) -> Repository {
        Repository {
            key: key.to_owned(),
            name: "a-repository-name-that-is-far-too-long".to_owned(),
            root: "/repo".into(),
        }
    }

    fn state() -> CiState {
        CiState {
            number: 42,
            draft: false,
            pull_request_state: PullRequestState::Open,
            review: crate::github::ReviewStatus::NotReviewed,
            verdict: CiVerdict::Failed,
            checks: vec![Check {
                bucket: CheckBucket::Fail,
                name: "test".to_owned(),
                link: String::new(),
                workflow: String::new(),
            }],
            required_check_names: HashSet::from(["test".to_owned()]),
            local_branch: "main".to_owned(),
            local_head_oid: "abcdef1234567890".to_owned(),
            remote_head_oid: "abcdef1234567890".to_owned(),
        }
    }

    fn config() -> Config {
        Config {
            max_attempts: 3,
            fix_timeout: Duration::from_secs(30),
            watch_interval: Duration::from_secs(1),
        }
    }

    fn temporary_state(label: &str) -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "herdr-github-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn repo_and_journal(state_dir: &Path) -> (Repository, PathBuf) {
        let repo = Repository {
            key: "gh:example/service".to_owned(),
            name: "service".to_owned(),
            root: CHECKOUT.into(),
        };
        let path = journal_path(state_dir, &state_key(&repo), 42);
        (repo, path)
    }

    #[test]
    fn agent_names_are_valid_bounded_and_scoped() {
        let repo = repository("gh:example/repo");
        let name = agent_name(&repo, 1234, 1);
        assert!(name.starts_with("ghci-"));
        assert!(name.len() <= 32);
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        );
        assert_ne!(name, agent_name(&repo, 1235, 1));
        assert_ne!(name, agent_name(&repo, 1234, 2));
        assert_ne!(
            name,
            agent_name(&repository("gh:someone-else/repo"), 1234, 1)
        );
    }

    #[test]
    fn pending_journal_reservations_are_reused_and_committed() {
        let state_dir = temporary_state("journal");
        let (repo, path) = repo_and_journal(&state_dir);
        let pending = reserve_attempt(&path, &repo, 42, 2).unwrap();
        assert_eq!(reserve_attempt(&path, &repo, 42, 2).unwrap(), pending);
        assert_eq!(read_journal(&path).unwrap().pending, Some(pending.clone()));
        commit_attempt(&path, &pending).unwrap();
        assert_eq!(read_journal(&path).unwrap().committed, 1);
        let second = reserve_attempt(&path, &repo, 42, 2).unwrap();
        assert_eq!(second.attempt, 2);
        commit_attempt(&path, &second).unwrap();
        assert!(reserve_attempt(&path, &repo, 42, 2).is_err());
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn a_pending_journal_recovers_the_agent_started_before_a_crash() {
        let state_dir = temporary_state("journal-recovery");
        let (repo, path) = repo_and_journal(&state_dir);
        let pending = reserve_attempt(&path, &repo, 42, 3).unwrap();
        let agents = format!(
            r#""agents":[{{"pane_id":"w1:p2","workspace_id":"w1","tab_id":"w1:t2","agent_status":"working","name":"{}"}}]"#,
            pending.agent
        );
        let snapshot = SNAPSHOT.replace(r#""agents":[]"#, &agents);
        let server = Server::start(vec![Step::reply(snapshot.clone()), Step::reply(snapshot)]);
        let source = FakeCiSource::new(vec![state()]);

        let recovered = start(
            &server.client(),
            &source,
            Path::new(CHECKOUT),
            &state_dir,
            &state(),
            config(),
            LaunchMode::Detached,
        )
        .unwrap();

        assert!(!recovered.new);
        assert_eq!(recovered.agent, pending.agent);
        assert_eq!(recovered.attempt, 1);
        assert_eq!(
            read_journal(&path).unwrap(),
            AttemptJournal {
                committed: 1,
                pending: None,
            }
        );
        assert_eq!(server.requests().len(), 2);
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn every_mutable_pr_invariant_is_revalidated() {
        let initial = state();
        let mut variants = Vec::new();
        let mut changed = initial.clone();
        changed.number = 43;
        variants.push(changed);
        let mut changed = initial.clone();
        changed.pull_request_state = PullRequestState::Closed;
        variants.push(changed);
        let mut changed = initial.clone();
        changed.verdict = CiVerdict::Green;
        variants.push(changed);
        let mut changed = initial.clone();
        changed.remote_head_oid = "remote-advanced".to_owned();
        variants.push(changed);
        let mut changed = initial.clone();
        changed.local_head_oid = "local-advanced".to_owned();
        variants.push(changed);
        for changed in variants {
            assert!(validate_current(&initial, &changed).is_err());
        }
        assert!(validate_current(&initial, &initial).is_ok());
    }

    #[test]
    fn a_checkout_that_changes_while_waiting_is_not_modified() {
        let server = Server::start(vec![reply(SNAPSHOT), reply(CHANGED_REPOSITORY)]);
        let state_dir = temporary_state("changed-repository");
        let source = FakeCiSource::new(vec![state(), state()]);
        let error = start(
            &server.client(),
            &source,
            Path::new(CHECKOUT),
            &state_dir,
            &state(),
            config(),
            LaunchMode::Detached,
        )
        .unwrap_err();
        assert!(error.to_string().contains("changed"));
        assert!(source.brief_heads.lock().unwrap().is_empty());
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn stale_github_state_is_rejected_before_writing_or_launching() {
        let server = Server::start(vec![reply(SNAPSHOT), reply(SNAPSHOT)]);
        let state_dir = temporary_state("stale-state");
        let mut advanced = state();
        advanced.remote_head_oid = "advanced".to_owned();
        let source = FakeCiSource::new(vec![advanced]);
        let error = start(
            &server.client(),
            &source,
            Path::new(CHECKOUT),
            &state_dir,
            &state(),
            config(),
            LaunchMode::Detached,
        )
        .unwrap_err();
        assert!(error.to_string().contains("advanced"), "{error:#}");
        assert!(source.brief_heads.lock().unwrap().is_empty());
        assert_eq!(server.requests().len(), 2);
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn a_failed_launch_is_rolled_back_without_consuming_the_attempt() {
        let server = Server::start(vec![
            reply(SNAPSHOT),
            reply(SNAPSHOT),
            reply(TAB_CREATED),
            reply(REJECTED),
            reply(CLOSED),
        ]);
        let state_dir = temporary_state("failed-launch");
        let source = FakeCiSource::new(vec![state(), state()]);
        assert!(
            start(
                &server.client(),
                &source,
                Path::new(CHECKOUT),
                &state_dir,
                &state(),
                config(),
                LaunchMode::Detached,
            )
            .is_err()
        );
        let (_, path) = repo_and_journal(&state_dir);
        assert_eq!(read_journal(&path).unwrap(), AttemptJournal::default());
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn detached_launch_prompts_without_waiting_and_commits_same_brief_state() {
        let server = Server::start(vec![
            reply(SNAPSHOT),
            reply(SNAPSHOT),
            reply(TAB_CREATED),
            reply(AGENT_STARTED),
            reply(AGENT_PROMPTED),
            reply(NOTIFIED),
        ]);
        let state_dir = temporary_state("successful-launch");
        let source = FakeCiSource::new(vec![state(), state()]);
        let started = start(
            &server.client(),
            &source,
            Path::new(CHECKOUT),
            &state_dir,
            &state(),
            config(),
            LaunchMode::Detached,
        )
        .unwrap();
        assert!(started.new);
        assert_eq!(started.attempt, 1);
        assert_eq!(
            source.brief_heads.lock().unwrap().as_slice(),
            ["abcdef1234567890"]
        );
        let (_, path) = repo_and_journal(&state_dir);
        assert_eq!(read_journal(&path).unwrap().committed, 1);
        let requests = server.requests();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.method.as_str())
                .collect::<Vec<_>>(),
            [
                "session.snapshot",
                "session.snapshot",
                "tab.create",
                "agent.start",
                "agent.prompt",
                "notification.show"
            ]
        );
        assert!(
            requests
                .get(4)
                .and_then(|request| request.params.get("wait"))
                .is_none()
        );
        fs::remove_dir_all(state_dir).unwrap();
    }

    #[test]
    fn armed_launch_uses_one_atomic_prompt_and_wait_request() {
        let server = Server::start(vec![
            reply(SNAPSHOT),
            reply(SNAPSHOT),
            reply(TAB_CREATED),
            reply(AGENT_STARTED),
            reply(AGENT_PROMPTED),
            reply(NOTIFIED),
        ]);
        let state_dir = temporary_state("atomic-wait");
        let source = FakeCiSource::new(vec![state(), state()]);
        start(
            &server.client(),
            &source,
            Path::new(CHECKOUT),
            &state_dir,
            &state(),
            config(),
            LaunchMode::WaitForSettlement,
        )
        .unwrap();
        let requests = server.requests();
        let request = requests.get(4).unwrap();
        assert_eq!(request.method, "agent.prompt");
        assert!(request.params.get("wait").is_some());
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "agent.wait")
                .count(),
            0
        );
        fs::remove_dir_all(state_dir).unwrap();
    }
}
