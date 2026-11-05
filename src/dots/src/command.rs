//! The command tree is the source for parsing, help, and agent documentation.
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use github_inspect::{Repository, SnapshotRef};

#[derive(Debug, Parser)]
#[command(name = "dots", version, about = "Workflow tools for coding agents", long_about = "Workflow tools for coding agents across local tools, services, and host integrations.\nUse the available command groups for supported tasks; native tools remain available.\nHelp and skill generation work offline. Commands document their own effects and prerequisites.", disable_help_subcommand = true, term_width = 100, color = clap::ColorChoice::Never)]
pub(crate) struct Cli {
    /// Return JSON instead of readable output
    #[arg(long, global = true)]
    pub json: bool,
    /// Working directory for commands that use checkout context
    #[arg(long, global = true, value_name = "PATH")]
    pub directory: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Publish and inspect immutable project evidence; publication does not accept tasks
    Output {
        #[arg(long, global = true)]
        project: Option<String>,
        #[arg(long, global = true)]
        owner: Option<String>,
        #[command(subcommand)]
        command: OutputCommand,
    },
    /// Initialize the workspace catalog and explicitly bind local Git projects
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Inspect GitHub CI evidence and PR review threads, and keep a PR watch list
    Github {
        /// Explicit repository for numeric IDs or refs; URLs must agree with it
        #[arg(long, global = true, value_name = "[HOST/]OWNER/REPO")]
        repo: Option<Repository>,
        /// Bypass log caching and saved observations; fresh inspection always queries status
        #[arg(long, global = true)]
        no_cache: bool,
        #[command(subcommand)]
        command: GitHubCommand,
    },
    /// Track shared, claimable work for a project across concurrent sessions
    #[command(
        long_about = "Local work list shared by every session and worktree of one project: tasks, dependencies, claims, results, and issue/PR references. Git and GitHub remain the source of truth for code.\nThe project comes from the Git checkout at --directory, bound with dots project init; --project selects a registered project ID.\nA claim marks the actor responsible for a task. It lasts until that actor releases, hands off, or closes it; Pi releases only an ending runtime's claims and preserves them on reload. Workers report results to their parent session, which records them.\nA goal's tasks change only through its coordinator, the owner of its root claim (dots task goal coordinate); other sessions read them, and dots attention requests hold the work they name until incorporated.\nThe catalog is $XDG_DATA_HOME/dots/workspace.sqlite3 (default ~/.local/share/dots/workspace.sqlite3)."
    )]
    Task {
        /// Registered project ID instead of the current Git common-directory binding
        #[arg(long, global = true, value_name = "KEY")]
        project: Option<String>,
        /// Actor for claims and events; defaults to the invoking agent's live runtime (`DOTS_AGENT_OWNER` when it names the invoking session), else its session (pi-$PI_SESSION_ID, claude-$CLAUDE_CODE_SESSION_ID, or codex-$CODEX_SESSION_ID); supply explicitly for direct CLI use
        #[arg(long, global = true, value_name = "NAME")]
        owner: Option<String>,
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Ask the user durable questions about goal work and record goal decisions
    #[command(
        long_about = "Attention requests are durable questions a goal's coordinator raises when only the user can decide, authorize or act; decisions record what was decided, including the execution mandate. Both belong to the goal in the workspace catalog, not to a session, so they survive session ends, handoffs and history rewinds.\nA request moves open -> answered -> closed; its content never changes. An open or answered (not yet incorporated) request holds the tasks it names and their descendants on every claim and acceptance path. Answers come from any session; everything else belongs to the goal's coordinator (dots task goal coordinate). Incorporating an answer records a decision and closes the request in one transaction.\nContent files are JSON (or - for stdin) with unknown fields refused; see each command's help for its shape. Never put credentials in requests, answers or decisions. Provenance is cooperative bookkeeping that keeps the human's own words apart from an agent's interpretation; it is not authentication."
    )]
    Attention {
        /// Registered project ID instead of the current Git common-directory binding; list and decisions use --scope instead
        #[arg(long, global = true, value_name = "KEY")]
        project: Option<String>,
        /// Actor for claims and events; defaults to the invoking agent's live runtime (`DOTS_AGENT_OWNER` when it names the invoking session), else its session (pi-$PI_SESSION_ID, claude-$CLAUDE_CODE_SESSION_ID, or codex-$CODEX_SESSION_ID); supply explicitly for direct CLI use
        #[arg(long, global = true, value_name = "NAME")]
        owner: Option<String>,
        #[command(subcommand)]
        command: AttentionCommand,
    },
    /// Read, publish, and search scoped Markdown memory without a service
    #[command(
        long_about = "Portable Markdown memory outside the checkout. Reads never initialize storage. Writes require an explicit scope and expected revision, briefly lock that scope, and atomically replace one file. No automatic ingestion, installation, indexing, or network access. See the shared memory skill for evidence and maintenance policy."
    )]
    Memory {
        /// Registered project ID instead of resolving the current checkout
        #[arg(long, global = true, value_name = "ID")]
        project: Option<String>,
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Review a repository's changes in threads anchored to exact revisions, across its worktrees
    #[command(
        long_about = "Reviews of a repository's changes. A review pins its target to exact content: a base commit and a head tree, with uncommitted work captured as a tree, so threads refer to bytes reviewers can read with `git show HEAD_TREE:PATH`. Each revision also records a snapshot commit whose tree is its head, so viewers can diff base..snapshot. A thread is an agent finding or a comment: an optional anchor validated against the reviewed diff, optional finding fields, a status, and a conversation whose replies and status changes name their author. Refresh carries threads forward when the change moves. The author `user` is the human reviewing in the viewer: only the user drafts and submits comments, and only the user changes the status of a thread the user started. The user's drafts stay private until submitted; reviewed marks, the revision last looked at and presets serve the viewer. Reviews never post anywhere.\nEach worktree has one living review of its branch against its base, which open creates and refreshes; inbox summarises the worktrees of every known repository; start reviews other targets ad hoc. A review belongs to the worktree that holds it: its file lives in that worktree's own Git directory ($(git rev-parse --git-dir)/dots/reviews) and disappears with it. A shared ref (refs/dots/reviews/ID) keeps reviewed content safe from garbage collection and makes ids unique across the repository, so any worktree can show or change any review by id; without an id, commands use this worktree's most recently updated review. remove releases the ref, and the next start or remove releases refs whose review is gone from every worktree. Parallel writers to one review are safe."
    )]
    Review {
        #[command(subcommand)]
        command: ReviewCommand,
    },
    /// Export agent discovery documentation from this command tree
    Skills {
        #[command(subcommand)]
        command: SkillCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum OutputCommand {
    /// Publish a UTF-8 snapshot (at most 1 MiB); read the published revision before acceptance
    Publish {
        slug: String,
        #[arg(long)]
        filename: String,
        #[arg(long, value_parser=["text/markdown","text/plain","application/json"])]
        media_type: String,
        #[arg(long)]
        content_file: PathBuf,
        /// First publication; only an identical sole revision can retry this operation
        #[arg(long)]
        create: bool,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long, default_value = "")]
        provenance: String,
        /// Optional discovery association; does not record acceptance
        #[arg(long)]
        task: Option<String>,
    },
    /// List output slugs, optionally associated with one task; never select implicit latest evidence
    List {
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// List up to 20 exact revisions with metadata and pagination
    Revisions {
        slug: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Read and verify an exact revision; missing, corrupt and purged never fall back
    Show {
        slug: String,
        #[arg(long)]
        digest: String,
        #[arg(long, default_value_t = 1)]
        start: usize,
        #[arg(long, default_value_t=200, value_parser=clap::value_parser!(u16).range(1..=200))]
        lines: u16,
    },
    /// Show an exact revision's native path and availability without printing its content
    Path {
        slug: String,
        #[arg(long)]
        digest: String,
    },
    /// Associate output discovery with a task without accepting it
    Attach {
        slug: String,
        #[arg(long)]
        task: String,
    },
    /// Remove discovery only, preserving historical acceptance evidence and bytes
    Detach {
        slug: String,
        #[arg(long)]
        task: String,
    },
    /// Inspect payloads, orphan/staging files and catalog remnants without repairing them
    Integrity,
    /// Explicitly erase one project digest; tombstone before unlink, retry interrupted removal
    #[command(
        long_about = "Use only after an explicit user request to purge this project digest. Marks all matching revisions purged before removing matching managed output bytes, including orphan/staging copies. Sources, scratch, other projects, backups and session copies remain outside this guarantee. No automatic cleanup."
    )]
    Purge {
        #[arg(long)]
        digest: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommand {
    /// Initialize a catalog if absent, register a project, and bind this Git common directory
    Init {
        name: String,
        /// Start a fresh catalog while leaving any old-format data untouched
        #[arg(long)]
        fresh: bool,
    },
    /// Register an unbound project for explicit --project use
    Register { name: String },
    /// List registered projects and their exact common-directory bindings
    List,
    /// Show the binding and workspace directory for this checkout; never initialize storage
    Resolve,
    /// Bind this checkout to a registered project; refuse conflicting ownership
    Bind { id: String },
    /// Remove one exact binding, preserving the project and all its work
    Unbind {
        id: String,
        /// Exact recorded common directory from project list; may no longer exist
        #[arg(long)]
        path: PathBuf,
    },
    /// Move an exact recorded binding to this checkout's common directory
    Rebind {
        id: String,
        #[arg(long)]
        from: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum MemoryScope {
    Global,
    Project,
}

#[derive(Debug, Subcommand)]
pub(crate) enum MemoryCommand {
    /// Resolve global and current-project locations; creates nothing
    Scope,
    /// Read a bounded page and its full-file revision; absent files have revision "missing"
    Read {
        path: PathBuf,
        #[arg(long, value_enum)]
        scope: MemoryScope,
        #[arg(long, default_value_t = 1, value_parser = positive)]
        start: usize,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(1..=200))]
        lines: u16,
    },
    /// Publish one Markdown file from UTF-8 input; rejects stale revisions and symlink paths
    #[command(
        long_about = "Publish one file using --expected from memory read (or 'missing' for a new file). Reads at most 256 KiB from --content-file. Creates scope/topic directories on first write. A kernel-held dots/mutation.lock covers revision checking and atomic replacement across scopes; contention returns busy without waiting; conflicts change no file contents. Multi-file updates are NOT transactions: write topics before the index, and handle interrupted updates explicitly. No shell commands or model calls run under the lock."
    )]
    Write {
        path: PathBuf,
        #[arg(long, value_enum)]
        scope: MemoryScope,
        #[arg(long)]
        expected: String,
        #[arg(long)]
        content_file: PathBuf,
    },
    /// Remove one file with revision checking; does not erase backups or external indexes
    Remove {
        path: PathBuf,
        #[arg(long, value_enum)]
        scope: MemoryScope,
        #[arg(long)]
        expected: String,
    },
    /// Case-insensitive literal search of current knowledge; defaults to global plus this project
    #[command(
        long_about = "Search live Markdown files, not a cached index. Default scope is global plus the current project, never unrelated projects. The knowledge change log is excluded. Retained outputs and scratch are outside the searched knowledge roots. Results are deterministic path/line matches, not semantic ranking; read the source before applying a snippet. Output reports backend, scope, and pagination. QMD is not installed or invoked."
    )]
    Search {
        query: String,
        #[arg(long, value_enum)]
        scope: Option<MemoryScope>,
        /// Skip this many matches in each selected scope
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=100000))]
        offset: u32,
        /// Maximum matches per scope
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..=20))]
        limit: u16,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskCommand {
    /// Create, list, and inspect goals: root tasks with a Markdown brief
    #[command(
        long_about = "A goal is a root task with a Markdown brief in the project workspace's briefs/ directory; dots project resolve prints the workspace path. Create briefs/ there if it does not exist yet. The root task ID is the goal ID; its tasks, claims, and results stay in the task list."
    )]
    Goal {
        #[command(subcommand)]
        command: GoalCommand,
    },
    #[command(flatten)]
    Query(TaskQuery),
    #[command(flatten)]
    Mutation(TaskMutation),
}

