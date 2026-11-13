//! The command tree is the source for parsing, help, and agent documentation.
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use dots_memory::{ProjectId, Scope as MemoryScope, Snapshot};
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
        long_about = "Local work list shared by every session and worktree of one project: tasks, dependencies, claims, results, and issue/PR references. Git and GitHub remain the source of truth for code.\nThe project comes from the Git checkout at --directory, bound with dots project init; --project selects a registered project ID.\nA claim marks the actor responsible for a task. It lasts until that actor releases, hands off, or closes it; Pi releases only an ending runtime's claims and preserves them on reload. Workers report results to their parent session, which records them.\nA goal's tasks change only through its coordinator, the owner of its root claim (dots task goal coordinate); other sessions read them.\nThe catalog is $XDG_DATA_HOME/dots/workspace.sqlite3 (default ~/.local/share/dots/workspace.sqlite3)."
    )]
    Task {
        /// Registered project ID instead of the current Git common-directory binding
        #[arg(long, global = true, value_name = "KEY")]
        project: Option<String>,
        /// Actor for claims and events; defaults to the invoking agent's live runtime (`DOTS_AGENT_OWNER` when it names the invoking session), else its session (pi-$PI_SESSION_ID, claude-$CLAUDE_CODE_SESSION_ID, or codex-$CODEX_SESSION_ID); supply explicitly for direct CLI use
        #[arg(long, global = true, value_name = "NAME")]
        owner: Option<String>,
        /// Run one sync pass first, importing other machines' current trees (when sharing is on)
        #[arg(long, global = true)]
        fresh: bool,
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Read and publish shared knowledge snapshots across machines
    #[command(
        long_about = "Shared knowledge on the cross-machine substrate (see dots substrate). One snapshot pins all global/project documents and the shared project registry; its ID is the SHA-256 of its manifest. Reads never contact the remote: they serve this machine's local copy, which the sync agent (dots sync run) keeps current, and report its freshness: when it was last confirmed current (as_of, age_seconds), stale once that is over two minutes old, and the failing sync's error. --fresh runs one sync pass first. A machine that has never synced has no copy and says so. Publication syncs, then changes a batch against an expected snapshot: disjoint concurrent changes merge, same-document divergence waits for memory reconcile, and offline or repeatedly pre-empted publications stay pending on this machine. Tasks and outputs replicate only through dots task share; credentials and checkout bindings remain machine-local."
    )]
    Memory {
        /// Registered project ID instead of resolving the current checkout
        #[arg(long, global = true, value_name = "ID")]
        project: Option<String>,
        /// Run one sync pass before reading, instead of serving the local copy as is
        #[arg(long, global = true)]
        fresh: bool,
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Configure and inspect this machine's connection to the shared cross-machine remote
    #[command(
        long_about = "The substrate stores content-addressed blobs and compare-and-swap refs in one shared Git remote. Each machine keeps a verified blob cache, a Git cache repository and a SQLite state database (listing and freshness record, known values, outbox, stranded updates, push budget, sync health, counters and machine identity) under $XDG_DATA_HOME/dots/substrate (default ~/.local/share/dots/substrate). Domains such as memory read the local copy; dots sync keeps it current. This group configures the remote and reports local state."
    )]
    Substrate {
        #[command(subcommand)]
        command: SubstrateCommand,
    },
    /// Keep this machine's substrate copy current with the shared remote
    #[command(
        long_about = "A sync pass lists every ref in one request (5 second deadline), fetches the values that changed into the local cache, flushes deferred updates within the push budget and records the outcome. Passes are idempotent and run one at a time per machine; a failed pass backs off later scheduled passes with jittered exponential delays (5 seconds doubling to 5 minutes). Without a configured substrate there is nothing to sync."
    )]
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
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
    Init { name: String },
    /// Show or set up this checkout's sharing across machines: bind, create, or decline
    #[command(
        long_about = "Without an action, report whether this checkout's project is registered, bound to a shared identity (which one), or declined on this machine, whether this machine shares (dots task share enable), and the shared identities known to knowledge. Names never establish identity: pick --bind with an identity that is this same repository, or --create for a new one. Each action registers the checkout first when needed. Binding replicates the project's goals and tasks while machine sharing is on, and its project knowledge; it queues existing trees at once. --decline remembers on this machine that the user does not want the project shared, so agents stop asking; binding later clears it."
    )]
    Share(ProjectShare),
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

