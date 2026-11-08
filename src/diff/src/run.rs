//! The interactive viewer: one event loop owns the state; loading, diffing,
//! highlighting, watching and the control socket run on threads that send
//! it messages.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;

use crate::app::{
    self, App, Body, FileTexts, Focus, LayoutChoice, Line, ListItem, Scope, Settings, Summary,
};
use crate::ctl::{self, Command};
use crate::deliver;
use crate::graphics::Graphics;
use crate::highlight::{self, Key};
use crate::input::Input;
use crate::keys::{self, Action};
use crate::moves;
use crate::picker::Batch;
use crate::repo::{self, Changes, FileChange, GitState, Mode, Reader, Side, Stamp};
use crate::review;
use crate::stats::{self, Stats};
use crate::switcher::Switcher;
use crate::theme::{self, Detected, Palette};
use crate::timings::Timings;
use crate::ui;
use crate::watch::{self, Burst, Refresh, Touched, Watch};

mod annotate;

#[allow(clippy::large_enum_variant)]
pub(crate) enum Msg {
    Input(Event),
    Loaded {
        generation: u64,
        worktree: PathBuf,
        mode: Mode,
        result: Result<Changes, String>,
        /// The worktree's Git directory and saved review.
        review: Result<(PathBuf, review::State), String>,
        elapsed: Duration,
    },
    /// Who a review of `worktree` goes to, and the live sessions.
    Resolved {
        /// Numbers resolutions; only the latest one counts.
        seq: u64,
        worktree: PathBuf,
        resolution: deliver::Resolution,
        agents: Vec<deliver::Agent>,
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
        elapsed: Duration,
    },
    /// A path-limited rescan after file events.
    Partial {
        generation: u64,
        paths: Vec<String>,
        result: Result<Vec<FileChange>, String>,
        /// When the first event of the burst arrived.
        since: Instant,
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
    pub(crate) graphics: bool,
    #[allow(clippy::option_option)]
    pub(crate) timings: Option<Option<PathBuf>>,
    pub(crate) settings: Settings,
}

/// How often the safety net checks for changes the watcher missed.
const RECONCILE: Duration = Duration::from_secs(10);

/// Rex reports no theme change, so the colours are asked for again.
const THEME_POLL: Duration = Duration::from_secs(2);
/// Quiet time before another worktree's switcher counts are recomputed.
const SUMMARY_SETTLE: Duration = Duration::from_secs(3);
/// Threads computing switcher counts.
const SUMMARY_THREADS: usize = 3;
const PREFETCH: usize = 3;

struct DiffJob {
    cancel: Arc<AtomicBool>,
    focus: Arc<AtomicUsize>,
}

#[allow(clippy::struct_excessive_bools)]
struct Runtime {
    app: App,
    tx: Sender<Msg>,
    generation: u64,
    diffs: Option<DiffJob>,
    pool: highlight::Pool,
    watcher: Option<Watch>,
    summaries: SummaryQueue,
    /// Worktrees whose switcher counts are stale, and when they last changed.
    stale: HashMap<PathBuf, Instant>,
    /// File events waiting to be coalesced into one refresh.
    burst: Burst,
    /// A path-limited rescan is running; later bursts wait for it.
    partial_busy: bool,
    /// Changes with each new file list, so diffs of an older list are dropped.
    epoch: u64,
    /// A refresh waiting to reach the screen: when its burst began and its paths.
    refreshing: Option<(Instant, Vec<String>)>,
    next_reconcile: Instant,
    state: Option<GitState>,
    wanted: Vec<Key>,
    timings: Timings,
    /// When the current worktree began loading, until its first full frame.
    loading_since: Option<Instant>,
    theme_fixed: bool,
    detected: Option<Detected>,
    next_theme: Instant,
    quit: bool,
    summaries_started: bool,
    /// Where the current stream's contents are read from.
    source: Option<(gix::ThreadSafeRepository, PathBuf)>,
    /// The diffs the last moved-code search was started for, by fingerprint.
    moves_for: Option<u64>,
    /// Numbers moved-code searches; only the latest one's result is kept.
    moves_seq: u64,
    /// The worktree the file watch was set up around.
    watched: Option<usize>,
    /// What the control socket reports.
    ctl_status: Arc<Mutex<ctl::Status>>,
    /// A file to scroll to once the worktree loading now has loaded.
    pending_file: Option<String>,
    /// The Rex pane reviews go to while its worktree is shown.
    bound: Option<(String, usize)>,
    /// Who a review goes to, once resolved, and the live sessions.
    resolution: Option<deliver::Resolution>,
    /// Numbers resolutions and picks, so an older result never replaces a newer one.
    resolve_seq: u64,
    agents: Vec<deliver::Agent>,
    /// A review is being delivered.
    sending: bool,
    /// `;` was pressed; the next key completes the command.
    prefix: bool,
    /// Save the next frame as a PNG here.
    shot: Option<PathBuf>,
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run(options: Options) -> Result<()> {
    let mut timings = Timings::new(options.timings.is_some());
    let started = Instant::now();
    let repo = repo::discover(&options.path)?;
    timings.span(
        "enumerate worktrees",
        started.elapsed(),
        &format!("{} worktrees", repo.worktrees.len()),
    );
    let current = repo.index_of(&options.path).unwrap_or(0);

    let mut terminal = ratatui::try_init().context("starting the terminal")?;
    let result = (|| {
        ratatui::crossterm::execute!(
            std::io::stdout(),
            ratatui::crossterm::event::EnableMouseCapture,
            ratatui::crossterm::event::EnableBracketedPaste,
            // Tells Ctrl+Enter from Enter where the terminal can.
            ratatui::crossterm::event::PushKeyboardEnhancementFlags(
                ratatui::crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        )?;
        let detected = match options.theme {
            Some(d) => Some(d),
            None => theme::ask(Duration::from_millis(400)).ok().flatten(),
        };
        timings.note(&format!(
            "theme {}",
            if detected.is_some() {
                "reported"
            } else {
                "not reported; light fallback"
            }
        ));
        let palette = Palette::new(detected.unwrap_or_else(Detected::light));
        let mut settings = options.settings.clone();
        if options.graphics {
            settings.header_rows = 2;
        }
        let app = App::new(repo, current, palette, settings);
        let (tx, rx) = mpsc::channel();
        let input = Input::spawn(tx.clone(), Msg::Input);
        let (ctl_status, _cleanup) = serve_ctl(&tx, app.worktree_path(), &mut timings);
        let mut rt = Runtime::new(app, tx, Threads::default());
        rt.ctl_status = ctl_status;
        rt.timings = timings;
        rt.loading_since = Some(started);
        rt.theme_fixed = options.theme.is_some();
        rt.detected = detected;
        rt.pending_file.clone_from(&options.file);
        rt.bound = options.bind.clone().map(|b| (b, current));
        let mut graphics = options.graphics.then(Graphics::new);
        rt.start_load();
        rt.spawn_watch();
        let outcome = rt.event_loop(&mut terminal, &rx, &input, graphics.as_mut());
        if let Some(g) = graphics.as_mut() {
            g.clear();
        }
        outcome.map(|()| rt.timings)
    })();
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::PopKeyboardEnhancementFlags,
        ratatui::crossterm::event::DisableBracketedPaste,
        ratatui::crossterm::event::DisableMouseCapture
    );
    ratatui::try_restore()?;
    let timings = result?;
    if let Some(dest) = options.timings {
        timings.write(started, dest.as_deref())?;
    }
    Ok(())
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
            generation: 0,
            diffs: None,
            pool,
            watcher: None,
            stale: HashMap::new(),
            burst: Burst::default(),
            partial_busy: false,
            epoch: 0,
            refreshing: None,
            next_reconcile: Instant::now() + RECONCILE,
            state: None,
            wanted: Vec::new(),
            timings: Timings::new(false),
            loading_since: None,
            theme_fixed: false,
            detected: None,
            next_theme: Instant::now() + THEME_POLL,
            quit: false,
            summaries_started: false,
            source: None,
            moves_for: None,
            moves_seq: 0,
            watched: None,
            ctl_status: Arc::new(Mutex::new(ctl::Status {
                block: None,
                worktree,
            })),
            pending_file: None,
            bound: None,
            resolution: None,
            resolve_seq: 0,
            agents: Vec::new(),
            sending: false,
            prefix: false,
            shot: None,
        }
    }