#[derive(Debug, Subcommand)]
pub(crate) enum GoalCommand {
    /// Create a root task with an existing brief
    Create {
        title: String,
        /// Existing Markdown file, as briefs/<name>.md relative to the project workspace (dots project resolve prints its path)
        #[arg(long, value_name = "PATH")]
        brief: PathBuf,
    },
    /// Attach or replace a root task's brief
    Attach {
        id: String,
        /// Existing Markdown file, as briefs/<name>.md relative to the project workspace (dots project resolve prints its path)
        #[arg(long, value_name = "PATH")]
        brief: PathBuf,
    },
    /// List unfinished goals with progress, most recently active first
    List {
        /// Include done and dropped goals
        #[arg(long)]
        all: bool,
        /// Only goals where NAME claims the root or a descendant
        #[arg(long, value_name = "NAME")]
        claimed_by: Option<String>,
    },
    /// Show a goal's brief, progress, and unfinished tasks
    Show {
        id: String,
        /// Include done and dropped tasks
        #[arg(long)]
        all: bool,
    },
    /// Remove the brief association; refused once the goal has requests or decisions
    Detach { id: String },
    /// Take coordination of a goal: claim its root although work remains below it
    #[command(
        long_about = "Claim a goal's root as its coordinator. Only the coordinator dispatches, edits, accepts or closes the goal's tasks and raises or incorporates its attention requests; other sessions read, and anyone can answer a request. Coordination is responsibility, not dispatch: it works with unfinished children, prerequisites and held work. A managed worker (DOTS_WORKER_ID set) cannot coordinate, and a Pi session needs its live runtime owner unless --owner is given. Another owner's claim on the root or any descendant fails unless --take, which moves every such claim to this owner in one transaction and records each transfer. A held (paused) or closed root needs --resume, which reopens it. Coordinating again is a no-op. Stop coordinating with dots task release ID; pause with dots task hold ID, which holds every descendant."
    )]
    Coordinate {
        id: String,
        /// Take over claims held by other owners on the root and its descendants
        #[arg(long)]
        take: bool,
        /// Reopen a held or closed goal root before coordinating it
        #[arg(long)]
        resume: bool,
    },
    /// List the goals the acting owner coordinates, across every registered project
    #[command(
        long_about = "List exact PROJECT and GOAL identities for every open or in-review goal whose root the acting owner (--owner, else the invoking agent's runtime owner or session) claims, across every registered project. Claims below a root, held or closed roots, and released or taken-over roots are not coordination. A claim records responsibility, not that its session is running. Reads only the catalog; it never initializes one, and --project does not apply."
    )]
    Coordinating,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskQuery {
    /// List unfinished root tasks; use --parent ID to drill down or --flat for every level
    #[command(
        long_about = "Unfinished root tasks by default, oldest first, with child counts. Use --parent ID for descendants or --flat for every level; --claimed-by searches every level unless --roots is given. --all includes closed tasks."
    )]
    List {
        /// Only tasks anywhere below this one
        #[arg(long, value_name = "ID", conflicts_with = "roots")]
        parent: Option<String>,
        /// Only tasks claimed by NAME
        #[arg(long, value_name = "NAME")]
        claimed_by: Option<String>,
        /// Only top-level tasks (the default unless --parent, --flat, or --claimed-by is given)
        #[arg(long, conflicts_with = "flat")]
        roots: bool,
        /// Every task level, rather than the default root overview
        #[arg(long, conflicts_with = "parent")]
        flat: bool,
        /// Exactly one status: open, review, held, done or dropped
        #[arg(long, conflicts_with = "all", value_parser = ["open", "review", "held", "done", "dropped"])]
        status: Option<String>,
        /// Include done and dropped tasks
        #[arg(long)]
        all: bool,
    },
    /// Unclaimed, unblocked open tasks without children
    Ready {
        /// Only tasks anywhere below this one
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
    },
    /// Show one task with its result, evidence, and recent events
    Show { id: String },
    /// Show an earlier result and its evidence
    ResultEvent {
        id: String,
        #[arg(long)]
        event: i64,
    },
    /// Project history after an event seq, oldest first; use the last seen seq as a cursor
    #[command(
        long_about = "Project history after an event seq, oldest first; use the last seen seq as a cursor. dots task event-kinds lists the kinds an event can have.\nWith --json the page also carries head, the project's newest seq (0 before its first event), even with --under; --limit 0 returns only the head, a starting cursor for reading what follows.\n--under ID keeps only events on that task and its descendants, judged by the task tree at each read: tasks created in or moved into it count from then on, and a task moved out stops counting. An ID outside the project is an error.\n--follow keeps running instead of returning a page: it prints every event after --since, then each new event as any session commits it, within about a second, one per line with no gaps or repeats. With --json each line is one event object, shaped like an item of the events array (NDJSON). It exits 0 when its output closes or on SIGTERM, SIGINT, or SIGHUP. A storage failure ends it nonzero with the error on stderr; with --json it also prints one {\"error\": ...} line. To resume after a restart, pass the seq of the last event received as --since."
    )]
    Events {
        /// Only events with seq greater than this (default 0)
        #[arg(long, default_value_t = 0)]
        since: i64,
        /// Maximum events to return (0 for only the head); --follow streams every event instead
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(0..=1000), conflicts_with = "follow")]
        limit: u16,
        /// Only events on this task and its descendants
        #[arg(long, value_name = "ID")]
        under: Option<String>,
        /// Keep printing events as they commit, one per line, until stopped
        #[arg(long)]
        follow: bool,
    },
    /// List every kind an event can have
    EventKinds,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskMutation {
    /// Create an open task; --parent groups it under a larger task or goal
    Add {
        title: String,
        /// What the task is for and how to tell it is done
        #[arg(long)]
        body: Option<String>,
        /// Parent task ID
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        /// Prerequisite task IDs; repeatable
        #[arg(long, value_name = "ID")]
        blocked_by: Vec<String>,
    },
    /// Claim a task, or the next ready one, for this session
    #[command(
        long_about = "Claim a task for this session. Fails if the task is blocked, has unfinished children, is neither open nor in review, or is claimed by another session; --take takes over a claim whose session has stopped working on it. Claiming your own task again changes nothing. Review tasks need an explicit ID. --next claims the oldest ready task without children, optionally under --parent, and exits 1 when nothing is ready."
    )]
    Claim {
        /// Task ID; omit with --next
        #[arg(required_unless_present = "next", conflicts_with = "next")]
        id: Option<String>,
        /// Claim the oldest ready task instead of a specific ID
        #[arg(long)]
        next: bool,
        /// With --next, only consider tasks anywhere below this one
        #[arg(long, value_name = "ID", requires = "next")]
        parent: Option<String>,
        /// Take over another session's claim
        #[arg(long, conflicts_with = "next")]
        take: bool,
    },
    /// Release this session's claim on a task, or every claim it holds with --all
    Release {
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        id: Option<String>,
        /// Release every claim this owner holds, in every project
        #[arg(long)]
        all: bool,
    },
    /// Edit a task's fields, or replace its current result
    #[command(
        long_about = "Edit the title, body, branch, or parent, or replace the current result with --result; earlier results stay in history. A task claimed by another session needs --force."
    )]
    Update {
        id: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New description in Markdown: scope, acceptance criteria, and pointers; replaces the old one
        #[arg(long)]
        body: Option<String>,
        /// Current result in Markdown: what changed, where it is, checks run, and what remains
        #[arg(long, conflicts_with_all = ["title", "body", "branch", "parent"])]
        result: Option<String>,
        /// Published output revision the result cites; repeatable
        #[arg(long, num_args = 2, value_names = ["SLUG", "DIGEST"], requires = "result")]
        evidence: Vec<String>,
        /// Branch carrying the work, so another session can find it
        #[arg(long)]
        branch: Option<String>,
        /// New parent task ID; an empty string detaches
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        /// Edit a task claimed by another session
        #[arg(long)]
        force: bool,
    },
    /// Record a result and move the task to review; clears the claim
    Review {
        id: String,
        /// What changed, where it is, checks run, and what the reviewer should look at
        #[arg(long)]
        result: String,
        /// Published output revision the result cites; repeatable
        #[arg(long, num_args = 2, value_names = ["SLUG", "DIGEST"])]
        evidence: Vec<String>,
        /// Ignore blockers, unfinished children, and another session's claim
        #[arg(long)]
        force: bool,
    },
    /// Close a task whose acceptance criteria are met
    Done {
        id: String,
        /// The outcome and how it was verified
        #[arg(long)]
        result: String,
        /// Published output revision the result cites; repeatable
        #[arg(long, num_args = 2, value_names = ["SLUG", "DIGEST"])]
        evidence: Vec<String>,
        /// Ignore blockers, unfinished children, and another session's claim
        #[arg(long)]
        force: bool,
    },
    /// Pause a task with a reason and clear its claim; its descendants stay claimable
    Hold {
        id: String,
        #[arg(long)]
        reason: String,
        /// Pause a task claimed by another session
        #[arg(long)]
        force: bool,
    },
    /// Close a task that will not be done; its dependents are unblocked
    Drop {
        id: String,
        #[arg(long)]
        reason: String,
        /// Ignore blockers, unfinished children, and another session's claim
        #[arg(long)]
        force: bool,
    },
    /// Return a task to open; its result stays
    Reopen {
        id: String,
        #[arg(long)]
        reason: Option<String>,
        /// Reopen a task claimed by another session
        #[arg(long)]
        force: bool,
    },
    /// Link issues, pull requests, and tickets to a task; nothing is sent to their provider
    Ref {
        #[command(subcommand)]
        command: TaskReferenceCommand,
    },
    /// Manage prerequisites between tasks
    Dep {
        #[command(subcommand)]
        command: DepCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskReferenceCommand {
    /// Link an issue, pull request, or ticket by URL; setting it again replaces the label
    Set {
        id: String,
        /// Full URL, such as a GitHub pull request or a Linear issue
        url: String,
        /// Short name to show instead of the URL, such as ENG-123
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Remove only this task's association, leaving the external item unchanged
    Remove {
        id: String,
        url: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum DepCommand {
    /// Make ID wait for --blocked-by; duplicates, cycles and parent/child pairs are rejected
    Add {
        id: String,
        #[arg(long, value_name = "ID")]
        blocked_by: String,
    },
    /// Remove a prerequisite
    Remove {
        id: String,
        #[arg(long, value_name = "ID")]
        blocked_by: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GitHubCommand {
    /// Inspect PR, ref, commit, or workflow-run CI and retrieve failure evidence
    #[command(
        long_about = "Inspect CI for a PR, remote ref, commit, or exact workflow run/attempt.\nRead-only; uses existing gh authentication.\nWith no subcommand, inspect the current branch's PR. Use --pr URL for another PR, --ref main or --sha SHA with --repo, or --run RUN_URL [--attempt N].\nA ref selects its current commit, not the latest historical/scheduled run. Use gh run list for run discovery; pass a run URL here for diagnosis.\nDetails default to failed, cancelled, unknown and pending checks. --all includes passing/skipped checks; --name TEXT searches check names across all states. Both remain paged; summary always covers every check.\nFailed or pending CI is data, not a command error. No checks does not mean CI passed.\nFollow returned commands rather than constructing job references or selecting the latest run."
    )]
    Ci(Box<CiArgs>),
    /// Inspect unresolved PR review threads and read exact conversations
    #[command(
        long_about = "Read-only review-thread investigation. Defaults to unresolved threads, including outdated feedback. Opening-comment previews are not full conversations. Follow the returned thread command for replies and reviewed diff context. Use --json for agent workflows. General PR discussion, review submission, replies, resolution, and merging remain with gh or GitHub. Fresh lists traverse at most 1000 threads; conversations at most 200 comments. Query errors and incomplete traversals fail explicitly. Saved observations expire after five minutes; paging requires local credential/keychain access but makes no GitHub requests. Refresh preserves the filter and page size and restarts at offset zero. JSON has_more indicates continuation; complete means this page contains the entire matching collection."
    )]
    Reviews(Box<GitHubReviewArgs>),
    /// Keep an agent-curated list of PRs with cached status
    #[command(
        long_about = "Agent-curated list of pull requests worth following, with cached status for pickers and agents. Add PRs you create or work on; adding records intent only and authorizes no CI fixes, monitoring, or merging. Entries are identified by canonical github.com PR URL.\nNo daemon: each row stores its next due check (60s before the first check or while CI is pending, 10 minutes once settled, 5 minutes after an error; merged or closed PRs are never checked again). Refresh fetches only due rows, one GraphQL request per 50 PRs, using existing gh authentication; concurrent refreshers never fetch the same PR twice. A failed check keeps the previous status and records the error. While a picker is open, its follow command refreshes due PRs; nothing runs otherwise.\nThe store is $XDG_STATE_HOME/dots/github/watch.sqlite3 (default ~/.local/state/dots/github/watch.sqlite3)."
    )]
    Watch {
        #[command(subcommand)]
        command: WatchCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum WatchCommand {
    /// Add a PR, or update its checkout and sessions; idempotent, then refreshes that PR
    #[command(
        long_about = "Add a PR by URL, or update its entry: the original added time is kept, the checkout, agent session, and Rex session become the latest non-empty values, and the PR becomes due. The agent session is the invoking agent's: pi-$PI_SESSION_ID, claude-$CLAUDE_CODE_SESSION_ID, or codex-$CODEX_SESSION_ID. The Rex session is the one the work belongs to: $REX_SESSION in a Rex terminal, otherwise $DOTS_REX_SESSION, which Pi workers inherit from the session that started them. Then refreshes this PR only; a GitHub failure is a warning in the result and never fails the add."
    )]
    Add {
        /// github.com pull request URL
        #[arg(value_parser = pull_request_url)]
        url: github_watch::PullRequestUrl,
        /// Checkout the PR is worked from, stored as an absolute path; defaults to --directory or the current directory (a linked worktree itself, not its main checkout)
        #[arg(long, value_name = "PATH")]
        checkout: Option<PathBuf>,
    },
    /// Remove a PR from the list; an unlisted PR is not an error
    Remove {
        /// github.com pull request URL
        #[arg(value_parser = pull_request_url)]
        url: github_watch::PullRequestUrl,
    },
    /// List watched PRs with cached status, newest first; no network
    #[command(
        long_about = "List watched PRs with their cached status, most recently added first, and how many are due for a check. Makes no network requests; run refresh to update. Drops merged or closed PRs a day after their closure was observed."
    )]
    List,
    /// Show one watched PR's cached status and description; no network
    #[command(
        long_about = "Show one watched PR for a preview pane: identity and title, state, review and CI, branch, checkout, Rex session, who added it and when, when it was last checked, the last check error, then the PR description cached by the last successful check. --color adds ANSI styling; COLUMNS wraps visible text to the given width. Makes no network requests. A PR that is not on the watch list is an error. --json returns the entry with its description."
    )]
    Show {
        /// github.com pull request URL
        #[arg(value_parser = pull_request_url)]
        url: github_watch::PullRequestUrl,
        /// Style the text with basic ANSI colors; ignored by --json
        #[arg(long)]
        color: bool,
    },
    /// Refresh due PRs while a picker reads the output; prints a line when rows change
    #[command(
        long_about = "Meant as a picker's watch command. Prints `ready` at once, then refreshes PRs as they become due, exactly like refresh, and prints `changed` whenever the watched rows differ from what it last saw: a status change, a recorded error, or an entry another session added or removed. It has no effect beyond refreshing due PRs. Sleeps until the earliest due PR (between 1 and 60 seconds, re-checking at least every minute for new entries). Runs only while its reader is alive: it exits 0 when its output is closed or on SIGTERM, SIGINT, or SIGHUP. GitHub failures are recorded on their PRs and never end it; a storage failure does."
    )]
    Follow,
    /// Fetch status for PRs that are due; --all makes every open PR due first
    Refresh {
        /// Check every PR that is not merged or closed, due or not
        #[arg(long)]
        all: bool,
    },
}

