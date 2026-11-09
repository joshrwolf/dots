//! The interactive viewer: one event loop owns the state; loading, diffing,
//! highlighting, watching and the control socket run on threads that send
//! it messages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

use crate::app::{App, Body, FileTexts, Line, Mode, Settings, Summary};
use crate::ctl::{self, Command};
use crate::deliver;
use crate::graphics::Graphics;
use crate::highlight::{self, Key};
use crate::input::{self, Input, Replies};
use crate::moves;
use crate::repo::{self, Changes, FileChange, GitState, Reader};
use crate::review;
use crate::terminal;
use crate::theme::{self, Detected, Palette};
use crate::ui;
use crate::watch::{Refresh, Touched, Watch};

mod annotate;
mod interact;
mod refresh;

use refresh::{Pipeline, SummaryQueue};

#[expect(
    clippy::large_enum_variant,
    reason = "each message is moved once through the channel; boxing buys nothing"
)]
pub(crate) enum Msg {
    Input(Input),
    Loaded {
        generation: u64,
        worktree: PathBuf,
        mode: repo::Mode,
        result: Result<Changes, String>,
        /// The worktree's Git directory and saved review.
        review: Result<(PathBuf, review::State), String>,
    },
    /// Who a review of `worktree` goes to, and the live sessions.
    Resolved {
        /// Numbers resolutions; only the latest one counts.
        seq: u64,
        worktree: PathBuf,
        resolution: deliver::Resolution,
        agents: Vec<crate::agents::Agent>,
        /// Registrations that could not be read.
        problems: Vec<String>,
    },
    /// A review was delivered, or not.
    Sent {
        worktree: PathBuf,
        /// Where the review it was sent from is saved.
        dir: Option<PathBuf>,
        ids: Vec<u64>,
        result: Result<annotate::Delivered, String>,
    },
    /// A moved-code search finished.
    Moves {
        epoch: u64,
        seq: u64,
        moves: Arc<moves::Moves>,
    },
    Diffed {
        epoch: u64,
        file: usize,
        body: Body,
        texts: Option<FileTexts>,
    },
    /// A path-limited rescan after file events.
    Partial {
        generation: u64,
        paths: Vec<String>,
        result: Result<Vec<FileChange>, String>,
    },
    /// The periodic safety-net check.
    Reconciled {
        generation: u64,
        state: Option<GitState>,
        /// Working-tree files whose size or time moved without an event.
        moved: Vec<String>,
    },
    Highlighted(highlight::Done),
    Summary {
        path: PathBuf,
        summary: Result<Summary, String>,
    },
    Touched(Touched),
    Watching(Result<Watch, String>),
    /// Interactive Pi sessions and workers changed.
    Agents(crate::agents::Snapshot),
    /// Which agent wrote which file of a worktree.
    Attributed(crate::attribution::Attribution),
    Ctl(Command),
}

#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) path: PathBuf,
    /// A file to scroll to once loaded.
    pub(crate) file: Option<String>,
    /// The Rex pane whose Pi session receives reviews of the first worktree.
    pub(crate) bind: Option<String>,
    /// Fixed colours instead of the terminal's.
    pub(crate) theme: Option<Detected>,
    pub(crate) settings: Settings,
}

/// How often the safety net checks for changes the watcher missed.
const RECONCILE: Duration = Duration::from_secs(10);

/// How often the colours are asked for.
const THEME_POLL: Duration = Duration::from_secs(2);
/// Quiet time before another worktree's switcher counts are recomputed.
const SUMMARY_SETTLE: Duration = Duration::from_secs(3);
/// Threads computing switcher counts.
const SUMMARY_THREADS: usize = 3;
const PREFETCH: usize = 3;

/// Following the terminal's colours. Terminals reporting theme changes
/// (mode 2031) report only a switch between light and dark, so the colours
/// are also asked for now and then.
#[derive(Debug)]
struct Theme {
    /// When to ask next; `None` when the colours were given on the command
    /// line.
    next_poll: Option<Instant>,
    detected: Option<Detected>,
    /// Ask for the colours at the next turn of the loop.
    ask: bool,
    /// The palette changed, so every cell is drawn again.
    changed: bool,
}

/// The latest moved-code search.
#[derive(Debug, Default)]
struct MovesSearch {
    /// The diffs it was started for, by fingerprint.
    started_for: Option<u64>,
    /// Numbers searches; only the latest one's result is kept.
    seq: u64,
}

struct Runtime {
    app: App,
    tx: Sender<Msg>,
    pipeline: Pipeline,
    pool: highlight::Pool,
    watcher: Option<Watch>,
    /// The worktree the file watch was set up around.
    watched: Option<usize>,
    summaries: SummaryQueue,
    summaries_started: bool,
    /// Worktrees whose switcher counts are stale, and when they last changed.
    stale: HashMap<PathBuf, Instant>,
    wanted: Vec<Key>,
    theme: Theme,
    quit: bool,
    moves: MovesSearch,
    /// What the control socket reports.
    ctl_status: Arc<Mutex<ctl::Status>>,
    /// A file to scroll to once the worktree loading now has loaded.
    pending_file: Option<String>,
    delivery: annotate::Delivery,
    /// Attributes files to agents, off the event loop; `None` in tests.
    attribution: Option<Sender<crate::attribution::Request>>,
    /// Session files that could not be read, as last reported.
    attribution_problems: Vec<String>,
    /// Save the next frame as a PNG here.
    shot: Option<PathBuf>,
}

