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
    self, App, Body, FileTexts, Focus, LayoutChoice, Line, ListItem, Settings, Summary,
};
use crate::ctl::{self, Command};
use crate::graphics::Graphics;
use crate::highlight::{self, Key};
use crate::input::Input;
use crate::repo::{self, Changes, FileChange, Fsmonitor, Mode, Reader, Side, Stamp};
use crate::stats::{self, Stats};
use crate::switcher::Switcher;
use crate::theme::{self, Detected, Palette};
use crate::timings::Timings;
use crate::ui;
use crate::watch::{self, Burst, Refresh, Touched, Watch};

#[allow(clippy::large_enum_variant)]
pub(crate) enum Msg {
    Input(Event),
    Loaded {
        generation: u64,
        worktree: PathBuf,
        mode: Mode,
        result: Result<Changes, String>,
        elapsed: Duration,
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
        signature: Option<Signature>,
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
    /// Fixed colours instead of the terminal's.
    pub(crate) theme: Option<Detected>,
    pub(crate) graphics: bool,
    #[allow(clippy::option_option)]
    pub(crate) timings: Option<Option<PathBuf>>,
    pub(crate) settings: Settings,
}

/// How often the safety net checks for changes the watcher missed.
const RECONCILE: Duration = Duration::from_secs(10);

/// What decides a worktree's changes besides its files: HEAD, the index
/// and the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signature {
    head: Option<gix::ObjectId>,
    index: Option<Stamp>,
    target: repo::Target,
}
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
    signature: Option<Signature>,
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
    /// The worktree the file watch was set up around.
    watched: Option<usize>,
}

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
            ratatui::crossterm::event::EnableMouseCapture
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
        let pool_tx = Mutex::new(tx.clone());
        let pool = highlight::Pool::new(2, move |done| {
            if let Ok(tx) = pool_tx.lock() {
                let _ = tx.send(Msg::Highlighted(done));
            }
        });
        let socket = ctl::socket_path();
        let ctl_tx = Mutex::new(tx.clone());
        let _cleanup = match ctl::serve(&socket, move |c| {
            if let Ok(tx) = ctl_tx.lock() {
                let _ = tx.send(Msg::Ctl(c));
            }
        }) {
            Ok(()) => Some(ctl::Cleanup(socket)),
            Err(e) => {
                timings.note(&format!("control socket unavailable: {e:#}"));
                None
            }
        };
        let tx_for_summaries = tx.clone();
        let mut rt = Runtime {
            app,
            tx,
            generation: 0,
            diffs: None,
            pool,
            watcher: None,
            summaries: SummaryQueue::new(SUMMARY_THREADS, &tx_for_summaries),
            stale: HashMap::new(),
            burst: Burst::default(),
            partial_busy: false,
            epoch: 0,
            refreshing: None,
            next_reconcile: Instant::now() + RECONCILE,
            signature: None,
            wanted: Vec::new(),
            timings,
            loading_since: Some(started),
            theme_fixed: options.theme.is_some(),
            detected,
            next_theme: Instant::now() + THEME_POLL,
            quit: false,
            summaries_started: false,
            source: None,
            watched: None,
        };
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
        ratatui::crossterm::event::DisableMouseCapture
    );
    ratatui::try_restore()?;
    let timings = result?;
    if let Some(dest) = options.timings {
        timings.write(started, dest.as_deref())?;
    }
    Ok(())
}