fn pull_request_url(value: &str) -> Result<github_watch::PullRequestUrl, String> {
    github_watch::PullRequestUrl::parse(value).map_err(|error| error.to_string())
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct CiArgs {
    #[command(flatten)]
    pub inspect: InspectArgs,
    #[command(subcommand)]
    pub command: Option<CiCommand>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CiCommand {
    /// Inspect the selected CI target, with bounded evidence and complete scope counts
    Inspect(InspectArgs),
    /// Read exact job logs from a URL, ID, or inspection reference
    #[command(
        long_about = "Read exact job logs from a URL, ID, or inspection reference.\nCopy REF from inspection output. PR updates and reruns do not redirect it. Job URLs and numeric IDs with --repo also work; no PR is required.\nUse --search TEXT for a compact match index; follow Context for a compact window with up to three preceding lines.\nFor Go failures, follow Test output or use --test NAME to join RUN/NAME/CONT regions. Test output starts near a diagnostic when available; --start 1 reads from the beginning. Attribution is heuristic when concurrent packages interleave.\nUse --start to page lines, resume a search, or continue attributed test output.\nSearch reports total and cursor-eligible matches, up to ten matching lines with context commands, and a command for the next ten matches. Follow saved-observation commands to search/page without GitHub queries. Saved job observations expire after five minutes; omit --snapshot to refresh. Downloads are cached for 24 hours, bounded at 512 MiB; excerpts are at most 16 KiB.\nMissing logs include a gh fallback; do not infer a cause from status alone.\nAn aggregate job may only report a dependency failure: inspect the other failed jobs for the cause."
    )]
    Logs(LogArgs),
    /// Read GitHub-hosted check output; works for Actions and external-provider checks
    #[command(
        long_about = "Read bounded GitHub check-run title, summary and text. Default windows use conservative compiler/failure markers; this is navigation, not a root-cause verdict. Use --start 1 for the beginning. Search context defaults to 20 lines with up to three preceding lines. Follow saved-observation commands to search/page without additional GitHub calls. Snapshots expire after five minutes; omit --snapshot to refresh. --annotations reads a fresh, bounded page of source diagnostics instead of text. Provider links and gh fallbacks remain available."
    )]
    Output(OutputArgs),
}