pub(crate) fn run(options: &Options) -> Result<()> {
    let repo = repo::discover(&options.path)?;
    let current = repo.index_of(&options.path).unwrap_or(0);

    let mut terminal = terminal::start().context("starting the terminal")?;
    let result = view(&mut terminal, options, repo, current);
    terminal::stop(&mut terminal)?;
    result
}

/// Shows worktree `current` of `repo` in the started terminal until the
/// user quits.
fn view(
    terminal: &mut terminal::Tui,
    options: &Options,
    repo: repo::Repo,
    current: usize,
) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    input::spawn(tx.clone(), Msg::Input)?;
    // Input that arrives while the replies are awaited is handled after.
    let colours = if options.theme.is_some() {
        String::new()
    } else {
        theme::query()
    };
    terminal::send(
        terminal,
        &format!(
            "{}{colours}{}",
            crate::graphics::QUERY,
            terminal::END_OF_REPLIES
        ),
    )?;
    let (replies, early) = first_replies(&rx, Duration::from_millis(400));
    let detected = options.theme.or(replies.colours);
    let palette = Palette::new(detected.unwrap_or_else(Detected::light));
    let mut settings = options.settings.clone();
    if replies.graphics {
        settings.headers = crate::app::Headers::Images;
    }
    let app = App::new(repo, current, palette, settings);
    let (ctl_status, _cleanup) = serve_ctl(&tx, app.worktree_path());
    let agents_tx = tx.clone();
    crate::agents::follow(move |s| agents_tx.send(Msg::Agents(s)).is_ok());
    let attributed_tx = tx.clone();
    let mut rt = Runtime::new(app, tx, Threads::default());
    rt.attribution = Some(crate::attribution::spawn(move |a| {
        attributed_tx.send(Msg::Attributed(a)).is_ok()
    }));
    rt.ctl_status = ctl_status;
    rt.pipeline.loading = true;
    if options.theme.is_some() {
        rt.theme.next_poll = None;
    }
    rt.theme.detected = detected;
    rt.pending_file.clone_from(&options.file);
    rt.delivery.bound = options.bind.clone().map(|b| (b, current));
    let mut graphics = replies.graphics.then(Graphics::new);
    rt.start_load();
    rt.spawn_watch();
    for msg in early {
        rt.handle(msg);
    }
    let outcome = rt.event_loop(terminal, &rx, graphics.as_mut());
    if let Some(g) = graphics.as_mut() {
        terminal::send(terminal, &g.clear())?;
    }
    outcome
}

/// Waits up to `timeout` for the terminal to answer the startup queries,
/// keeping what else arrives meanwhile. A terminal that does not answer
/// gets light colours and text headers.
fn first_replies(rx: &Receiver<Msg>, timeout: Duration) -> (Replies, Vec<Msg>) {
    let deadline = Instant::now() + timeout;
    let mut early = Vec::new();
    while let Ok(msg) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        match msg {
            Msg::Input(Input::Replies(replies)) => return (replies, early),
            msg => early.push(msg),
        }
    }
    let none = Replies {
        colours: None,
        graphics: false,
    };
    (none, early)
}

/// Lists a worktree's changes in `mode` and reads its saved review. With
/// `since`, the changes are those since the review was last sent.
pub(crate) fn load(
    worktree: &Path,
    mode: repo::Mode,
    since: bool,
) -> (
    Result<Changes, String>,
    Result<(PathBuf, review::State), String>,
) {
    let mut review = Err(String::new());
    let changes = repo::changes(worktree, mode)
        .map(|mut changes| {
            let repo = changes.repo.to_thread_local();
            let git_dir = repo.git_dir().to_owned();
            review = review::State::load(&git_dir)
                .map(|s| (git_dir, s))
                .map_err(|e| format!("{e:#}"));
            if let (true, Ok((_, state))) = (since, &review) {
                let mut reader = Reader::new(repo, changes.root.clone());
                changes.files = review::since_sent(
                    changes.files,
                    &state.seen,
                    |_| true,
                    changes.target,
                    &mut reader,
                );
            }
            changes
        })
        .map_err(|e| format!("{e:#}"));
    (changes, review)
}

/// Background threads a runtime starts.
#[derive(Debug, Clone, Copy)]
struct Threads {
    highlight: usize,
    summaries: usize,
}

impl Default for Threads {
    fn default() -> Self {
        Self {
            highlight: 2,
            summaries: SUMMARY_THREADS,
        }
    }
}