#[derive(Debug, Args)]
#[group(multiple = false)]
pub(crate) struct ProjectShare {
    /// Bind this checkout to an existing shared identity
    #[arg(long, value_name = "SHARED_ID")]
    pub bind: Option<ProjectId>,
    /// Create a shared identity with this display name and bind this checkout to it
    #[arg(long, value_name = "NAME")]
    pub create: Option<String>,
    /// Remember on this machine not to share this checkout's project
    #[arg(long)]
    pub decline: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SubstrateCommand {
    /// Point this machine at its shared remote; refuses to change an existing configuration
    #[command(
        long_about = "Write this machine's substrate configuration: the Git remote URL (a private repository such as https://github.com/OWNER/REPO.git, or file:// for tests) and an optional Git credential helper command, such as '!gh auth git-credential'. Credentials come only from that helper; dots never reads or stores tokens. An identical rerun succeeds; changing an existing configuration is refused."
    )]
    Init {
        #[arg(long, value_name = "URL")]
        remote: String,
        #[arg(long, value_name = "CMD")]
        credential_helper: Option<String>,
    },
    /// Show the remote, machine identity, last pass and error, backoff, unflushed and stranded updates and counters without networking
    Status,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SyncCommand {
    /// Run one sync pass now, waiting for one in progress, then pack the Git cache if needed
    Once,
    /// Run the per-machine sync agent until SIGTERM or SIGINT
    #[command(
        long_about = "The sync agent the login service runs. It runs a scheduled pass every --interval seconds (sooner when another process ran the last one), immediately when a local writer touches the state directory's changes marker with updates to flush, and on watch hints, which lengthen the period up to --resync. Failed passes back off. After passes that moved data it packs the Git cache once loose objects pass a few thousand. Until the substrate is configured it idles. It exits 0 on SIGTERM, SIGINT or SIGHUP after the pass in progress, and nonzero when the state schema is newer than this binary (as after an upgrade) or the state cannot be read, so the service manager restarts the installed binary."
    )]
    Run {
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..=3600), value_name = "SECONDS")]
        interval: u64,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=86400), value_name = "SECONDS")]
        resync: u64,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum MemoryCommand {
    /// Publish the first, empty snapshot to the substrate; refuses if knowledge exists
    Init,
    /// Read both MEMORY.md indexes at one snapshot from the local copy
    Context,
    /// Show the current snapshot, freshness and retained candidates with their integration plans
    Status,
    /// Read a published page; scope is global or an exact shared project ID
    Read {
        path: PathBuf,
        #[arg(long, value_parser=memory_scope, value_name="GLOBAL_OR_SHARED_ID")]
        scope: MemoryScope,
        #[arg(long)]
        snapshot: Option<Snapshot>,
        #[arg(long, default_value_t=1, value_parser=positive)]
        start: usize,
        #[arg(long, default_value_t=100, value_parser=clap::value_parser!(u16).range(1..=200))]
        lines: u16,
    },
    /// Publish one UTF-8 page against an expected snapshot; empty content is not removal
    Write {
        path: PathBuf,
        #[arg(long, value_parser=memory_scope, value_name="GLOBAL_OR_SHARED_ID")]
        scope: MemoryScope,
        #[arg(long)]
        expected_snapshot: Snapshot,
        /// UTF-8 file, or - to read bounded content from stdin
        #[arg(long)]
        content_file: PathBuf,
    },
    /// Remove one page against an expected snapshot; retain its published history
    Remove {
        path: PathBuf,
        #[arg(long, value_parser=memory_scope, value_name="GLOBAL_OR_SHARED_ID")]
        scope: MemoryScope,
        #[arg(long)]
        expected_snapshot: Snapshot,
    },
    /// Atomically publish a JSON array of changes against one expected snapshot
    #[command(
        long_about = "Publish a JSON array from --input-file (use - for bounded stdin). For one page use memory write or memory remove. Each change has scope ({kind:global} or {kind:project,project:SHARED_ID}), path, and action ({kind:write,content:TEXT} or {kind:remove}); JSON keys and string values need quotes. All changes share --expected-snapshot from context/read. If knowledge moved on since that snapshot, changes to other documents merge automatically into a two-parent snapshot; a document that also changed elsewhere fails the publication and retains it as a candidate for memory reconcile. Offline, the publication is retained as a pending candidate (state pending) and published by the next online publication or reconcile. Empty files and removals are distinct."
    )]
    Publish {
        #[arg(long)]
        expected_snapshot: Snapshot,
        /// JSON file, or - to read bounded changes from stdin
        #[arg(long)]
        input_file: PathBuf,
    },
    /// Literal search at one published snapshot; default is global plus this bound project
    Search {
        query: String,
        #[arg(long, value_parser=memory_scope, value_name="GLOBAL_OR_SHARED_ID")]
        scope: Option<MemoryScope>,
        #[arg(long)]
        snapshot: Option<Snapshot>,
        #[arg(long, default_value_t=0, value_parser=clap::value_parser!(u32).range(0..=100000))]
        offset: u32,
        #[arg(long, default_value_t=5, value_parser=clap::value_parser!(u16).range(1..=20))]
        limit: u16,
    },
    /// Manage shared identities and their separate machine-local catalog bindings
    Project {
        #[command(subcommand)]
        command: MemoryProjectCommand,
    },
    /// Publish a retained candidate against the head its plan names, with explicit choices
    #[command(
        long_about = "Read JSON {local, incoming, resolutions} from --input-file (use - for bounded stdin): local is a retained candidate and incoming the published snapshot from its plan in memory status. Each resolution names one conflicting resource and a choice: {kind:local}, {kind:incoming}, {kind:write,content:TEXT} or {kind:remove}. Every conflict needs exactly one choice; a candidate without conflicts takes an empty list. If knowledge moved past incoming, nothing changes; re-plan from memory status."
    )]
    Reconcile {
        #[arg(long)]
        input_file: PathBuf,
    },
    /// Read a page of a retained, unpublished candidate, labeled as such
    Inspect {
        #[arg(long)]
        candidate: Snapshot,
        path: PathBuf,
        #[arg(long, value_parser=memory_scope, value_name="GLOBAL_OR_SHARED_ID")]
        scope: MemoryScope,
        #[arg(long, default_value_t=1, value_parser=positive)]
        start: usize,
        #[arg(long, default_value_t=100, value_parser=clap::value_parser!(u16).range(1..=200))]
        lines: u16,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum MemoryProjectCommand {
    /// Register a shared identity in the atomic knowledge snapshot
    Create {
        name: String,
        #[arg(long)]
        expected_snapshot: Snapshot,
    },
    /// List shared identities; names never select identities
    List,
    /// Associate this local catalog project with a shared identity and queue its trees for sharing; refuse conflicting ownership
    Bind { id: ProjectId },
    /// Remove an exact local association without deleting shared knowledge
    Unbind {
        #[arg(long)]
        expected: ProjectId,
    },
}

fn memory_scope(value: &str) -> std::result::Result<MemoryScope, String> {
    if value == "global" {
        return Ok(MemoryScope::Global);
    }
    value
        .parse::<ProjectId>()
        .map(MemoryScope::Project)
        .map_err(|_| {
            "scope must be global or an exact shared project ID (p- plus 32 lowercase hex digits)"
                .into()
        })
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
    /// Replicate goals and task trees of bound projects across machines
    #[command(
        long_about = "Off by default and switched per machine. While on, every goal or top-level task tree of a project bound to a shared identity (dots project share) replicates on the substrate (dots substrate). One machine, the tree's home, changes a tree: its writes commit locally at once and the sync agent pushes them within the push budget. Other machines import the tree read-only on each sync pass (or --fresh) and refuse changes to it, naming dots task share take. Unbound projects and live claims never leave the machine."
    )]
    Share {
        #[command(subcommand)]
        command: ShareCommand,
    },
    #[command(flatten)]
    Query(TaskQuery),
    #[command(flatten)]
    Mutation(TaskMutation),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ShareCommand {
    /// Turn sharing on for this machine and queue every bound project's trees for export
    Enable,
    /// Turn sharing off; imported trees stay read-only as last seen
    Disable,
    /// Show this machine's switch, trees, pending replication, withheld parts and updates refused after a takeover
    Status,
    /// Make this machine the home of a tree imported from another machine
    #[command(
        long_about = "Compare-and-swap from the version this machine last imported. When the tree changed since, the command fetches and imports the new version, reports what changed and refuses; rerun it to take that version. Afterwards coordinate the goal here (dots task goal coordinate) before changing it; claims never move between machines."
    )]
    Take {
        /// The tree's top-level task (the goal ID for a goal)
        root: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum GoalPreview {
    Graph,
    Brief,
    History,
    Record,
    Task,
}

impl GoalPreview {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Brief => "brief",
            Self::History => "history",
            Self::Record => "record",
            Self::Task => "task",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Graph => "Dependency graph",
            Self::Brief => "Brief",
            Self::History => "History",
            Self::Record => "Full record",
            Self::Task => "Task record",
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum GoalCommand {
    /// Render a goal's read-only graph or inspection page
    Preview {
        id: String,
        #[arg(long, value_enum, default_value = "graph")]
        page: GoalPreview,
        #[arg(long, required_if_eq("page", "task"))]
        task: Option<String>,
    },
    /// Browse goals across registered projects without claiming or starting work
    View {
        /// Preselect an exact goal, including a completed one
        #[arg(long, value_name = "PROJECT:GOAL")]
        select: Option<String>,
        /// Action the view runs when it closes
        #[arg(long, value_name = "ACTION")]
        dismiss: Option<String>,
    },
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
    /// Remove the brief association
    Detach { id: String },
    /// Take coordination of a goal: claim its root although work remains below it
    #[command(
        long_about = "Claim a goal's root as its coordinator. Only the coordinator dispatches, edits, accepts or closes the goal's tasks; other sessions read. Coordination is responsibility, not dispatch: it works with unfinished children, prerequisites and held work. A managed worker (DOTS_WORKER_ID set) cannot coordinate, and a Pi session needs its live runtime owner unless --owner is given. Another owner's claim on the root or any descendant fails unless --take, which moves every such claim to this owner in one transaction and records each transfer. A held (paused) or closed root needs --resume, which reopens it. Coordinating again is a no-op. Stop coordinating with dots task release ID; pause with dots task hold ID, which holds every descendant."
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