#[derive(Debug, Args)]
pub(crate) struct InspectArgs {
    /// Remote branch/tag, resolved once to a commit; use refs/heads/NAME to disambiguate
    #[arg(long="ref", conflicts_with_all=["pr","sha","run","snapshot"])]
    pub git_ref: Option<String>,
    /// Full commit SHA; inspects checks/statuses on that revision, not branch history
    #[arg(long, conflicts_with_all=["pr","git_ref","run","snapshot"])]
    pub sha: Option<String>,
    /// Workflow run URL or numeric ID; covers all event types without a PR
    #[arg(long, conflicts_with_all=["pr","git_ref","sha","snapshot"])]
    pub run: Option<String>,
    /// Exact run attempt; default resolves and pins the current attempt
    #[arg(long, requires="run", value_parser=clap::value_parser!(u64).range(1..))]
    pub attempt: Option<u64>,
    /// Explicit PR URL or number; URLs work outside a checkout
    #[arg(long, value_name = "URL_OR_NUMBER")]
    pub pr: Option<String>,
    /// Read a saved observation (five-minute lifetime) without querying GitHub status
    #[arg(long, conflicts_with_all = ["pr", "no_cache", "repo", "git_ref", "sha", "run"], value_name = "REF")]
    pub snapshot: Option<SnapshotRef>,
    /// Skip automatic evidence retrieval (job logs and check output)
    #[arg(long)]
    pub no_logs: bool,
    /// Maximum check details to show; summary always covers all checks
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=1000))]
    pub limit: u16,
    /// Skip this many checks in failure-first order
    #[arg(long, default_value_t = 0)]
    pub offset: usize,
    /// Include passed/skipped checks; still bounded by --limit and --offset
    #[arg(long)]
    pub all: bool,
    /// Case-insensitive literal substring in check names; includes all states
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct OutputArgs {
    /// Check-run URL, numeric ID with --repo, or reference returned by inspection
    pub reference: String,
    /// Read the saved text observation without GitHub queries (five-minute lifetime)
    #[arg(long, conflicts_with_all = ["no_cache", "annotations"])]
    pub snapshot: Option<SnapshotRef>,
    /// First line (one-based), or first eligible search match
    #[arg(long, value_parser = positive, conflicts_with = "annotations")]
    pub start: Option<usize>,
    /// Maximum lines per window (default 40; search context 20)
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=500))]
    pub lines: Option<u16>,
    /// Literal, case-sensitive search; returns at most ten matches and context commands
    #[arg(long, allow_hyphen_values = true, conflicts_with = "annotations")]
    pub search: Option<String>,
    /// Read at most ten source annotations, with bounded messages
    #[arg(long)]
    pub annotations: bool,
    /// Annotation page to read; follow the returned command to continue
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..=100000), requires = "annotations")]
    pub annotation_page: u32,
}