impl Runtime {
    fn new(app: App, tx: Sender<Msg>, threads: Threads) -> Self {
        let pool_tx = Mutex::new(tx.clone());
        let pool = highlight::Pool::new(threads.highlight, move |done| {
            if let Ok(tx) = pool_tx.lock() {
                let _ = tx.send(Msg::Highlighted(done));
            }
        });
        let worktree = app.worktree_path().clone();
        Self {
            summaries: SummaryQueue::new(threads.summaries, &tx),
            app,
            tx,
            pipeline: Pipeline::new(),
            pool,
            watcher: None,
            stale: HashMap::new(),
            wanted: Vec::new(),
            theme: Theme {
                next_poll: Some(Instant::now() + THEME_POLL),
                detected: None,
                ask: false,
                changed: false,
            },
            quit: false,
            summaries_started: false,
            moves: MovesSearch::default(),
            watched: None,
            ctl_status: Arc::new(Mutex::new(ctl::Status {
                block: None,
                worktree,
            })),
            pending_file: None,
            delivery: annotate::Delivery::default(),
            attribution: None,
            attribution_problems: Vec::new(),
            shot: None,
        }
    }

    fn view_rows(&self) -> u16 {
        self.app.areas.diff.height.max(1)
    }

    fn event_loop(
        &mut self,
        terminal: &mut terminal::Tui,
        rx: &Receiver<Msg>,
        mut graphics: Option<&mut Graphics>,
    ) -> Result<()> {
        let mut dirty = true;
        while !self.quit {
            if dirty {
                if let Some(g) = graphics.as_deref_mut() {
                    // Images go out before the frame whose cells show them.
                    terminal.autoresize()?;
                    ui::lay_out(&mut self.app, terminal.get_frame().area());
                    terminal::send(terminal, &g.prepare(&mut self.app))?;
                }
                let completed = terminal.draw(|f| ui::draw(f, &mut self.app))?;
                if let Some(path) = self.shot.take()
                    && let Err(e) = crate::shot::picture(completed.buffer, &path)
                {
                    self.app.message = Some(format!("{e:#}"));
                }
                self.app.mark_notes_seen(self.view_rows());
                self.after_frame();
                dirty = false;
            }
            let now = Instant::now();
            let mut deadline = self.deadline(now);
            if graphics.as_deref().is_some_and(Graphics::pending) {
                deadline = deadline.min(now + Duration::from_millis(50));
                dirty = true;
            }
            match rx.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(msg) => dirty |= self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(msg) = rx.try_recv() {
                dirty |= self.handle(msg);
            }
            dirty |= self.tick(terminal)?;
        }
        Ok(())
    }

    /// When the loop next has work of its own, without messages.
    fn deadline(&self, now: Instant) -> Instant {
        let mut deadline = (now + Duration::from_secs(60)).min(self.pipeline.next_reconcile);
        if let Some(at) = self.theme.next_poll {
            deadline = deadline.min(at);
        }
        if let Some(at) = self.stale.values().min() {
            deadline = deadline.min(*at + SUMMARY_SETTLE);
        }
        if !self.pipeline.partial_busy
            && let Some(at) = self.pipeline.burst.due()
        {
            deadline = deadline.min(at);
        }
        if let Some(at) = self.app.flash_deadline() {
            deadline = deadline.min(at);
        }
        deadline
    }

    /// Does the loop's own work once messages are handled: saving,
    /// refreshes that are due, fading marks, the safety net, colour
    /// queries and stale switcher counts. Returns whether to draw.
    fn tick(&mut self, terminal: &mut terminal::Tui) -> Result<bool> {
        let mut dirty = false;
        self.refresh_moves();
        if self.app.session.dirty() {
            self.save_review();
        }
        let now = Instant::now();
        if !self.pipeline.partial_busy {
            match self.pipeline.burst.take(now) {
                Some(Refresh::Full) => self.start_load(),
                Some(Refresh::Paths(paths)) => self.start_partial(paths),
                None => {}
            }
        }
        if self.app.flash_deadline().is_some_and(|at| at <= now) {
            self.app.expire_flashes(now);
            dirty = true;
        }
        if self.pipeline.next_reconcile <= now {
            self.pipeline.next_reconcile = now + RECONCILE;
            self.spawn_reconcile();
            self.spawn_resolve();
        }
        if let Some(at) = self.theme.next_poll
            && at <= now
        {
            self.theme.next_poll = Some(now + THEME_POLL);
            self.theme.ask = true;
        }
        if std::mem::take(&mut self.theme.ask) {
            // The replies arrive as input.
            terminal::send(
                terminal,
                &format!("{}{}", theme::query(), terminal::END_OF_REPLIES),
            )?;
        }
        if std::mem::take(&mut self.theme.changed) {
            // Repaint everything: the buffer diff cannot know every cell changed.
            terminal::repaint(terminal)?;
            dirty = true;
        }
        let settled: Vec<PathBuf> = self
            .stale
            .iter()
            .filter(|(_, at)| **at + SUMMARY_SETTLE <= now)
            .map(|(p, _)| p.clone())
            .collect();
        for path in settled {
            self.stale.remove(&path);
            self.summaries.push(path, false);
        }
        Ok(dirty)
    }

