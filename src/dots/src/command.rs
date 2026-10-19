//! The command tree is the source for parsing, help, and agent documentation.
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use github_inspect::{Repository, SnapshotRef};

#[derive(Debug, Parser)]
#[command(name = "dots", version, about = "Workflow tools for coding agents", long_about = "Workflow tools for coding agents across local tools, services, and host integrations.\nUse the available command groups for supported tasks; native tools remain available.\nHelp and skill generation work offline. Commands document their own effects and prerequisites.", disable_help_subcommand = true, term_width = 100, color = clap::ColorChoice::Never)]
pub(crate) struct Cli {
    /// Explicit repository for numeric IDs or refs; URLs must agree with it
    #[arg(long, global = true, value_name = "[HOST/]OWNER/REPO")]
    pub repo: Option<Repository>,
    /// Return JSON instead of readable output
    #[arg(long, global = true)]
    pub json: bool,
    /// Bypass log caching and saved observations; fresh inspection always queries status
    #[arg(long, global = true)]
    pub no_cache: bool,
    /// Working directory for commands that use checkout context
    #[arg(long, global = true, value_name = "PATH")]
    pub directory: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Register and inspect PR associations in Herdr workspaces
    Herdr {
        #[command(subcommand)]
        command: HerdrCommand,
    },
    /// Inspect GitHub CI evidence and PR review threads
    Github {
        #[command(subcommand)]
        command: GitHubCommand,
    },
    /// Track shared, claimable work for a project across concurrent sessions
    #[command(
        long_about = "Local SQLite work list shared by every session and worktree of one project. Records intent and coordination: open slices, dependencies, atomic claims with leases, and the PR each slice produced. Git and GitHub remain the truth about code; reconcile against them when resuming.\nThe project defaults to the main checkout of the Git repository at --directory, so linked worktrees share one list; --project selects an explicit key.\nParent sessions create, claim, update and close tasks. Delegated workers report results; they do not edit the list. Claims expire so an abandoned session cannot hold work forever; renew by claiming again with the same owner. No network access."
    )]
    Task {
        /// Explicit project key instead of the main Git checkout path
        #[arg(long, global = true, value_name = "KEY")]
        project: Option<String>,
        /// Session or agent name recorded on claims and events
        #[arg(long, global = true, value_name = "NAME")]
        owner: Option<String>,
        #[command(subcommand)]
        command: TaskCommand,
    },
    /// Export agent discovery documentation from this command tree
    Skills {
        #[command(subcommand)]
        command: SkillCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskCommand {
    /// Associate durable briefs with task roots; inspect recorded progress without executing work
    #[command(
        long_about = "Optional goal context on existing root tasks. The root task id is the goal id; slices, claims, and status stay in the task store. Briefs remain external Markdown documents, independent of memory storage. These commands neither execute work nor authorize continuation. Overview counts describe local records, not verified GitHub or worker state."
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
    /// Create a root task and attach its brief atomically; no claims or execution
    Create {
        title: String,
        /// Existing Markdown file; relative to --directory, stored as an absolute path
        #[arg(long, value_name = "PATH")]
        brief: PathBuf,
    },
    /// Attach or replace a root's brief reference; never changes the document or executes work
    Attach {
        id: String,
        /// Existing Markdown file; relative to --directory (or the current directory), stored as an absolute path
        #[arg(long, value_name = "PATH")]
        brief: PathBuf,
    },
    /// List explicitly attached goals; open and held by default
    List {
        /// Include done and dropped goals
        #[arg(long)]
        all: bool,
    },
    /// Show open/held descendants and complete progress counts; no network or document reads
    Show {
        id: String,
        /// Include done/dropped task details; totals always include them
        #[arg(long)]
        all: bool,
    },
    /// Remove only the association; keep the document, tasks, and history
    Detach { id: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskQuery {
    /// List open/held roots by default; use --parent ID to drill down or --flat for every level
    #[command(
        long_about = "Project-scoped task overview: open and held root tasks by default, highest priority then oldest first. Child counts keep slices grouped. Use --parent ID for descendants or --flat for every level; --claimed-by also searches every level unless --roots is explicit. --all adds closed history without changing the hierarchy scope. Nothing is deleted or auto-archived."
    )]
    List {
        /// Only tasks anywhere below this one
        #[arg(long, value_name = "ID", conflicts_with = "roots")]
        parent: Option<String>,
        /// Only tasks whose recorded owner is NAME, live lease or not
        #[arg(long, value_name = "NAME")]
        claimed_by: Option<String>,
        /// Only top-level tasks (the default unless --parent, --flat, or --claimed-by is given)
        #[arg(long, conflicts_with = "flat")]
        roots: bool,
        /// Every task level, rather than the default root overview
        #[arg(long, conflicts_with = "parent")]
        flat: bool,
        /// Exactly one status: open, held, done or dropped
        #[arg(long, conflicts_with = "all")]
        status: Option<String>,
        /// Include done and dropped tasks
        #[arg(long)]
        all: bool,
    },
    /// Fresh leaf work: open, unblocked, childless, no PR recorded, not under a live lease
    Ready {
        /// Only tasks anywhere below this one
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
    },
    /// Show one task with blockers, children count, and recent events
    Show { id: String },
    /// Project history after an event seq, oldest first; use the last seen seq as a cursor
    Events {
        /// Only events with seq greater than this (default 0)
        #[arg(long, default_value_t = 0)]
        since: i64,
        /// Maximum events to return
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u16).range(1..=1000))]
        limit: u16,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TaskMutation {
    /// Create an open task; --parent groups slices under a larger goal
    Add {
        title: String,
        /// Longer description, pattern reference, or acceptance notes
        #[arg(long)]
        body: Option<String>,
        /// Parent task ID
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        /// Higher runs first among ready tasks (default 0)
        #[arg(long, default_value_t = 0, allow_hyphen_values = true)]
        priority: i64,
        /// Prerequisite task IDs; repeatable
        #[arg(long, value_name = "ID")]
        blocked_by: Vec<String>,
    },
    /// Atomically claim a task (or the next ready one) for --owner with a lease
    #[command(
        long_about = "Atomically claim a task for --owner. Fails if another owner holds a live lease, the task is blocked, it has open children, or it is not open. Claiming again with the same owner renews the lease; an expired lease can be taken over. --next claims the highest-priority ready task that has no children, optionally under --parent, and fails with exit 1 when nothing is ready. Claim before delegating a slice; release or close when the work stops."
    )]
    Claim {
        /// Task ID; omit with --next
        #[arg(required_unless_present = "next", conflicts_with = "next")]
        id: Option<String>,
        /// Claim the highest-priority ready task instead of a specific ID
        #[arg(long)]
        next: bool,
        /// With --next, only consider tasks anywhere below this one
        #[arg(long, value_name = "ID", requires = "next")]
        parent: Option<String>,
        /// Lease length in minutes, 1 to 10080 (default 120)
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u32).range(1..=10080))]
        lease: u32,
    },
    /// Release --owner's claim without changing status
    Release { id: String },
    /// Edit fields or append a note; recording the PR URL is the usual update
    #[command(
        long_about = "Edit fields or append a note. Field edits on a task under another owner's live lease are refused unless --force; a --note is always allowed. Once a PR is recorded the slice leaves the ready pool: claim --next skips it even after the lease expires, and picking it up again means claiming it by id."
    )]
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        /// Full pull/merge request URL produced for this task
        #[arg(long, value_name = "URL")]
        pr: Option<String>,
        /// Branch carrying the work, so another session can find it
        #[arg(long)]
        branch: Option<String>,
        #[arg(long, allow_hyphen_values = true)]
        priority: Option<i64>,
        /// New parent task ID; an empty string detaches
        #[arg(long, value_name = "ID")]
        parent: Option<String>,
        /// Append a dated note to the task history
        #[arg(long)]
        note: Option<String>,
        /// Edit fields despite another owner's live lease
        #[arg(long)]
        force: bool,
    },
    /// Mark done; refuses while blocked, with open children, or leased to another owner unless --force
    Done {
        id: String,
        /// Outcome, e.g. the merge or verification evidence
        #[arg(long)]
        note: Option<String>,
        /// Close anyway: ignore blockers, open children, and another owner's live lease
        #[arg(long)]
        force: bool,
    },
    /// Pause this task with a reason; it is never ready, its claim is cleared; descendants stay claimable (warned)
    Hold {
        id: String,
        #[arg(long)]
        reason: String,
        /// Override another owner's live lease
        #[arg(long)]
        force: bool,
    },
    /// Close a task as not going to happen; dependents are unblocked; same --force rules as done
    Drop {
        id: String,
        #[arg(long)]
        reason: String,
        /// Close anyway: ignore blockers, open children, and another owner's live lease
        #[arg(long)]
        force: bool,
    },
    /// Return a held, done or dropped task to open; fails if it is already open
    Reopen {
        id: String,
        #[arg(long)]
        note: Option<String>,
        /// Override another owner's live lease
        #[arg(long)]
        force: bool,
    },
    /// Manage prerequisites between tasks
    Dep {
        #[command(subcommand)]
        command: DepCommand,
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
pub(crate) enum HerdrCommand {
    /// Explicit workspace PR associations; local writes, no GitHub mutations
    #[command(
        long_about = "Register every PR created or worked on in this Herdr workspace. Associations survive branch changes and agent completion. Requires a host with persistent workspace identities. No GitHub publication, CI fixes, or merges. Use a full PR URL; never substitute the focused workspace. Registration is safe to repeat after shipping or resuming work. An explicit --workspace is required. Verify it against the task and live workspace inventory; inherited pane/workspace IDs, current directory, and UI focus are not reliable ownership evidence."
    )]
    Prs {
        /// Verified workspace handle; required, never inferred from environment or focus
        #[arg(long, global = true)]
        workspace: Option<String>,
        #[command(subcommand)]
        command: PrAssociationCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum PrAssociationCommand {
    /// Attach a PR to this workspace; repeat calls are harmless
    Register { url: String },
    /// List every PR registered in this workspace
    List,
    /// Remove this workspace's association without changing the PR
    Unregister { url: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum GitHubCommand {
    /// Inspect PR, ref, commit, or workflow-run CI and retrieve failure evidence
    #[command(
        long_about = "Inspect CI for a PR, remote ref, commit, or exact workflow run/attempt.\nRead-only; uses existing gh authentication and needs no Herdr session.\nWith no subcommand, inspect the current branch's PR. Use --pr URL for another PR, --ref main or --sha SHA with --repo, or --run RUN_URL [--attempt N].\nA ref selects its current commit, not the latest historical/scheduled run. Use gh run list for run discovery; pass a run URL here for diagnosis.\nDetails default to failed, cancelled, unknown and pending checks. --all includes passing/skipped checks; --name TEXT searches check names across all states. Both remain paged; summary always covers every check.\nFailed or pending CI is data, not a command error. No checks does not mean CI passed.\nFollow returned commands rather than constructing job references or selecting the latest run."
    )]
    Ci(Box<CiArgs>),
    /// Inspect unresolved PR review threads and read exact conversations
    #[command(
        long_about = "Read-only review-thread investigation. Defaults to unresolved threads, including outdated feedback. Opening-comment previews are not full conversations. Follow the returned thread command for replies and reviewed diff context. Use --json for agent workflows. General PR discussion, review submission, replies, resolution, and merging remain with gh or GitHub. Fresh lists traverse at most 1000 threads; conversations at most 200 comments. Query errors and incomplete traversals fail explicitly. Saved observations expire after five minutes; paging requires local credential/keychain access but makes no GitHub requests. Refresh preserves the filter and page size and restarts at offset zero. JSON has_more indicates continuation; complete means this page contains the entire matching collection."
    )]
    Reviews(Box<ReviewArgs>),
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
pub(crate) struct ReviewArgs {
    #[command(flatten)]
    pub list: ReviewListArgs,
    #[command(subcommand)]
    pub command: Option<ReviewCommand>,
}
#[derive(Debug, Subcommand)]
pub(crate) enum ReviewCommand {
    /// Read an exact review thread, its replies, and original reviewed diff context
    #[command(
        long_about = "Read an exact review conversation. Diff context appears only at offset zero; later pages provide context_command to recover it. original_commit_id identifies the original reviewed revision; commit_id is GitHub's current comment association and may be a later commit. full_diff_hunk_command retrieves the untruncated review hunk, not surrounding file source. Use gh or the checkout for broader context. has_more indicates another page; complete means all comments are in this page. Snapshot reads need local credentials but make no GitHub requests."
    )]
    Thread(ReviewThreadArgs),
}
#[derive(Debug, Args)]
pub(crate) struct ReviewListArgs {
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
pub(crate) struct ReviewThreadArgs {
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