#[derive(Debug, Args)]
pub(crate) struct LogArgs {
    /// Reuse saved job metadata and logs without GitHub queries; omit to refresh
    #[arg(long)]
    pub snapshot: Option<SnapshotRef>,
    /// Job URL, numeric ID with --repo, or reference returned by inspection
    pub reference: String,
    /// Find a literal, case-sensitive string; return a compact match index without context
    #[arg(long, allow_hyphen_values = true)]
    pub search: Option<String>,
    /// Read output regions for an exact Go test name; preserves original line numbers
    #[arg(long, conflicts_with = "search")]
    pub test: Option<String>,
    /// First line (one-based), or first eligible search match; default centers near a failure marker
    #[arg(long, value_parser = positive)]
    pub start: Option<usize>,
    /// Maximum lines per window/test (default 100; search context 20)
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=500))]
    pub lines: Option<u16>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SkillCommand {
    /// Print the generated skill or its command reference to stdout
    Render {
        /// Print the detailed help reference instead of SKILL.md
        #[arg(long)]
        reference: bool,
    },
    /// Write SKILL.md and its command reference to an explicit directory
    Generate {
        #[arg(long, value_name = "DIRECTORY")]
        output: PathBuf,
    },
    /// Verify generated skill files without changing them
    Check {
        #[arg(long, value_name = "DIRECTORY")]
        output: PathBuf,
    },
}

fn positive(value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or_else(|| "line must be a positive integer".into())
}