    fn after_frame(&mut self) {
        if self.pipeline.loading
            && let Some(stream) = &self.app.stream
        {
            // A frame is complete once every file on screen has its diff.
            let rows = self.view_rows();
            let top = self.app.global_top();
            let ready = (top..top + u32::from(rows)).all(|l| match self.app.line_at(l) {
                Line::Note { file } => !matches!(stream.files[file].body, Body::Pending),
                _ => true,
            });
            if ready {
                self.pipeline.loading = false;
                if self.summaries_started {
                    // Its summary tells what the user is looking at.
                    self.summaries.push(self.app.worktree_path().clone(), true);
                } else {
                    // Rail counts wait for the first full frame so they never delay it.
                    self.summaries_started = true;
                    self.spawn_summaries();
                }
            }
        }
        if self.app.needs_diffs {
            self.app.needs_diffs = false;
            self.spawn_diffs();
        }
        let jobs = self.app.wanted_highlights(self.view_rows(), PREFETCH);
        let keys: Vec<Key> = jobs.iter().map(|j| j.key).collect();
        if keys != self.wanted {
            self.wanted = keys;
            self.pool.want(jobs);
        }
        if let Some(job) = &self.pipeline.diffs {
            job.focus.store(self.app.top.file, Ordering::Relaxed);
        }
    }