    fn view_rows(&self) -> u16 {
        self.app.areas.diff.height.max(1)
    }

    #[allow(clippy::too_many_lines)]
    fn event_loop(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
        rx: &Receiver<Msg>,
        input: &Input,
        mut graphics: Option<&mut Graphics>,
    ) -> Result<()> {
        let mut dirty = true;
        let mut frames = 0u32;
        while !self.quit {
            if dirty {
                let t = Instant::now();
                let completed = terminal.draw(|f| ui::draw(f, &mut self.app))?;
                if let Some(path) = self.shot.take()
                    && let Err(e) = crate::render::picture(completed.buffer, &path)
                {
                    self.app.message = Some(format!("{e:#}"));
                }
                self.app.mark_notes_seen(self.view_rows());
                if let Some(g) = graphics.as_deref_mut() {
                    g.after_draw(&self.app)?;
                }
                frames += 1;
                if frames == 1 {
                    self.timings
                        .span("first frame", self.timings.since_start(), "");
                }
                self.after_frame(t.elapsed());
                dirty = false;
            }
            let now = Instant::now();
            let mut deadline = now + Duration::from_secs(60);
            if !self.theme_fixed {
                deadline = deadline.min(self.next_theme);
            }
            if let Some(at) = self.stale.values().min() {
                deadline = deadline.min(*at + SUMMARY_SETTLE);
            }
            if graphics.as_deref().is_some_and(Graphics::pending) {
                deadline = deadline.min(now + Duration::from_millis(50));
                dirty = true;
            }
            if !self.partial_busy
                && let Some(at) = self.burst.due()
            {
                deadline = deadline.min(at);
            }
            if let Some(at) = self.app.flash_deadline() {
                deadline = deadline.min(at);
            }
            deadline = deadline.min(self.next_reconcile);
            match rx.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(msg) => dirty |= self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(msg) = rx.try_recv() {
                dirty |= self.handle(msg);
            }
            self.refresh_moves();
            if self.app.review_dirty {
                self.save_review();
            }
            let now = Instant::now();
            if !self.partial_busy {
                match self.burst.take(now) {
                    Some(Refresh::Full) => self.start_load(),
                    Some(Refresh::Paths(paths)) => self.start_partial(paths, now),
                    None => {}
                }
            }
            if self.app.flash_deadline().is_some_and(|at| at <= now) {
                self.app.expire_flashes(now);
                dirty = true;
            }
            if self.next_reconcile <= now {
                self.next_reconcile = now + RECONCILE;
                self.spawn_reconcile();
                self.spawn_resolve();
            }
            if !self.theme_fixed && self.next_theme <= now {
                self.next_theme = now + THEME_POLL;
                let paused = input.pause();
                let asked = theme::ask(Duration::from_millis(150)).ok().flatten();
                drop(paused);
                if let Some(d) = asked
                    && Some(d) != self.detected
                {
                    self.detected = Some(d);
                    self.app.palette = Palette::new(d);
                    if let Some(g) = graphics.as_deref_mut() {
                        g.invalidate();
                    }
                    // Repaint everything: the buffer diff cannot know every cell changed.
                    terminal.clear()?;
                    dirty = true;
                }
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
        }
        Ok(())
    }

    fn after_frame(&mut self, draw: Duration) {
        if let Some(since) = self.loading_since
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
                self.loading_since = None;
                if !self.summaries_started {
                    // Rail counts wait for the first full frame so they never delay it.
                    self.summaries_started = true;
                    self.spawn_summaries();
                }
                self.timings.span(
                    "worktree shown with diffs",
                    since.elapsed(),
                    &format!("draw {draw:.1?}"),
                );
            }
        }
        if let Some((since, paths)) = &self.refreshing
            && let Some(stream) = &self.app.stream
        {
            let done = stream
                .files
                .iter()
                .filter(|f| paths.contains(&f.change.path))
                .all(|f| !matches!(f.body, Body::Pending));
            if done {
                self.timings.span(
                    "file event to screen",
                    since.elapsed(),
                    &format!("{} paths, draw {draw:.1?}", paths.len()),
                );
                self.refreshing = None;
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
        if let Some(job) = &self.diffs {
            job.focus.store(self.app.top.file, Ordering::Relaxed);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Input(event) => self.input(&event),
            Msg::Loaded {
                generation,
                worktree,
                mode,
                result,
                review,
                elapsed,
            } => {
                if generation != self.generation {
                    return false;
                }
                match result {
                    Ok(changes) => {
                        self.install_review(&worktree, review);
                        self.timings.span(
                            "load changes",
                            elapsed,
                            &format!("{} files", changes.files.len()),
                        );
                        self.app.message = None;
                        let reader =
                            Reader::new(changes.repo.to_thread_local(), changes.root.clone());
                        self.state = Some(changes.state);
                        self.source = Some((changes.repo, changes.root));
                        self.app.load(
                            worktree.clone(),
                            mode,
                            changes.base,
                            changes.target,
                            changes.files,
                            Some(reader),
                        );
                        self.app.refresh_marks();
                        self.app.cards_changed();
                        self.epoch += 1;
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
                if epoch != self.epoch || seq != self.moves_seq {
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
                elapsed,
            } => {
                if epoch != self.epoch {
                    return false;
                }
                let lines = match &body {
                    Body::Text(d) => d.old_len + d.new_len,
                    _ => 0,
                };
                self.timings.diff(elapsed, lines);
                self.app.set_body(file, body, texts);
                self.update_own_summary();
                true
            }
            Msg::Partial {
                generation,
                paths,
                result,
                since,
            } => {
                self.partial_busy = false;
                if generation != self.generation {
                    return false;
                }
                match result {
                    Ok(files) => {
                        self.app.apply_paths(&paths, files);
                        self.epoch += 1;
                        self.refreshing = Some((since, paths));
                        self.spawn_diffs();
                    }
                    // The paths could not be scanned alone; scan everything.
                    Err(_) => self.burst.push_full(Instant::now()),
                }
                true
            }
            Msg::Reconciled {
                generation,
                state,
                moved,
            } => {
                if generation != self.generation {
                    return false;
                }
                if state.is_some() && state != self.state {
                    // The full scan records the new state.
                    self.burst.push_full(Instant::now());
                }
                if !moved.is_empty() {
                    self.burst.push(Instant::now(), moved);
                }
                false
            }
            Msg::Highlighted(done) => {
                self.timings.highlight(done.elapsed);
                let result = done.result.ok();
                let size = result.as_ref().map_or(0, |l| l.bytes());
                self.app.highlights.insert(done.key, result, size);
                true
            }
            Msg::Summary { path, summary } => {
                if let Ok(s) = summary {
                    self.app.summaries.insert(path, s);
                }
                self.app.switcher.is_some()
            }
            Msg::Touched(touched) => {
                let current = self.app.current;
                let now = Instant::now();
                match touched {
                    Touched::Files { worktree, paths } if worktree == current => {
                        self.burst.push(now, paths);
                    }
                    Touched::GitState(i) if i == current => self.burst.push_full(now),
                    Touched::Files { worktree: i, .. } | Touched::GitState(i) => {
                        if let Some(w) = self.app.repo.worktrees.get(i) {
                            self.stale.insert(w.path.clone(), now);
                        }
                    }
                    Touched::Refs => {
                        // Any worktree's base may have moved.
                        self.burst.push_full(now);
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
                if seq != self.resolve_seq || worktree != *self.app.worktree_path() {
                    return false;
                }
                if let Some(problem) = problems.first() {
                    self.app.message = Some(format!("unreadable Pi registration {problem}"));
                }
                self.agents = agents;
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
                if let Some(i) = self.app.repo.index_of(&path) {
                    self.switch(i);
                } else {
                    match repo::discover(&path) {
                        Ok(repo) => {
                            let i = repo.index_of(&path).unwrap_or(0);
                            self.app.repo = repo;
                            self.app.summaries.clear();
                            self.app.current = usize::MAX;
                            self.summaries.reset();
                            self.watched = None;
                            self.switch(i);
                            self.spawn_summaries();
                            self.spawn_watch();
                        }
                        Err(e) => self.app.message = Some(format!("{e:#}")),
                    }
                }
            }
            Command::Status => {}
            Command::Note(spec) => self.note(spec),
            Command::ClearNotes(path) => {
                let rel = path.map(|p| self.relative(&p).1);
                self.app.clear_notes(rel.as_deref());
            }
            Command::Bind(block) => {
                self.bound = Some((block, self.app.current));
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
                    self.pending_file = Some(path);
                    self.switch(i);
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
                let found = self.app.stream.as_ref().and_then(|s| {
                    s.files
                        .iter()
                        .position(|f| f.change.path == rel)
                        .or_else(|| s.files.iter().position(|f| f.change.path.ends_with(&rel)))
                });
                match found {
                    Some(i) => {
                        let rows = self.view_rows();
                        self.app.jump_to_file(i, rows);
                    }
                    None => self.app.message = Some(format!("{rel} has no changes here")),
                }
            }
        }
    }

    fn switch(&mut self, worktree: usize) {
        if worktree == self.app.current || worktree >= self.app.repo.worktrees.len() {
            return;
        }
        self.app.current = worktree;
        self.app.stream = None;
        self.loading_since = Some(Instant::now());
        self.start_load();
        if !WATCH_ALL_FILES {
            self.spawn_watch();
        }
    }

    fn start_load(&mut self) {
        self.generation += 1;
        if let Ok(mut status) = self.ctl_status.lock() {
            status.worktree.clone_from(self.app.worktree_path());
        }
        if let Some(job) = self.diffs.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.burst = Burst::default();
        self.state = None;
        let generation = self.generation;
        let worktree = self.app.worktree_path().clone();
        let mode = self.app.settings.mode;
        let since = self.app.since;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut review = Err(String::new());
            let result = repo::changes(&worktree, mode)
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
            let _ = tx.send(Msg::Loaded {
                generation,
                worktree,
                mode,
                result,
                review,
                elapsed: started.elapsed(),
            });
        });
    }

    /// Rescans `paths` alone, off the event loop.
    fn start_partial(&mut self, paths: Vec<String>, since: Instant) {
        let (Some((repo, root)), Some(stream)) = (self.source.clone(), &self.app.stream) else {
            return;
        };
        let listed: Vec<&str> = stream
            .files
            .iter()
            .map(|f| f.change.path.as_str())
            .collect();
        if watch::needs_full_scan(&root, &paths, &listed) {
            self.start_load();
            return;
        }
        let target = stream.target;
        let renames: std::collections::HashMap<String, String> = stream
            .files
            .iter()
            .filter_map(|f| Some((f.change.path.clone(), f.change.from.clone()?)))
            .filter(|(path, _)| paths.contains(path))
            .collect();
        let generation = self.generation;
        let tx = self.tx.clone();
        let seen = self.app.since.then(|| self.app.review.seen.clone());
        self.partial_busy = true;
        std::thread::spawn(move || {
            let repo = repo.to_thread_local();
            let result = repo::changes_for(&repo, target, &paths, &renames)
                .map(|files| match &seen {
                    Some(seen) => review::since_sent(
                        files,
                        seen,
                        |p| paths.iter().any(|q| q == p),
                        target,
                        &mut Reader::new(repo, root),
                    ),
                    None => files,
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Partial {
                generation,
                paths,
                result,
                since,
            });
        });
    }

    /// The safety net behind the watcher: compares HEAD, the index and the
    /// base with the last check, and restats working-tree files.
    fn spawn_reconcile(&self) {
        let Some(stream) = &self.app.stream else {
            return;
        };
        let worktree = stream.worktree.clone();
        let state = self.state.clone();
        let files: Vec<(String, Stamp)> = stream
            .files
            .iter()
            .filter_map(|f| match f.change.new {
                Side::Worktree(stamp) => Some((f.change.path.clone(), stamp)),
                _ => None,
            })
            .collect();
        let generation = self.generation;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let state = state.map(|s| s.restat());
            let moved = files
                .into_iter()
                .filter(|(path, stamp)| {
                    let now = std::fs::symlink_metadata(worktree.join(path))
                        .ok()
                        .map(|m| Stamp {
                            modified: m.modified().ok(),
                            len: m.len(),
                        });
                    now != Some(*stamp)
                })
                .map(|(path, _)| path)
                .collect();
            let _ = tx.send(Msg::Reconciled {
                generation,
                state,
                moved,
            });
        });
    }

    /// Starts watching the worktrees; setting up takes a moment, so it
    /// happens off the event loop.
    fn spawn_watch(&mut self) {
        if self.watched == Some(self.app.current) {
            return;
        }
        self.watched = Some(self.app.current);
        self.watcher = None;
        let roots: Vec<PathBuf> = self
            .app
            .repo
            .worktrees
            .iter()
            .map(|w| w.path.clone())
            .collect();
        let selected = self.app.current;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let events = Mutex::new(tx.clone());
            let result = watch::watch(&roots, selected, WATCH_ALL_FILES, move |t| {
                if let Ok(tx) = events.lock() {
                    let _ = tx.send(Msg::Touched(t));
                }
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Watching(result));
        });
    }

    /// Starts a moved-code search when moves are shown and the diffs
    /// differ from those the last search was started for.
    fn refresh_moves(&mut self) {
        if !self.app.show_moves {
            return;
        }
        let Some(print) = self.app.move_fingerprint() else {
            return;
        };
        if self.moves_for == Some(print) {
            return;
        }
        let (Some(inputs), Some((repo, root))) = (self.app.move_inputs(), self.source.clone())
        else {
            return;
        };
        self.moves_for = Some(print);
        self.moves_seq += 1;
        let (tx, epoch, seq) = (self.tx.clone(), self.epoch, self.moves_seq);
        std::thread::spawn(move || {
            let mut reader = Reader::new(repo.to_thread_local(), root);
            let moves = Arc::new(moves::search(&mut reader, &inputs));
            let _ = tx.send(Msg::Moves { epoch, seq, moves });
        });
    }

    /// Diffs pending files on several threads, nearest to the view first.
    fn spawn_diffs(&mut self) {
        let pending = self.app.pending();
        if pending.is_empty() {
            return;
        }
        if let Some(job) = self.diffs.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        let Some(stream) = &self.app.stream else {
            return;
        };
        let Some((repo, root)) = self.source.clone() else {
            return;
        };
        let todo: Vec<Option<FileChange>> = (0..stream.files.len())
            .map(|i| {
                pending
                    .binary_search(&i)
                    .ok()
                    .map(|_| stream.files[i].change.clone())
            })
            .collect();
        let todo = Arc::new(Mutex::new(todo));
        let job = DiffJob {
            cancel: Arc::new(AtomicBool::new(false)),
            focus: Arc::new(AtomicUsize::new(self.app.top.file)),
        };
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4));
        for _ in 0..threads {
            let (repo, root) = (repo.clone(), root.clone());
            let (todo, cancel, focus, tx, epoch) = (
                Arc::clone(&todo),
                Arc::clone(&job.cancel),
                Arc::clone(&job.focus),
                self.tx.clone(),
                self.epoch,
            );
            std::thread::spawn(move || {
                let mut reader = Reader::new(repo.to_thread_local(), root);
                while !cancel.load(Ordering::Relaxed) {
                    let next = {
                        let Ok(mut todo) = todo.lock() else { return };
                        nearest(&todo, focus.load(Ordering::Relaxed))
                            .and_then(|i| todo[i].take().map(|c| (i, c)))
                    };
                    let Some((file, change)) = next else { return };
                    let started = Instant::now();
                    let (body, texts) = app::compute_body(&mut reader, &change);
                    if tx
                        .send(Msg::Diffed {
                            epoch,
                            file,
                            body,
                            texts,
                            elapsed: started.elapsed(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            });
        }
        self.diffs = Some(job);
    }

    /// Queues every worktree's switcher counts, those listed after the
    /// current one first.
    fn spawn_summaries(&self) {
        let n = self.app.repo.worktrees.len();
        let at = self.app.current.min(n.saturating_sub(1));
        let order = (0..n).map(|d| (at + d) % n);
        for i in order {
            self.summaries
                .push(self.app.repo.worktrees[i].path.clone(), false);
        }
    }

    /// The selected worktree's switcher entry follows its own stream once complete.
    fn update_own_summary(&mut self) {
        let Some(stream) = &self.app.stream else {
            return;
        };
        if stream.mode != Mode::Base {
            return;
        }
        let (added, removed, done) = stream.counts();
        if done {
            let summary = Summary {
                added,
                removed,
                files: stream.files.len(),
            };
            self.app.summaries.insert(stream.worktree.clone(), summary);
        }
    }

    fn input(&mut self, event: &Event) -> bool {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                self.key(*key);
                true
            }
            Event::Mouse(mouse) => self.mouse(*mouse),
            Event::Paste(text) => {
                if let Some(editor) = &mut self.app.editor {
                    editor.buffer.insert(text);
                }
                true
            }
            Event::Resize(..) | Event::FocusGained => true,
            _ => false,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn key(&mut self, key: KeyEvent) {
        // Terminals reporting keys unambiguously send Shift+c rather than C.
        let key = match key.code {
            KeyCode::Char(c)
                if key.modifiers.contains(KeyModifiers::SHIFT) && c.is_ascii_lowercase() =>
            {
                KeyEvent {
                    code: KeyCode::Char(c.to_ascii_uppercase()),
                    ..key
                }
            }
            _ => key,
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        self.app.message = None;
        if self.app.editor.is_some() {
            self.editor_key(key);
            return;
        }
        // Ctrl-C quits from anywhere else, overlays included.
        if ctrl && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.app.help {
            self.app.help = false;
            return;
        }
        if self.app.switcher.is_some() {
            self.switcher_key(key, ctrl);
            return;
        }
        if self.app.picker.is_some() {
            self.picker_key(key, ctrl);
            return;
        }
        if std::mem::take(&mut self.prefix) {
            match key.code {
                KeyCode::Char('c') => self.app.begin_comment(Scope::Review),
                _ => self.app.message = Some("; then c comments on the whole review".to_owned()),
            }
            return;
        }
        let Some(action) = keys::action(key.code, ctrl) else {
            return;
        };
        if action.needs_diff() && self.app.focus != Focus::Diff {
            self.app.message = Some("press tab to focus the diff first".to_owned());
            return;
        }
        self.act(action);
    }

    /// Does what a key is bound to.
    #[allow(clippy::too_many_lines)]
    fn act(&mut self, action: Action) {
        let rows = self.view_rows();
        let half = i64::from(rows / 2);
        let page = i64::from(rows.saturating_sub(2));
        let diff = self.app.focus == Focus::Diff;
        match action {
            Action::Quit => self.quit = true,
            Action::Help => self.app.help = true,
            Action::Down => self.move_by(1),
            Action::Up => self.move_by(-1),
            Action::HalfDown => self.scroll(half),
            Action::HalfUp => self.scroll(-half),
            Action::Space if !diff && self.app.toggle_dir(self.app.cursor) => {}
            Action::Space | Action::PageDown => self.scroll(page),
            Action::PageUp => self.scroll(-page),
            Action::Top if diff => self.app.move_cursor(i64::MIN / 2, rows),
            Action::Bottom if diff => self.app.move_cursor(i64::MAX / 2, rows),
            Action::Top => {
                self.app.scroll_to(0, rows);
                self.app.sync_files_cursor();
            }
            Action::Bottom => {
                self.app.scroll_to(u32::MAX, rows);
                self.app.sync_files_cursor();
            }
            Action::NextFile | Action::PrevFile => {
                self.app.next_file(action == Action::NextFile, rows);
                if diff {
                    self.app.cursor_row = 0;
                    self.app.clamp_cursor(rows);
                }
            }
            Action::NextChange | Action::PrevChange => {
                self.app.next_hunk(action == Action::NextChange, rows);
                if diff {
                    self.cursor_to_change(rows);
                }
            }
            Action::Comment => self.app.begin_comment(Scope::Lines),
            Action::Suggest => self.app.begin_comment(Scope::Suggestion),
            Action::Visual => self.app.toggle_visual(),
            Action::Cancel => self.app.visual = None,
            Action::FileComment => self.app.begin_comment(Scope::File),
            Action::Prefix => self.prefix = true,
            Action::Enter if !diff => {
                if !self.app.toggle_dir(self.app.cursor) {
                    self.app.select_row(self.app.cursor, rows);
                    self.app.focus = Focus::Diff;
                }
            }
            Action::Enter | Action::Expand => {
                if !diff {
                    self.app.expand_visible_gap(rows);
                } else if let Some(card) = self.app.cursor_card() {
                    self.app.edit_card(card);
                } else if let Line::Header { file, .. } = self.app.line_at(self.app.cursor_line()) {
                    self.app.toggle_fold(file);
                } else if let Line::Body { file, row, .. } =
                    self.app.line_at(self.app.cursor_line())
                    && self.app.expand_at(file, row)
                {
                } else {
                    self.app.expand_visible_gap(rows);
                }
            }
            Action::ExpandAll => self.app.toggle_expand_all(),
            Action::Fold => {
                let file = if diff { self.app.cursor_file() } else { None };
                self.app.toggle_fold(file.unwrap_or(self.app.top.file));
            }
            Action::FoldAll => self.app.toggle_fold_all(),
            Action::Delete => match self.app.cursor_card() {
                Some(card) => self.app.delete_card(card),
                None => self.app.message = Some("move onto a card to delete it".to_owned()),
            },
            Action::NextCard => self.app.next_card(true, rows),
            Action::PrevCard => self.app.next_card(false, rows),
            Action::OldSide => self.app.cursor_old = !self.app.cursor_old,
            Action::Send => self.send(Batch::All),
            Action::PickAgent => self.open_picker(None),
            Action::Moves => {
                self.app.show_moves = !self.app.show_moves;
                self.app.message = Some(
                    if self.app.show_moves {
                        "showing moved code"
                    } else {
                        "moved code hidden"
                    }
                    .to_owned(),
                );
                self.refresh_moves();
            }
            Action::Since => {
                self.app.since = !self.app.since;
                self.app.visual = None;
                self.app.stream = None;
                self.loading_since = Some(Instant::now());
                self.start_load();
            }
            Action::Reviewed => self.app.toggle_reviewed(),
            Action::Switcher => {
                self.app.switcher = Some(Switcher::new(&self.app.repo, self.app.current));
            }
            Action::NextWorktree => {
                self.switch((self.app.current + 1) % self.app.repo.worktrees.len());
            }
            Action::PrevWorktree => {
                let n = self.app.repo.worktrees.len();
                self.switch((self.app.current + n - 1) % n);
            }
            Action::Focus => {
                self.app.focus = if diff && self.app.areas.files.width > 0 {
                    Focus::Files
                } else {
                    Focus::Diff
                };
            }
            Action::Left if !diff => self.tree_left(),
            Action::Left => {
                if self.app.areas.files.width > 0 {
                    self.app.focus = Focus::Files;
                }
            }
            Action::Right if !diff => self.tree_right(),
            Action::Right => {}
            Action::Layout => {
                self.app.settings.layout = match self.app.settings.layout {
                    LayoutChoice::Auto => LayoutChoice::Unified,
                    LayoutChoice::Unified => LayoutChoice::Split,
                    LayoutChoice::Split => LayoutChoice::Auto,
                };
            }
            Action::Wrap => {
                self.app.settings.wrap = !self.app.settings.wrap;
                self.app.relayout();
            }
            Action::Compare => {
                self.app.settings.mode = match self.app.settings.mode {
                    Mode::Base => Mode::Head,
                    Mode::Head => Mode::Base,
                };
                self.app.stream = None;
                self.loading_since = Some(Instant::now());
                self.start_load();
            }
            Action::Sidebar => {
                self.app.settings.sidebar = !self.app.settings.sidebar;
                if !self.app.settings.sidebar {
                    self.app.focus = Focus::Diff;
                }
            }
            Action::Narrower | Action::Wider => {
                let now = self.app.areas.files.width;
                let width = if action == Action::Narrower {
                    now.saturating_sub(4)
                } else {
                    now + 4
                };
                self.app.settings.sidebar = true;
                self.app.settings.sidebar_width = Some(width.clamp(16, 64));
            }
            Action::Refresh => self.start_load(),
        }
    }

    /// Keys while the worktree switcher is open: typing filters it.
    fn switcher_key(&mut self, key: KeyEvent, ctrl: bool) {
        let Some(sw) = self.app.switcher.as_mut() else {
            return;
        };
        let repo = &self.app.repo;
        match (key.code, ctrl) {
            (KeyCode::Esc, _) | (KeyCode::Char('c' | 'g'), true) => self.app.switcher = None,
            (KeyCode::Enter, _) => {
                let chosen = sw.selected();
                self.app.switcher = None;
                if let Some(i) = chosen {
                    self.switch(i);
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('n' | 'j'), true) => sw.move_by(1),
            (KeyCode::Up, _) | (KeyCode::Char('p' | 'k'), true) => sw.move_by(-1),
            (KeyCode::Backspace, _) => sw.pop(repo),
            (KeyCode::Char('u'), true) => {
                while !sw.query.is_empty() {
                    sw.pop(repo);
                }
            }
            (KeyCode::Char(c), false) => sw.push(repo, c),
            _ => {}
        }
    }

    /// Scrolls the view; the cursor keeps its place on screen.
    fn scroll(&mut self, delta: i64) {
        let rows = self.view_rows();
        self.app.scroll_by(delta, rows);
        self.app.clamp_cursor(rows);
        self.app.sync_files_cursor();
    }

    fn move_by(&mut self, delta: i64) {
        match self.app.focus {
            Focus::Diff => self.app.move_cursor(delta, self.view_rows()),
            Focus::Files => {
                let row = step(self.app.cursor, delta, self.app.list.len());
                self.app.select_row(row, self.view_rows());
            }
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) -> bool {
        let modal = self.app.switcher.is_some()
            || self.app.editor.is_some()
            || self.app.picker.is_some()
            || self.app.help
            || self.prefix;
        if modal {
            return false;
        }
        let at = Position::new(mouse.column, mouse.row);
        let areas = self.app.areas;
        let rows = self.view_rows();
        if let MouseEventKind::Drag(MouseButton::Left) = mouse.kind {
            // Dragging in the diff selects lines, from where the press was.
            if self.app.focus != Focus::Diff || !areas.diff.contains(at) {
                return false;
            }
            if self.app.visual.is_none() {
                self.app.visual = self.app.cursor_pos();
            }
            self.app.cursor_row = mouse.row - areas.diff.y;
            self.app.clamp_cursor(rows);
            return true;
        }
        let delta = match mouse.kind {
            MouseEventKind::ScrollDown => 3,
            MouseEventKind::ScrollUp => -3,
            MouseEventKind::Down(MouseButton::Left) => 0,
            _ => return false,
        };
        if areas.diff.contains(at) {
            if delta != 0 {
                self.scroll(delta);
            } else {
                self.app.focus = Focus::Diff;
                self.app.visual = None;
                let y = mouse.row - areas.diff.y;
                let pinned = self
                    .app
                    .pinned_header()
                    .filter(|_| y < self.app.settings.header_rows);
                if let Some(file) = pinned {
                    self.app.toggle_fold(file);
                    return true;
                }
                if let Line::Header { file, .. } =
                    self.app.line_at(self.app.global_top() + u32::from(y))
                {
                    self.app.toggle_fold(file);
                    return true;
                }
                self.app.cursor_row = mouse.row - areas.diff.y;
                self.app.clamp_cursor(rows);
                if let Line::Body { file, row, .. } = self.app.line_at(self.app.cursor_line()) {
                    self.app.expand_at(file, row);
                }
            }
        } else if areas.files.contains(at) {
            if delta != 0 {
                self.app.focus = Focus::Files;
                self.move_by(delta.signum());
            } else if let Some(row) = self.row_at(mouse.row) {
                self.app.focus = Focus::Files;
                if !self.app.toggle_dir(row) {
                    self.app.select_row(row, rows);
                }
            }
        }
        true
    }

    /// The sidebar row at screen row `y`, below the sidebar's branch line.
    fn row_at(&self, y: u16) -> Option<usize> {
        let area = self.app.areas.files;
        let rows = usize::from(area.height.saturating_sub(1));
        let start = self.app.list_start(rows);
        let slot = usize::from(y.checked_sub(area.y + 1)?);
        let row = start + slot;
        (row < self.app.list.len()).then_some(row)
    }

    /// Collapses the selected directory, or moves to the one holding the
    /// selected row.
    fn tree_left(&mut self) {
        let row = self.app.cursor;
        match self.app.list.get(row) {
            Some(ListItem::Dir { path, .. }) if !self.app.collapsed.contains(path) => {
                let path = path.clone();
                self.app.set_collapsed(&path, true);
            }
            Some(_) => {
                if let Some(parent) = self.app.parent_row(row) {
                    self.app.select_row(parent, self.view_rows());
                }
            }
            None => {}
        }
    }

    /// Expands the selected directory or steps into it; on a file, moves
    /// focus to the diff.
    fn tree_right(&mut self) {
        let row = self.app.cursor;
        match self.app.list.get(row) {
            Some(ListItem::Dir { path, .. }) if self.app.collapsed.contains(path) => {
                let path = path.clone();
                self.app.set_collapsed(&path, false);
            }
            Some(ListItem::Dir { .. }) => self.app.select_row(row + 1, self.view_rows()),
            _ => self.app.focus = Focus::Diff,
        }
    }
}

/// The repository's line-count caches, opened by the first summary thread
/// that needs them so loading never delays the first frame.
type SharedStats = Arc<std::sync::OnceLock<Stats>>;
type Queue = Arc<(Mutex<VecDeque<(PathBuf, SharedStats)>>, Condvar)>;

/// Background threads computing switcher counts, one worktree at a time each.
struct SummaryQueue {
    queue: Queue,
    stats: SharedStats,
}

impl SummaryQueue {
    fn new(threads: usize, tx: &Sender<Msg>) -> Self {
        let queue: Queue = Arc::default();
        for _ in 0..threads {
            let (queue, tx) = (Arc::clone(&queue), tx.clone());
            std::thread::spawn(move || {
                loop {
                    let (path, shared) = {
                        let (lock, cv) = &*queue;
                        let Ok(mut q) = lock.lock() else { return };
                        loop {
                            if let Some(p) = q.pop_front() {
                                break p;
                            }
                            let Ok(next) = cv.wait(q) else { return };
                            q = next;
                        }
                    };
                    let stats = shared.get_or_init(|| Stats::open(&path));
                    let summary = stats::summarize(stats, &path).map_err(|e| format!("{e:#}"));
                    if tx.send(Msg::Summary { path, summary }).is_err() {
                        return;
                    }
                }
            });
        }
        Self {
            queue,
            stats: SharedStats::default(),
        }
    }

    /// Starts over with another repository's caches.
    fn reset(&mut self) {
        if let Ok(mut q) = self.queue.0.lock() {
            q.clear();
        }
        self.stats = SharedStats::default();
    }

    /// Queues `path` unless it is already waiting; `urgent` puts it first.
    fn push(&self, path: PathBuf, urgent: bool) {
        let (lock, cv) = &*self.queue;
        let Ok(mut q) = lock.lock() else { return };
        if let Some(i) = q.iter().position(|(p, _)| *p == path) {
            if !urgent {
                return;
            }
            q.remove(i);
        }
        let item = (path, Arc::clone(&self.stats));
        if urgent {
            q.push_front(item);
        } else {
            q.push_back(item);
        }
        cv.notify_one();
    }
}

fn step(at: usize, delta: i64, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = i64::try_from(len - 1).unwrap_or(i64::MAX);
    let next = (i64::try_from(at).unwrap_or(0) + delta).clamp(0, max);
    usize::try_from(next).unwrap_or(0)
}

/// The undone index nearest to `focus`, looking forward first.
fn nearest<T>(todo: &[Option<T>], focus: usize) -> Option<usize> {
    let n = todo.len();
    let focus = focus.min(n.saturating_sub(1));
    (0..n).find_map(|d| {
        let ahead = focus + d;
        if ahead < n && todo[ahead].is_some() {
            return Some(ahead);
        }
        focus.checked_sub(d).filter(|&i| i < n && todo[i].is_some())
    })
}

/// Listens for `ctl` commands. The viewer works without its socket.
fn serve_ctl(
    tx: &Sender<Msg>,
    worktree: &Path,
    timings: &mut Timings,
) -> (Arc<Mutex<ctl::Status>>, Option<ctl::Cleanup>) {
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
        Err(e) => {
            timings.note(&format!("control socket unavailable: {e:#}"));
            (status, None)
        }
    }
}

/// Files are watched in every worktree where watches are cheap; on Linux
/// each directory costs an inotify watch, so only the selected one is.
const WATCH_ALL_FILES: bool = !cfg!(target_os = "linux");

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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
        crate::render::dump(terminal.backend().buffer(), false)
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
            modifiers: KeyModifiers::NONE,
        });
    }

    const TWO: &[(&str, &str, &str)] = &[
        ("a.rs", "a\nb\nc\n", "a\nB\nc\nd\n"),
        ("b.rs", "x\n", "y\n"),
    ];

    fn agent(id: &str, block: &str) -> deliver::Agent {
        deliver::Agent {
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
        rt.resolve_seq += 1;
        let before = rt.resolve_seq;
        rt.app.picker = Some(crate::picker::Picker::new(vec![agent("x", "b:2")], None));
        rt.key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(rt.app.review.agent.as_deref(), Some("x"));
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
            matches!(&rt.resolution, Some(deliver::Resolution::Agent { agent, .. }) if agent.session_id == "x"),
            "{}",
            rt.app.target
        );
    }

    #[test]
    fn a_send_finishing_after_a_switch_is_recorded_in_its_worktree() {
        let root = std::env::temp_dir().join(format!("dots-diff-sent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (a, b) = (root.join("a"), root.join("b"));
        let (mut rt, _rx) = runtime(TWO);
        rt.install_review(
            Path::new("/wt/a"),
            Ok((a.clone(), review::State::default())),
        );
        let id = rt.app.review.add(
            review::Place::Review,
            review::Kind::Comment,
            "x".into(),
            None,
        );
        rt.app.review_dirty = true;
        rt.install_review(Path::new("/wt/b"), Ok((b, review::State::default())));
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
        assert!(rt.app.review.comments.is_empty(), "b's review is untouched");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_mouse_does_nothing_under_the_editor_and_overlays() {
        let (mut rt, _rx) = runtime(TWO);
        let x = rt.app.areas.diff.x + 10;
        press(&mut rt, "j");
        press(&mut rt, "c");
        assert!(rt.app.editor.is_some(), "{:?}", rt.app.message);
        let row = rt.app.cursor_row;
        mouse(&mut rt, MouseEventKind::Drag(MouseButton::Left), x, 5);
        assert_eq!((rt.app.visual.clone(), rt.app.cursor_row), (None, row));
        rt.key(KeyEvent::from(KeyCode::Esc));
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

    #[test]
    fn diffs_files_nearest_the_view_first() {
        let todo = [Some(()), None, None, Some(()), Some(())];
        assert_eq!(nearest(&todo, 2), Some(3));
        assert_eq!(nearest(&todo, 1), Some(0));
        assert_eq!(nearest(&[None::<()>, None], 0), None);
        assert_eq!(nearest(&todo, 9), Some(4));
    }
}