#[derive(Debug, Subcommand)]
pub(crate) enum ReviewCommand {
    /// Start a review of one target and print its revision, changed files, and diff command
    #[command(
        long_about = "Start a review of another target than the worktree's own review (see open), such as an ad hoc pass by agents. Choose exactly one target: --uncommitted reviews uncommitted changes (tracked and untracked, not ignored) against HEAD; --base REF reviews everything since the merge base of REF and HEAD, including uncommitted work. Refuses an empty change. Reviewers read the patch with the returned diff_command and file content with `git show HEAD:PATH` using the review's head tree, not the live working tree."
    )]
    Start {
        #[command(flatten)]
        target: ReviewTarget,
        /// Name of the profile running the review, recorded as given
        #[arg(long, value_name = "NAME")]
        profile: Option<String>,
        /// Session told when the user submits comments; defaults to the invoking agent session (pi-$PI_SESSION_ID, claude-$CLAUDE_CODE_SESSION_ID, or codex-$CODEX_SESSION_ID), otherwise none
        #[arg(long, value_name = "NAME")]
        owner: Option<String>,
    },
    /// Open this worktree's own review, creating or refreshing it, and show it
    #[command(
        long_about = "Open the worktree's own review: its branch through the working tree against its base. There is one per worktree; open creates it on first use and otherwise refreshes it to the working tree as it is now (new commits and edits; threads carry forward as for refresh), then prints what show prints plus an opened record: whether it was created, whether it moved, the threads carried and outdated, and the delta since the previous revision. Run it in the worktree, or name one with --directory. The base is, in order: a base set with --base, kept for later opens and refreshes until --default-base; the branch's upstream when it names another branch (a stacked branch), never the branch's own name on a remote; HEAD itself on the default branch, so only uncommitted changes; otherwise the nearest default-branch candidate among each remote's default branch (refs/remotes/REMOTE/HEAD) and the local main, else master: the one HEAD has the fewest commits beyond, ties preferring upstream, then origin, then other remotes, then the local branch. Each revision's basis records the reference, the reason, and every candidate's distance. An --owner, or the invoking agent session, becomes the review's owner; without one the owner stays. Refuses to create a review of an empty change."
    )]
    Open {
        /// Compare against the merge base with REF from now on
        #[arg(long, value_name = "REF", conflicts_with = "default_base")]
        base: Option<String>,
        /// Drop a base set earlier and use the default rule
        #[arg(long)]
        default_base: bool,
        /// Session told when the user submits comments; defaults to the invoking agent session, otherwise the owner stays
        #[arg(long, value_name = "NAME")]
        owner: Option<String>,
        /// Include the user's drafts
        #[arg(long)]
        drafts: bool,
        /// Add a view, as for show
        #[arg(long, value_parser = ["everything", "uncommitted", "committed"])]
        preset: Option<String>,
    },
    /// Summarise the worktrees of every known repository: changes and what their reviews wait on the user for
    #[command(
        long_about = "Summarise worktrees across repositories without creating or refreshing a review or writing a ref; like git status, Git may refresh each index's cached file stats. Repositories: the common Git directories bound in the dots project catalog, the current repository, and each --repo. Per worktree: path, branch, HEAD, the base its review would use now with its basis, the working tree's change against it (files, added and removed lines, untracked files included), how many other reviews it holds, and its own review if it has one: id, owner, revision, threads waiting on the user, the user's drafts, and the paths whose change differs from the revision the user last looked at (null before the first look). A thread waits on the user when it is open, accepted or outdated and its latest comment, ignoring refresh's, is not the user's: an agent's finding, or the user's thread an agent answered. Most in need of attention first: waiting threads, drafts, changed paths, having a review, change size. Worktrees with no change and no review are left out unless --all. Repositories and worktrees that cannot be read are listed as skipped with the reason."
    )]
    Inbox {
        /// Another repository, by any directory in it
        #[arg(long = "repo", value_name = "PATH")]
        repos: Vec<PathBuf>,
        /// Include worktrees with no change and no review
        #[arg(long)]
        all: bool,
    },
    /// List this worktree's reviews, or every worktree's with --all
    #[command(
        long_about = "List reviews, oldest first: this worktree's, or with --all those of every worktree of the repository. Each entry carries its owner (the session that started it, or null), worktree (path and branch at start), review file, current revision and thread counts by status; with --drafts, the number of the user's drafts."
    )]
    List {
        /// Reviews of every worktree of the repository
        #[arg(long)]
        all: bool,
        /// Only reviews owned by this session, such as pi-<id>
        #[arg(long, value_name = "NAME")]
        owner: Option<String>,
        /// Count the user's drafts
        #[arg(long)]
        drafts: bool,
    },
    /// Show a review with its worktree, revisions, threads, changed files, and diff command
    #[command(
        long_about = "Show a review: its file, worktree, revisions (each with base, head tree and snapshot, the commit whose tree is that head, so base..snapshot is the reviewed diff), threads with their conversations, the user's submissions, reviewed marks and last-looked revision, the current revision's changed files (each with status, line counts, whether the user marked that exact change reviewed, and whether it changed since the revision the user last looked at), and the command that reads its patch. Patches take attributes from the reviewed tree, never from the calling worktree, so they read the same everywhere. The user's drafts are private until submitted: --drafts includes them. --preset adds a view for the viewer: the commits its sides show, the paths it diffs, the presets with changes at this revision, and where each anchor of the current revision falls in its line numbers (null where it does not show). ID defaults to this worktree's most recently updated review."
    )]
    Show {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Include the user's drafts
        #[arg(long)]
        drafts: bool,
        /// Add a view: everything (base..head), uncommitted (HEAD commit..head) or committed (base..HEAD commit)
        #[arg(long, value_parser = ["everything", "uncommitted", "committed"])]
        preset: Option<String>,
    },
    /// Open threads from a JSON file at the current revision; all or none
    #[command(
        long_about = "Open threads at the review's current revision. The file (or - for stdin) holds {\"threads\": [...]}, at most 200 per batch. Each thread: body (the opening comment; Markdown, at most 16 KiB), author (who wrote it: a model id, an agent, user), optional anchor: {path, side: new|old, start, end} whose lines fall within one hunk of the reviewed diff, or {path} alone for the whole of a changed file (omit the anchor for a review-level thread), and, for findings, optional title (one line, at most 200 characters), severity (critical, important, minor or nit), pre_existing (bool), confidence (0 to 1), angle (lowercase name like correctness) and rule {path, start, end} citing instruction-file lines in the reviewed head. Unknown fields are refused. Any invalid thread rejects the whole batch, listing every error by index. Pass --revision with the index of the revision the anchors were read against (its position in show's revisions, from zero); if the review has moved to another revision since, the whole batch is refused. ID defaults to this worktree's most recently updated review.",
        allow_missing_positional = true
    )]
    Add {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// JSON file with the threads, or - for stdin
        #[arg(long, value_name = "PATH")]
        threads: PathBuf,
        /// Revision index the anchors were read against; refuse the batch if the review has moved
        #[arg(long, value_name = "N")]
        revision: Option<usize>,
    },
    /// Reply to a thread without changing its status
    #[command(allow_missing_positional = true)]
    Reply {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Thread id, like t1
        thread: String,
        /// Who is replying: a model id, an agent, user
        #[arg(long, value_name = "NAME")]
        author: String,
        #[command(flatten)]
        body: ReviewBody,
    },
    /// Change a thread's status: open, accepted, dismissed, fixed or posted
    #[command(
        long_about = "Change a thread's status, recorded in its conversation as a comment naming the author and new status, with an optional body giving the evidence. Outdated is set only by refresh. Only the user (author user) changes the status of a thread the user started; others reply instead. A thread left at an earlier revision cannot be reopened (open or accepted); open a new thread instead. ID defaults to this worktree's most recently updated review.",
        allow_missing_positional = true
    )]
    Set {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Thread id, like t1
        thread: String,
        #[arg(long, value_parser = ["open", "accepted", "dismissed", "fixed", "posted"])]
        status: String,
        /// Who decides: a model id, an agent, user
        #[arg(long, value_name = "NAME")]
        author: String,
        /// Why, for the record
        #[arg(long, value_name = "TEXT")]
        body: Option<String>,
    },
    /// Record a draft of the user's: a new thread, or a reply with --thread
    #[command(
        long_about = "Record one of the user's comments as a draft at the current revision: a reply to a submitted thread with --thread, or a new thread with an optional anchor: --path alone for the whole file, with --side, --start and --end for lines. Anchor lines count lines of --preset's view (default everything) and must map to lines of the reviewed diff; lines that differ between that view's side and the review's are refused. Drafts belong to the user, are hidden from show and list unless asked for, and are published only by submit. --revision refuses a review that has moved, as for add. ID defaults to this worktree's most recently updated review.",
        allow_missing_positional = true
    )]
    Draft(ReviewDraft),
    /// Drop one of the user's drafts, or all of them
    #[command(allow_missing_positional = true)]
    Discard {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        #[command(flatten)]
        which: DiscardWhich,
    },
    /// Publish every draft of the user's at once, with an optional summary
    #[command(
        long_about = "Publish every draft at once, in order: new threads open as the user's, replies join their threads. Records the submission (time, summary, and each published comment), which the owning session is told about. All or none: a new thread whose lines changed since it was drafted refuses the whole submission, listing every such draft; discard it and comment again. A summary alone is a submission too. ID defaults to this worktree's most recently updated review.",
        allow_missing_positional = true
    )]
    Submit {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Summary (Markdown)
        #[arg(long, value_name = "TEXT", conflicts_with = "body_file")]
        body: Option<String>,
        /// File holding the summary, or - for stdin
        #[arg(long, value_name = "PATH")]
        body_file: Option<PathBuf>,
    },
    /// Mark a changed file reviewed until its change differs
    #[command(
        long_about = "Mark a changed file of the current revision reviewed; with --revision, refuse if the review has moved past the revision the user saw. The mark records the file's change (modes and object ids on both sides), so it holds exactly while the file's change is that change: it lapses by itself when the file changes at a later revision, and holds again if the change returns. ID defaults to this worktree's most recently updated review.",
        allow_missing_positional = true
    )]
    Mark {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Changed path
        path: String,
        /// Revision index the user saw; refuse if the review has moved
        #[arg(long, value_name = "N")]
        revision: Option<usize>,
    },
    /// Clear a file's reviewed mark
    #[command(allow_missing_positional = true)]
    Unmark {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Changed path
        path: String,
        /// Revision index the user saw; refuse if the review has moved
        #[arg(long, value_name = "N")]
        revision: Option<usize>,
    },
    /// Record the revision the user looked at
    #[command(
        long_about = "Record the revision the user looked at (--revision, default the current one) as the one last looked at; show then reports, per changed file, whether its change differs from that revision's. A look never moves back to an earlier revision than one already recorded. ID defaults to this worktree's most recently updated review."
    )]
    Look {
        /// Review id; defaults to this worktree's most recently updated review
        id: Option<String>,
        /// Revision index the user saw; defaults to the current one
        #[arg(long, value_name = "N")]
        revision: Option<usize>,
    },
    /// Re-resolve the target in the review's worktree; carry unchanged threads forward, mark changed ones outdated
    #[command(
        long_about = "Re-resolve the review's target in the worktree the review belongs to, whichever worktree runs the command. When the content or its HEAD commit moved, append a revision; open and accepted threads whose lines are unchanged and still within the new diff move to their new line numbers, and the rest become outdated through a status comment by dots review refresh. Dismissed, fixed and posted threads follow unchanged lines too but are never outdated; one left at an earlier revision cannot be reopened, so open a new thread instead. Whether a thread was fixed is a judgment for reviewers: compare with the returned delta diff_command. Whole-file threads carry forward while their path is still changed; review-level threads always do. The user's drafts carry forward the same way; a draft whose lines changed stays behind and cannot be submitted."
    )]
    Refresh { id: String },
    /// Delete a review and release its pinned content
    Remove { id: String },
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(crate) struct DiscardWhich {
    /// Draft id, like d1
    #[arg(long, value_name = "DRAFT")]
    pub draft: Option<String>,
    /// Every draft
    #[arg(long)]
    pub all: bool,
}