    #[expect(clippy::too_many_lines, reason = "one match arm per message or action")]
    fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Input(Input::Event(event)) => self.input(&event),
            Msg::Input(Input::Resized) => true,
            // Graphics are chosen once, at startup.
            Msg::Input(Input::Replies(replies)) => replies.colours.is_some_and(|d| self.colours(d)),
            Msg::Loaded {
                generation,
                worktree,
                mode,
                result,
                review,
            } => {
                if generation != self.pipeline.generation {
                    return false;
                }
                match result {
                    Ok(changes) => {
                        if !self.app.session.shows(&worktree) {
                            // Another worktree's review goes to its own agent.
                            self.delivery.resolution = None;
                            "→ …".clone_into(&mut self.app.target);
                        }
                        self.pipeline.state = Some(changes.state.clone());
                        self.pipeline.source = Some((changes.repo.clone(), changes.root.clone()));
                        self.app.message = self.app.install(worktree, mode, changes, review);
                        self.pipeline.epoch += 1;
                        self.spawn_diffs();
                        self.spawn_resolve();
                        if let Some(file) = self.pending_file.take() {
                            self.command(Command::File(file));
                        }
                    }
                    Err(e) => self.app.message = Some(e),
                }
                true
            }
            Msg::Moves { epoch, seq, moves } => {
                if epoch != self.pipeline.epoch || seq != self.moves.seq {
                    return false;
                }
                if moves.skipped {
                    self.app.message =
                        Some("too many changed lines to look for moved code".to_owned());
                }
                self.app.moves = Some(moves);
                true
            }
            Msg::Diffed {
                epoch,
                file,
                body,
                texts,
            } => {
                if epoch != self.pipeline.epoch {
                    return false;
                }
                self.app.set_body(file, body, texts);
                self.update_own_summary();
                true
            }
            Msg::Partial {
                generation,
                paths,
                result,
            } => {
                self.pipeline.partial_busy = false;
                if generation != self.pipeline.generation {
                    return false;
                }
                match result {
                    Ok(files) => {
                        self.app.apply_paths(&paths, files);
                        self.pipeline.epoch += 1;
                        self.spawn_diffs();
                    }
                    // The paths could not be scanned alone; scan everything.
                    Err(_) => self.pipeline.burst.push_full(Instant::now()),
                }
                true
            }
            Msg::Reconciled {
                generation,
                state,
                moved,
            } => {
                if generation != self.pipeline.generation {
                    return false;
                }
                if state.is_some() && state != self.pipeline.state {
                    // The full scan records the new state.
                    self.pipeline.burst.push_full(Instant::now());
                }
                if !moved.is_empty() {
                    self.pipeline.burst.push(Instant::now(), moved);
                }
                false
            }
            Msg::Highlighted(done) => {
                self.app.cache.keep_highlight(done.key, done.result.ok());
                true
            }
            Msg::Summary { path, summary } => {
                let Ok(s) = summary else { return false };
                // Showing a worktree is looking at it.
                if path == *self.app.worktree_path()
                    && self.app.session.shows(&path)
                    && let Some(content) = &s.content
                {
                    self.app.look(content);
                    // Its files changed, so their authors may have.
                    self.request_attribution();
                }
                self.app.summaries.insert(path, s);
                matches!(self.app.mode, Mode::Switcher(_))
            }
            Msg::Agents(snapshot) => {
                if let Some(problem) = self.app.set_agents(snapshot) {
                    self.app.message = Some(problem);
                }
                self.request_attribution();
                true
            }
            Msg::Attributed(attribution) => {
                if attribution.worktree != *self.app.worktree_path() {
                    return false;
                }
                if attribution.problems != self.attribution_problems {
                    self.attribution_problems.clone_from(&attribution.problems);
                    if let Some(p) = attribution.problems.first() {
                        self.app.message = Some(format!("not attributed: {p}"));
                    }
                }
                if self.app.set_attribution(Some(attribution)) {
                    self.start_load();
                }
                true
            }
            Msg::Touched(touched) => {
                let current = self.app.current;
                let now = Instant::now();
                match touched {
                    Touched::Files { worktree, paths } if worktree == current => {
                        self.pipeline.burst.push(now, paths);
                        self.stale.insert(self.app.worktree_path().clone(), now);
                    }
                    Touched::GitState(i) if i == current => {
                        self.pipeline.burst.push_full(now);
                        self.stale.insert(self.app.worktree_path().clone(), now);
                    }
                    Touched::Files { worktree: i, .. } | Touched::GitState(i) => {
                        if let Some(w) = self.app.repo.worktrees.get(i) {
                            self.stale.insert(w.path.clone(), now);
                        }
                    }
                    Touched::Refs => {
                        // Any worktree's base may have moved.
                        self.pipeline.burst.push_full(now);
                        for (i, w) in self.app.repo.worktrees.iter().enumerate() {
                            if i != current {
                                self.stale.insert(w.path.clone(), now);
                            }
                        }
                    }
                }
                false
            }
            Msg::Watching(result) => {
                match result {
                    Ok(w) => self.watcher = Some(w),
                    Err(e) => self.app.message = Some(format!("not watching: {e}")),
                }
                true
            }
            Msg::Ctl(command) => {
                self.command(command);
                true
            }
            Msg::Resolved {
                seq,
                worktree,
                resolution,
                agents,
                problems,
            } => {
                if seq != self.delivery.resolve_seq || worktree != *self.app.worktree_path() {
                    return false;
                }
                if let Some(problem) = problems.first() {
                    self.app.message = Some(format!("unreadable Pi registration {problem}"));
                }
                self.delivery.agents = agents;
                self.set_resolution(resolution);
                true
            }
            Msg::Sent {
                worktree,
                dir,
                ids,
                result,
            } => {
                self.on_sent(&worktree, dir.as_deref(), &ids, result);
                true
            }
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Refresh => self.start_load(),
            Command::Worktree(path) => {
                if !self.may_leave() {
                    return;
                }
                if let Some(i) = self.app.repo.index_of(&path) {
                    self.switch(i);
                } else {
                    match repo::discover(&path) {
                        Ok(repo) => {
                            let i = repo.index_of(&path).unwrap_or(0);
                            self.app.repo = repo;
                            self.app.match_owners();
                            self.app.summaries.clear();
                            self.summaries.reset();
                            self.watched = None;
                            self.show(i);
                            self.spawn_summaries();
                            self.spawn_watch();
                        }
                        Err(e) => self.app.message = Some(format!("{e:#}")),
                    }
                }
            }
            Command::Note(spec) => self.note(spec),
            Command::ClearNotes(path) => {
                let rel = path.map(|p| self.relative(&p).1);
                self.app.clear_notes(rel.as_deref());
            }
            Command::Bind(block) => {
                self.delivery.bound = Some((block, self.app.current));
                self.spawn_resolve();
            }
            Command::Shot(path) => self.shot = Some(path),
            Command::File(path) => {
                // A file in another worktree, or before anything loaded,
                // waits for its worktree's load.
                let elsewhere = Path::new(&path)
                    .is_absolute()
                    .then(|| self.app.repo.index_of(Path::new(&path)))
                    .flatten()
                    .filter(|&i| i != self.app.current);
                if let Some(i) = elsewhere {
                    if self.may_leave() {
                        self.pending_file = Some(path);
                        self.switch(i);
                    }
                    return;
                }
                if self.app.stream.is_none() {
                    self.pending_file = Some(path);
                    return;
                }
                let root = self.app.worktree_path().clone();
                let rel = Path::new(&path)
                    .strip_prefix(&root)
                    .map_or(path.clone(), |p| p.to_string_lossy().into_owned());
                match self.app.find_file(&rel) {
                    Some(i) => {
                        let rows = self.view_rows();
                        self.app.jump_to_file(i, rows);
                    }
                    None => self.app.message = Some(format!("{rel} has no changes here")),
                }
            }
        }
    }

    /// Shows another worktree of the repository.
    fn switch(&mut self, worktree: usize) {
        if worktree != self.app.current {
            self.show(worktree);
        }
    }

    /// Whether the shown worktree may be left: not while a comment on it is
    /// being written, which belongs to its review.
    fn may_leave(&mut self) -> bool {
        if self.app.editor().is_some() {
            self.app.message =
                Some("save or cancel the comment before showing another worktree".to_owned());
            return false;
        }
        true
    }

    /// Shows worktree `worktree`, loading it from scratch.
    fn show(&mut self, worktree: usize) {
        if worktree >= self.app.repo.worktrees.len() || !self.may_leave() {
            return;
        }
        self.app.current = worktree;
        // Authors are the shown worktree's.
        self.app.author_filter = None;
        self.app.set_attribution(None);
        self.request_attribution();
        self.reload();
        if !WATCH_ALL_FILES {
            self.spawn_watch();
        }
    }

    /// Loads the shown worktree from scratch, dropping its stream and the
    /// selection in it.
    fn reload(&mut self) {
        self.app.end_visual();
        self.app.stream = None;
        self.pipeline.loading = true;
        self.start_load();
    }

    /// Asks who wrote the shown worktree's files.
    fn request_attribution(&self) {
        let Some(tx) = &self.attribution else { return };
        let sources = self.app.attribution_sources();
        if !sources.is_empty() {
            let _ = tx.send(crate::attribution::Request {
                worktree: self.app.worktree_path().clone(),
                sources,
            });
        }
    }

    /// The terminal answered a colour query.
    fn colours(&mut self, d: Detected) -> bool {
        if Some(d) == self.theme.detected {
            return false;
        }
        self.theme.detected = Some(d);
        self.app.palette = Palette::new(d);
        self.theme.changed = true;
        true
    }
}