impl Runtime {
    fn view_rows(&self) -> u16 {
        self.app.areas.diff.height.max(1)
    }

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
                terminal.draw(|f| ui::draw(f, &mut self.app))?;
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
                elapsed,
            } => {
                if generation != self.generation {
                    return false;
                }
                match result {
                    Ok(changes) => {
                        self.timings.span(
                            "load changes",
                            elapsed,
                            &format!("{} files", changes.files.len()),
                        );
                        self.app.message = None;
                        let reader =
                            Reader::new(changes.repo.to_thread_local(), changes.root.clone());
                        self.source = Some((changes.repo, changes.root));
                        self.app.load(
                            worktree.clone(),
                            mode,
                            changes.base,
                            changes.target,
                            changes.files,
                            Some(reader),
                        );
                        self.epoch += 1;
                        self.spawn_diffs();
                    }
                    Err(e) => self.app.message = Some(e),
                }
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
                signature,
                moved,
            } => {
                if generation != self.generation {
                    return false;
                }
                match (&self.signature, signature) {
                    (Some(old), Some(new)) if *old != new => {
                        self.burst.push_full(Instant::now());
                    }
                    (_, new) => self.signature = new,
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
            Command::File(path) => {
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
        if let Some(job) = self.diffs.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.burst = Burst::default();
        self.signature = None;
        let generation = self.generation;
        let worktree = self.app.worktree_path().clone();
        let mode = self.app.settings.mode;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            // The viewed worktree may use, and start, the configured fsmonitor.
            let result = repo::changes(&worktree, mode, Fsmonitor::AsConfigured)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Loaded {
                generation,
                worktree,
                mode,
                result,
                elapsed: started.elapsed(),
            });
        });
    }

    /// Rescans `paths` alone, off the event loop.
    fn start_partial(&mut self, paths: Vec<String>, since: Instant) {
        let (Some((repo, _)), Some(stream)) = (self.source.clone(), &self.app.stream) else {
            return;
        };
        let target = stream.target;
        let generation = self.generation;
        let tx = self.tx.clone();
        self.partial_busy = true;
        std::thread::spawn(move || {
            let repo = repo.to_thread_local();
            let result = repo::changes_for(&repo, target, &paths).map_err(|e| format!("{e:#}"));
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
        let mode = stream.mode;
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
            let signature = signature(&worktree, mode).ok();
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
                signature,
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
            Event::Resize(..) | Event::FocusGained => true,
            _ => false,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn key(&mut self, key: KeyEvent) {
        let rows = self.view_rows();
        let half = i64::from(rows / 2);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        self.app.message = None;
        // Ctrl-C quits from anywhere, overlays included.
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
        match (key.code, ctrl) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), true) => self.quit = true,
            (KeyCode::Char('?'), _) => self.app.help = true,
            (KeyCode::Enter | KeyCode::Char(' '), _)
                if self.app.focus == Focus::Files && self.app.toggle_dir(self.app.cursor) => {}
            (KeyCode::Char('d'), true) => self.scroll(half),
            (KeyCode::Char('u'), true) => self.scroll(-half),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown | KeyCode::Char(' '), _) => {
                self.scroll(i64::from(rows.saturating_sub(2)));
            }
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => {
                self.scroll(-i64::from(rows.saturating_sub(2)));
            }
            (KeyCode::Tab | KeyCode::BackTab, _) => {
                self.app.focus = if self.app.focus == Focus::Diff && self.app.areas.files.width > 0
                {
                    Focus::Files
                } else {
                    Focus::Diff
                };
            }
            (KeyCode::Char('h') | KeyCode::Left, _) if self.app.focus == Focus::Files => {
                self.tree_left();
            }
            (KeyCode::Char('h') | KeyCode::Left, _) if self.app.areas.files.width > 0 => {
                self.app.focus = Focus::Files;
            }
            (KeyCode::Char('l') | KeyCode::Right, _) if self.app.focus == Focus::Files => {
                self.tree_right();
            }
            (KeyCode::Char('l') | KeyCode::Right, _) => self.app.focus = Focus::Diff,
            (KeyCode::Char('j') | KeyCode::Down, _) => self.move_by(1),
            (KeyCode::Char('k') | KeyCode::Up, _) => self.move_by(-1),
            (KeyCode::Char('g') | KeyCode::Home, _) => {
                self.app.scroll_to(0, rows);
                self.app.sync_files_cursor();
            }
            (KeyCode::Char('G'), _) => {
                self.app.scroll_to(u32::MAX, rows);
                self.app.sync_files_cursor();
            }
            (KeyCode::Char('J' | ']'), _) => self.app.next_file(true, rows),
            (KeyCode::Char('K' | '['), _) => self.app.next_file(false, rows),
            (KeyCode::Char('n'), _) => self.app.next_hunk(true, rows),
            (KeyCode::Char('N' | 'p'), _) => self.app.next_hunk(false, rows),
            (KeyCode::Char('}'), _) => {
                self.switch((self.app.current + 1) % self.app.repo.worktrees.len());
            }
            (KeyCode::Char('{'), _) => {
                let n = self.app.repo.worktrees.len();
                self.switch((self.app.current + n - 1) % n);
            }
            (KeyCode::Char('w'), false) => {
                self.app.switcher = Some(Switcher::new(&self.app.repo, self.app.current));
            }
            (KeyCode::Enter, _) => match self.app.focus {
                Focus::Files => {
                    self.app.select_row(self.app.cursor, rows);
                    self.app.focus = Focus::Diff;
                }
                Focus::Diff => {
                    self.app.expand_visible_gap(rows);
                }
            },
            (KeyCode::Char('e'), _) => {
                self.app.expand_visible_gap(rows);
            }
            (KeyCode::Char('E'), _) => self.app.toggle_expand_all(),
            (KeyCode::Char('v'), _) => self.app.toggle_reviewed(),
            (KeyCode::Char('s'), _) => {
                self.app.settings.layout = match self.app.settings.layout {
                    LayoutChoice::Auto => LayoutChoice::Unified,
                    LayoutChoice::Unified => LayoutChoice::Split,
                    LayoutChoice::Split => LayoutChoice::Auto,
                };
            }
            (KeyCode::Char('z'), _) => {
                self.app.settings.wrap = !self.app.settings.wrap;
                self.app.relayout();
            }
            (KeyCode::Char('m'), _) => {
                self.app.settings.mode = match self.app.settings.mode {
                    Mode::Base => Mode::Head,
                    Mode::Head => Mode::Base,
                };
                self.app.stream = None;
                self.loading_since = Some(Instant::now());
                self.start_load();
            }
            (KeyCode::Char('t'), _) => {
                self.app.settings.sidebar = !self.app.settings.sidebar;
                if !self.app.settings.sidebar {
                    self.app.focus = Focus::Diff;
                }
            }
            (KeyCode::Char(c @ ('<' | '>')), _) => {
                let now = self.app.areas.files.width;
                let width = if c == '<' {
                    now.saturating_sub(4)
                } else {
                    now + 4
                };
                self.app.settings.sidebar = true;
                self.app.settings.sidebar_width = Some(width.clamp(16, 64));
            }
            (KeyCode::Char('r'), _) => self.start_load(),
            _ => {}
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

    fn scroll(&mut self, delta: i64) {
        let rows = self.view_rows();
        self.app.scroll_by(delta, rows);
        self.app.sync_files_cursor();
    }

    fn move_by(&mut self, delta: i64) {
        match self.app.focus {
            Focus::Diff => self.scroll(delta),
            Focus::Files => {
                let row = step(self.app.cursor, delta, self.app.list.len());
                self.app.select_row(row, self.view_rows());
            }
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.app.switcher.is_some() {
            return false;
        }
        let at = Position::new(mouse.column, mouse.row);
        let areas = self.app.areas;
        let rows = self.view_rows();
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
                let line = self.app.global_top() + u32::from(mouse.row - areas.diff.y);
                if let Line::Body { file, row, .. } = self.app.line_at(line) {
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

/// Files are watched in every worktree where watches are cheap; on Linux
/// each directory costs an inotify watch, so only the selected one is.
const WATCH_ALL_FILES: bool = !cfg!(target_os = "linux");

/// A worktree's [`Signature`].
fn signature(worktree: &Path, mode: Mode) -> Result<Signature> {
    let (repo, _, target) = repo::open(worktree, mode)?;
    let index = std::fs::metadata(repo.index_path()).ok().map(|m| Stamp {
        modified: m.modified().ok(),
        len: m.len(),
    });
    Ok(Signature {
        head: repo.head_id().ok().map(gix::Id::detach),
        index,
        target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_files_nearest_the_view_first() {
        let todo = [Some(()), None, None, Some(()), Some(())];
        assert_eq!(nearest(&todo, 2), Some(3));
        assert_eq!(nearest(&todo, 1), Some(0));
        assert_eq!(nearest(&[None::<()>, None], 0), None);
        assert_eq!(nearest(&todo, 9), Some(4));
    }
}