/// One of the user's comments, as `dots review draft` takes it.
#[derive(Debug, Args)]
pub(crate) struct ReviewDraft {
    /// Review id; defaults to this worktree's most recently updated review
    pub id: Option<String>,
    /// Reply to this submitted thread, like t1
    #[arg(long, value_name = "THREAD", conflicts_with = "path")]
    pub thread: Option<String>,
    /// Anchor a new thread to this changed path
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,
    /// Side the lines count on
    #[arg(long, value_parser = ["new", "old"], requires_all = ["path", "start"])]
    pub side: Option<String>,
    /// First line
    #[arg(long, value_name = "N", requires = "side")]
    pub start: Option<u32>,
    /// Last line; defaults to --start
    #[arg(long, value_name = "N", requires = "start")]
    pub end: Option<u32>,
    /// The view the lines count in
    #[arg(long, value_parser = ["everything", "uncommitted", "committed"], default_value = "everything")]
    pub preset: String,
    /// Revision index the anchor was read against; refuse the draft if the review has moved
    #[arg(long, value_name = "N")]
    pub revision: Option<usize>,
    #[command(flatten)]
    pub body: ReviewBody,
}

/// A comment body, given inline or read from a file.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewBody {
    /// Comment body (Markdown)
    #[arg(long, value_name = "TEXT")]
    pub body: Option<String>,
    /// File holding the comment body, or - for stdin
    #[arg(long, value_name = "PATH")]
    pub body_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewTarget {
    /// Uncommitted changes against HEAD
    #[arg(long)]
    pub uncommitted: bool,
    /// Changes since the merge base with REF, including uncommitted work
    #[arg(long, value_name = "REF")]
    pub base: Option<String>,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub(crate) struct GitHubReviewArgs {
    #[command(flatten)]
    pub list: GitHubReviewListArgs,
    #[command(subcommand)]
    pub command: Option<GitHubReviewCommand>,
}
#[derive(Debug, Subcommand)]
pub(crate) enum GitHubReviewCommand {
    /// Read an exact review thread, its replies, and original reviewed diff context
    #[command(
        long_about = "Read an exact review conversation. Diff context appears only at offset zero; later pages provide context_command to recover it. original_commit_id identifies the original reviewed revision; commit_id is GitHub's current comment association and may be a later commit. full_diff_hunk_command retrieves the untruncated review hunk, not surrounding file source. Use gh or the checkout for broader context. has_more indicates another page; complete means all comments are in this page. Snapshot reads need local credentials but make no GitHub requests."
    )]
    Thread(GitHubReviewThreadArgs),
}
#[derive(Debug, Args)]
pub(crate) struct GitHubReviewListArgs {
    /// PR URL or number; defaults to the current branch's PR
    #[arg(long)]
    pub pr: Option<String>,
    /// Page a saved thread index without GitHub queries
    #[arg(long, conflicts_with_all = ["pr", "repo", "no_cache"])]
    pub snapshot: Option<SnapshotRef>,
    /// Include resolved threads; outdated and resolved are separate states
    #[arg(long)]
    pub all: bool,
    /// Maximum thread previews to return
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=20))]
    pub limit: u16,
    /// Skip this many matching threads
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=100000))]
    pub offset: u32,
}
#[derive(Debug, Args)]
pub(crate) struct GitHubReviewThreadArgs {
    /// Exact reference emitted by review-thread discovery
    #[arg(conflicts_with = "repo")]
    pub reference: github_inspect::reviews::ThreadRef,
    /// Page the saved conversation without GitHub queries
    #[arg(long, conflicts_with = "no_cache")]
    pub snapshot: Option<SnapshotRef>,
    /// Maximum comments to return; each body is bounded to 4 KiB
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..=20))]
    pub limit: u16,
    /// Skip this many comments in chronological API order
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=100000))]
    pub offset: u32,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AttentionCommand {
    /// Raise a request for a goal this session coordinates; a repeated key returns the existing request
    #[command(
        long_about = "Raise a request. The content file holds {\"kind\": \"decision\"|\"authorization\"|\"action\", \"title\": one line of at most 120 bytes, \"question\": Markdown of at most 4096 bytes, \"context\": optional rationale (8192), \"recommendation\": optional (1024), \"choices\": optional, at most 8 of {\"id\": 1..32 of a-z 0-9 -, \"label\": one line (80), \"detail\": optional (512)} and none for an action, \"tasks\": goal root or descendant IDs it holds (at most 32; naming the root holds the whole goal; none holds nothing)}. --key names the question's purpose within the goal (1..64 of a-z 0-9 . _ -). The same key with identical content returns the existing request in whatever state it reached (created false), with its answer or closure; different content fails: supersede an open request, or choose a new key once it is answered or closed. Claims on newly held work are reported; steer their holders."
    )]
    Raise {
        /// Goal root ID
        goal: String,
        #[arg(long, value_name = "KEY")]
        key: String,
        #[arg(long, value_name = "PATH")]
        content_file: PathBuf,
    },
    /// Replace an open request's content with a new request under the same key
    #[command(
        long_about = "Close an open request as superseded and raise its replacement, in one transaction, with content shaped as for raise. An answered request cannot be superseded: incorporate its answer. Answers sent to the old request later fail and name the replacement; they are never moved onto it."
    )]
    Supersede {
        request: String,
        #[arg(long, value_name = "PATH")]
        content_file: PathBuf,
    },
    /// Close an open request without an answer
    Withdraw {
        request: String,
        #[arg(long)]
        reason: String,
    },
    /// Record a human answer to an open request, from any session
    #[command(
        long_about = "Answer an open request. The content file holds {\"choice\": one of the request's choice ids, \"text\": the human's own words (4096), \"source\": \"ui\"|\"chat\", \"provenance\": required one line (512) naming where the human input came from (the viewer and terminal for ui; the session and message entry for chat), \"interpretation\": an agent's reading (4096)}; give a choice, text or both. source ui is an explicit human selection or entry and has no interpretation. source chat maps a real user message: quote the smallest relevant instruction as text and record the interpretation. Vague assent, cancellation, silence or dismissing a prompt is not an answer. Fails when the request is already answered, closed or superseded, naming the replacement."
    )]
    Answer {
        request: String,
        #[arg(long, value_name = "PATH")]
        content_file: PathBuf,
    },
    /// Record the coordinator's decision on an answered request and close it
    #[command(
        long_about = "Incorporate an answered request: the content file holds {\"kind\": \"decision\"|\"mandate\", \"decision\": Markdown (8192) stating what was decided and how the work changes}. The decision and the closure are recorded together, which releases the tasks the request held."
    )]
    Incorporate {
        request: String,
        #[arg(long, value_name = "PATH")]
        content_file: PathBuf,
    },
    /// Record a goal decision or execution mandate established outside any request
    #[command(
        long_about = "Record a decision the user made in ordinary conversation. The content file holds {\"kind\": \"mandate\"|\"decision\", \"decision\": the coordinator's account (8192), \"human\": {\"text\": the user's own relevant words (4096), \"source\": \"chat\"|\"ui\", \"provenance\": required one line (512): the session and message entry where the user said it}}. A mandate records the accepted execution agreement: outcome and scope, finish line, delegated work, publication and monitoring, and excluded actions; the newest mandate is current. Human input is required: a model-authored summary is not consent."
    )]
    Decide {
        /// Goal root ID
        goal: String,
        #[arg(long, value_name = "PATH")]
        content_file: PathBuf,
    },
    /// Show one request with its answer and closure
    Show { request: String },
    /// List requests in an explicit scope; never falls back to another scope
    #[command(
        long_about = "List requests grouped by goal. --scope global reads every registered project: goals with matching requests, plus unfinished goals with a recorded mandate (requests may be empty) so stopped work stays discoverable. --scope goals reads exactly the --goal identities, listing each even without requests; none reads nothing. coordinator is the current root claim holder, not evidence that its session is running; null means nobody coordinates. Projects or goals that cannot be read appear under errors; a missing or unreadable catalog fails the command."
    )]
    List {
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long, value_enum, default_value_t = RequestStates::Outstanding)]
        state: RequestStates,
    },
    /// List goal decisions and mandates in an explicit scope, oldest first
    Decisions {
        #[command(flatten)]
        scope: ScopeArgs,
        #[arg(long, value_enum)]
        kind: Option<DecisionKinds>,
    },
    /// Review and answer requests in an explicit scope in a terminal view
    View {
        #[command(flatten)]
        scope: ScopeArgs,
        /// Action the view runs when it closes, as given by the opener
        #[arg(long, value_name = "ACTION")]
        dismiss: Option<String>,
    },
}

/// Which goals an attention read covers.
#[derive(Debug, Args)]
pub(crate) struct ScopeArgs {
    /// global: every registered project; goals: exactly the --goal identities
    #[arg(long, value_enum)]
    pub scope: AttentionScope,
    /// Goal as PROJECT:GOAL; repeatable, only with --scope goals
    #[arg(long = "goal", value_name = "PROJECT:GOAL")]
    pub goals: Vec<String>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum AttentionScope {
    Global,
    Goals,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum RequestStates {
    /// Open or answered but not yet incorporated
    Outstanding,
    Open,
    Answered,
    Closed,
    All,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum DecisionKinds {
    Mandate,
    Decision,
}