/// Listens for `ctl` commands. The viewer works without its socket.
fn serve_ctl(tx: &Sender<Msg>, worktree: &Path) -> (Arc<Mutex<ctl::Status>>, Option<ctl::Cleanup>) {
    let socket = ctl::socket_path();
    let status = Arc::new(Mutex::new(ctl::Status {
        block: std::env::var("REX_BLOCK").ok().filter(|b| !b.is_empty()),
        worktree: worktree.to_owned(),
    }));
    let tx = Mutex::new(tx.clone());
    let served = ctl::serve(&socket, Arc::clone(&status), move |c| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(Msg::Ctl(c));
        }
    });
    match served {
        Ok(()) => (status, Some(ctl::Cleanup(socket))),
        // The viewer works without its socket; `ctl` reports it missing.
        Err(_) => (status, None),
    }
}

/// Files are watched in every worktree where watches are cheap; on Linux
/// each directory costs an inotify watch, so only the selected one is.
const WATCH_ALL_FILES: bool = !cfg!(target_os = "linux");

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::termina::event::{
        KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::*;
    use crate::app::fixture::app_with;

    /// A runtime over a loaded app, drawn once at 100x30, with no threads.
    fn runtime(files: &[(&str, &str, &str)]) -> (Runtime, Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        let threads = Threads {
            highlight: 0,
            summaries: 0,
        };
        let mut rt = Runtime::new(app_with(files), tx, threads);
        draw(&mut rt);
        (rt, rx)
    }

    fn draw(rt: &mut Runtime) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
        terminal.draw(|f| ui::draw(f, &mut rt.app)).expect("draw");
        crate::shot::dump(terminal.backend().buffer(), false)
    }

    fn press(rt: &mut Runtime, keys: &str) {
        for c in keys.chars() {
            rt.key(KeyEvent::from(KeyCode::Char(c)));
        }
    }

    fn mouse(rt: &mut Runtime, kind: MouseEventKind, column: u16, row: u16) {
        rt.mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        });
    }

    const TWO: &[(&str, &str, &str)] = &[
        ("a.rs", "a\nb\nc\n", "a\nB\nc\nd\n"),
        ("b.rs", "x\n", "y\n"),
    ];

    fn agent(id: &str, block: &str) -> crate::agents::Agent {
        crate::agents::Agent {
            pid: 1,
            session_id: id.into(),
            session_file: None,
            session_name: Some(id.into()),
            rex_block: Some(block.into()),
            cwd: "/r".into(),
            socket: format!("/i/{id}.sock").into(),
        }
    }

    #[test]
    fn a_pick_is_not_replaced_by_a_resolution_started_before_it() {
        let (mut rt, _rx) = runtime(TWO);
        // A resolution in flight, as spawn_resolve numbers it.
        rt.delivery.resolve_seq += 1;
        let before = rt.delivery.resolve_seq;
        rt.app.mode = Mode::Picker(crate::picker::Picker::new(vec![agent("x", "b:2")], None));
        rt.key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(rt.app.session.state().agent.as_deref(), Some("x"));
        let bound = deliver::Resolution::Agent {
            agent: agent("bound", "b:1"),
            worker: None,
            why: deliver::Why::Bound,
        };
        rt.handle(Msg::Resolved {
            seq: before,
            worktree: "/r".into(),
            resolution: bound,
            agents: Vec::new(),
            problems: Vec::new(),
        });
        assert!(
            matches!(&rt.delivery.resolution, Some(deliver::Resolution::Agent { agent, .. }) if agent.session_id == "x"),
            "{}",
            rt.app.target
        );
    }

    #[test]
    fn a_worker_finishing_is_named_on_the_status_line_without_switching() {
        let (mut rt, _rx) = runtime(TWO);
        let snapshot = |status| crate::agents::Snapshot {
            agents: Vec::new(),
            workers: vec![crate::agents::Worker {
                id: "w-1".into(),
                label: "fix".into(),
                status,
                cwd: "/r/src".into(),
                model: None,
                owner_session: None,
                session_file: None,
                updated_at: 1,
            }],
            problems: Vec::new(),
        };
        rt.handle(Msg::Agents(snapshot(crate::agents::Status::Running)));
        assert_eq!(rt.app.message, None);
        assert!(draw(&mut rt).contains("fix running"));
        rt.handle(Msg::Agents(snapshot(crate::agents::Status::Done)));
        assert_eq!(rt.app.message.as_deref(), Some("fix finished in main"));
        assert_eq!(rt.app.current, 0);
        assert!(draw(&mut rt).contains("fix finished in main"));
    }

    #[test]
    fn showing_a_worktree_records_its_content_as_looked_at() {
        let dir = std::env::temp_dir().join(format!("dots-diff-looked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (mut rt, _rx) = runtime(TWO);
        rt.app
            .session
            .install(Path::new("/r"), Ok((dir.clone(), review::State::default())));
        let summary = |content: &str| crate::app::Summary {
            files: 2,
            content: Some(content.into()),
            ..crate::app::Summary::default()
        };
        rt.handle(Msg::Summary {
            path: "/r".into(),
            summary: Ok(summary("c1")),
        });
        rt.save_review();
        let saved = review::State::load(&dir).expect("load");
        assert_eq!(saved.looked.as_deref(), Some("c1"));
        // Another worktree's summary is not looked at.
        rt.handle(Msg::Summary {
            path: "/elsewhere".into(),
            summary: Ok(summary("c2")),
        });
        assert_eq!(rt.app.session.state().looked.as_deref(), Some("c1"));
        press(&mut rt, ".");
        assert_eq!(rt.app.message.as_deref(), Some("nothing waits for review"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_send_finishing_after_a_switch_is_recorded_in_its_worktree() {
        let root = std::env::temp_dir().join(format!("dots-diff-sent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (a, b) = (root.join("a"), root.join("b"));
        let (mut rt, _rx) = runtime(TWO);
        rt.app.session.install(
            Path::new("/wt/a"),
            Ok((a.clone(), review::State::default())),
        );
        let id = rt.app.session.edit().add(
            review::Place::Review,
            review::Kind::Comment,
            "x".into(),
            None,
        );
        rt.app
            .session
            .install(Path::new("/wt/b"), Ok((b, review::State::default())));
        let seen = std::collections::BTreeMap::from([("a.rs".to_owned(), "missing".to_owned())]);
        rt.on_sent(
            Path::new("/wt/a"),
            Some(&a),
            &[id],
            Ok(annotate::Delivered::test(seen.clone())),
        );
        let saved = review::State::load(&a).expect("load");
        assert_eq!(saved.drafts().count(), 0, "{saved:?}");
        assert_eq!(saved.seen, seen);
        assert!(
            rt.app.session.state().comments.is_empty(),
            "b's review is untouched"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_open_comment_keeps_its_worktree_until_it_is_saved() {
        let root = std::env::temp_dir().join(format!("dots-diff-editing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (mut rt, _rx) = runtime(TWO);
        rt.app.repo.worktrees.push(crate::repo::Worktree {
            path: "/r2".into(),
            branch: "other".into(),
            main: false,
            touched: None,
        });
        rt.app.session.install(
            Path::new("/r"),
            Ok((root.clone(), review::State::default())),
        );
        press(&mut rt, "jchello");
        assert!(rt.app.editor().is_some(), "{:?}", rt.app.message);
        // An agent asks the viewer to show another worktree.
        rt.command(Command::Worktree("/r2".into()));
        assert_eq!(rt.app.current, 0, "the switch waits for the comment");
        assert!(rt.app.editor().is_some(), "the comment stays open");
        assert!(
            rt.app
                .message
                .as_deref()
                .is_some_and(|m| m.contains("comment")),
            "{:?}",
            rt.app.message
        );
        ctrl(&mut rt, 's');
        let saved: Vec<_> = rt
            .app
            .session
            .state()
            .comments
            .iter()
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(saved, ["hello"], "saved in its own worktree's review");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_comment_that_cannot_be_saved_stays_open() {
        let root = std::env::temp_dir().join(format!("dots-diff-unsaved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (mut rt, _rx) = runtime(TWO);
        rt.app.session.install(
            Path::new("/r"),
            Ok((root.clone(), review::State::default())),
        );
        press(&mut rt, "jchello");
        assert!(rt.app.editor().is_some(), "{:?}", rt.app.message);
        // Another viewer holds the review the comment would be saved in.
        let editor = std::mem::take(&mut rt.app.mode);
        let other = review::Lock::take(&root.join("other"))
            .expect("lock")
            .expect("free");
        let message = rt.app.session.install(
            Path::new("/r"),
            Ok((root.join("other"), review::State::default())),
        );
        assert_eq!(message.as_deref(), Some(review::READ_ONLY));
        rt.app.mode = editor;
        ctrl(&mut rt, 's');
        assert!(rt.app.editor().is_some(), "the comment is kept");
        assert!(rt.app.session.state().comments.is_empty(), "nothing saved");
        assert!(rt.app.message.is_some(), "the refusal is reported");
        drop(other);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_comment_on_a_selection_ends_it_and_shows_the_editor_lines() {
        let (mut rt, _rx) = runtime(TWO);
        press(&mut rt, "jjvjc");
        let Some(editor) = rt.app.editor() else {
            panic!("no editor: {:?}", rt.app.message);
        };
        let review::Place::Lines { lines, .. } = &editor.place else {
            panic!("{:?}", editor.place);
        };
        assert_eq!(
            (lines.side, lines.start, lines.end),
            (review::Side::New, 1, 2)
        );
        assert_eq!(rt.app.visual(), None, "the editor replaced the selection");
        // The prefix waits for its second key, which the views never see.
        rt.key(KeyEvent::from(KeyCode::Escape));
        press(&mut rt, ";j");
        assert!(matches!(rt.app.mode, Mode::Normal));
        assert!(
            rt.app
                .message
                .as_deref()
                .is_some_and(|m| m.contains("; then c"))
        );
        press(&mut rt, ";c");
        let place = rt.app.editor().map(|e| e.place.clone());
        assert_eq!(place, Some(review::Place::Review));
    }

    #[test]
    fn the_mouse_does_nothing_under_the_editor_and_overlays() {
        let (mut rt, _rx) = runtime(TWO);
        let x = rt.app.areas.diff.x + 10;
        press(&mut rt, "j");
        press(&mut rt, "c");
        assert!(rt.app.editor().is_some(), "{:?}", rt.app.message);
        let row = rt.app.cursor_row;
        mouse(&mut rt, MouseEventKind::Drag(MouseButton::Left), x, 5);
        assert!(rt.app.editor().is_some());
        assert_eq!(rt.app.cursor_row, row);
        rt.key(KeyEvent::from(KeyCode::Escape));
        press(&mut rt, "?");
        mouse(&mut rt, MouseEventKind::Down(MouseButton::Left), x, 0);
        mouse(&mut rt, MouseEventKind::ScrollDown, x, 5);
        let stream = rt.app.stream.as_ref().expect("loaded");
        assert!(
            !stream.files[0].folded,
            "the click under help folded nothing"
        );
        assert_eq!(rt.app.global_top(), 0, "the view stayed under help");
    }

    fn ctrl(rt: &mut Runtime, c: char) {
        rt.key(KeyEvent::new(KeyCode::Char(c), Modifiers::CONTROL));
    }

    #[test]
    fn a_saved_comment_is_drawn_under_its_line_and_can_be_deleted() {
        let (mut rt, _rx) = runtime(TWO);
        press(&mut rt, "jjc");
        press(&mut rt, "looks off");
        ctrl(&mut rt, 's');
        assert!(matches!(rt.app.mode, Mode::Normal));
        assert_eq!(rt.app.session.state().drafts().count(), 1);
        assert!(rt.app.session.dirty(), "the draft waits to be saved");
        let frame = draw(&mut rt);
        let code = frame.find("+ B").expect("the commented line is drawn");
        let card = frame.find("looks off").expect("the card is drawn");
        assert!(code < card, "{frame}");
        // ( moves to the card from below; d deletes it.
        press(&mut rt, "G(d");
        draw(&mut rt);
        assert_eq!(rt.app.session.state().drafts().count(), 0);
        assert!(!draw(&mut rt).contains("looks off"));
    }

    #[test]
    fn an_abandoned_editor_leaves_no_draft() {
        let (mut rt, _rx) = runtime(TWO);
        press(&mut rt, "jjcnot this");
        rt.key(KeyEvent::from(KeyCode::Escape));
        assert!(matches!(rt.app.mode, Mode::Normal));
        assert_eq!(rt.app.session.state().drafts().count(), 0);
        assert!(!rt.app.session.dirty());
    }

    #[test]
    fn folding_a_file_hides_its_body_and_moves_the_next_one_up() {
        let (mut rt, _rx) = runtime(TWO);
        let before = draw(&mut rt);
        assert!(before.contains("+ B"), "{before}");
        press(&mut rt, "f");
        let after = draw(&mut rt);
        assert!(!after.contains("+ B"), "{after}");
        let row = |frame: &str| frame.lines().position(|l| l.contains("▾ ● b.rs"));
        assert!(row(&after) < row(&before), "{after}");
        press(&mut rt, "f");
        assert_eq!(draw(&mut rt), before);
    }

    #[test]
    fn help_and_the_switcher_open_over_the_diff_and_close_with_escape() {
        let (mut rt, _rx) = runtime(TWO);
        let plain = draw(&mut rt);
        press(&mut rt, "?");
        assert!(matches!(rt.app.mode, Mode::Help));
        let help = draw(&mut rt);
        assert!(help.contains("send all drafts to the agent"), "{help}");
        rt.key(KeyEvent::from(KeyCode::Escape));
        assert!(matches!(rt.app.mode, Mode::Normal));
        press(&mut rt, "w");
        assert!(matches!(rt.app.mode, Mode::Switcher(_)));
        assert!(draw(&mut rt).contains("main"));
        rt.key(KeyEvent::from(KeyCode::Escape));
        assert_eq!(draw(&mut rt), plain);
    }

    #[test]
    fn the_split_layout_shows_both_sides_on_one_row() {
        let (mut rt, _rx) = runtime(TWO);
        let one_row = |frame: &str| {
            frame
                .lines()
                .any(|l| l.contains("- b") && l.contains("+ B"))
        };
        // At 100 columns Auto is unified; L goes to unified, then split.
        assert!(!one_row(&draw(&mut rt)));
        press(&mut rt, "LL");
        let frame = draw(&mut rt);
        assert!(one_row(&frame), "{frame}");
    }
}
