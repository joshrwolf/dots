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

use crate::app::{self, App, Body, Focus, LayoutChoice, Line, ListItem, Settings, Summary};
use crate::ctl::{self, Command};
use crate::graphics::Graphics;
use crate::highlight::{self, Key};
use crate::input::Input;
use crate::repo::{self, Changes, FileChange, Mode};
use crate::theme::{self, Detected, Palette};
use crate::timings::Timings;
use crate::ui;
use crate::watch::{self, Touched, Watch};

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
        generation: u64,
        file: usize,
        body: Body,
        elapsed: Duration,
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

/// Wait after the last file event before reloading.
const DEBOUNCE: Duration = Duration::from_millis(250);
/// Rex reports no theme change, so the colours are asked for again.
const THEME_POLL: Duration = Duration::from_secs(2);
/// Quiet time before another worktree's rail counts are recomputed.
const RAIL_SETTLE: Duration = Duration::from_secs(3);
/// Threads computing rail counts.
const RAIL_THREADS: usize = 3;
const PREFETCH: usize = 3;

struct DiffJob {
    cancel: Arc<AtomicBool>,
    focus: Arc<AtomicUsize>,
}

struct Runtime {
    app: App,
    tx: Sender<Msg>,
    generation: u64,
    diffs: Option<DiffJob>,
    pool: highlight::Pool,
    watcher: Option<Watch>,
    summaries: SummaryQueue,
    /// Worktrees whose rail counts are stale, and when they last changed.
    stale: HashMap<PathBuf, Instant>,
    refresh_at: Option<Instant>,
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
            summaries: SummaryQueue::new(RAIL_THREADS, &tx_for_summaries),
            stale: HashMap::new(),
            refresh_at: None,
            wanted: Vec::new(),
            timings,
            loading_since: Some(started),
            theme_fixed: options.theme.is_some(),
            detected,
            next_theme: Instant::now() + THEME_POLL,
            quit: false,
            summaries_started: false,
            source: None,
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
                deadline = deadline.min(*at + RAIL_SETTLE);
            }
            if graphics.as_deref().is_some_and(Graphics::pending) {
                deadline = deadline.min(now + Duration::from_millis(50));
                dirty = true;
            }
            if let Some(at) = self.refresh_at {
                deadline = deadline.min(at);
            }
            match rx.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(msg) => dirty |= self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(msg) = rx.try_recv() {
                dirty |= self.handle(msg);
            }
            let now = Instant::now();
            if self.refresh_at.is_some_and(|at| at <= now) {
                self.refresh_at = None;
                self.start_load();
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
                .filter(|(_, at)| **at + RAIL_SETTLE <= now)
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
                        self.source = Some((changes.repo, changes.root));
                        self.app
                            .load(worktree.clone(), mode, changes.base, changes.files);
                        self.spawn_diffs();
                    }
                    Err(e) => self.app.message = Some(e),
                }
                true
            }
            Msg::Diffed {
                generation,
                file,
                body,
                elapsed,
            } => {
                if generation != self.generation {
                    return false;
                }
                let lines = match &body {
                    Body::Text(d) => d.old.len() + d.new.len(),
                    _ => 0,
                };
                self.timings.diff(elapsed, lines);
                self.app.set_body(file, body);
                self.update_own_summary();
                true
            }
            Msg::Highlighted(done) => {
                self.timings.highlight(done.elapsed);
                self.app.highlights.insert(done.key, done.result.ok());
                true
            }
            Msg::Summary { path, summary } => {
                if let Ok(s) = summary {
                    self.app.summaries.insert(path, s);
                }
                self.app.areas.rail.width > 0
            }
            Msg::Touched(touched) => {
                let current = self.app.current;
                match touched {
                    Touched::Worktree(i) if i != current => {
                        if let Some(w) = self.app.repo.worktrees.get(i) {
                            self.stale.insert(w.path.clone(), Instant::now());
                        }
                    }
                    _ => self.refresh_at = Some(Instant::now() + DEBOUNCE),
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
        self.app.rail_cursor = worktree;
        self.app.stream = None;
        self.loading_since = Some(Instant::now());
        self.start_load();
    }

    fn start_load(&mut self) {
        self.generation += 1;
        if let Some(job) = self.diffs.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        let generation = self.generation;
        let worktree = self.app.worktree_path().clone();
        let mode = self.app.settings.mode;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let result = repo::changes(&worktree, mode).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Loaded {
                generation,
                worktree,
                mode,
                result,
                elapsed: started.elapsed(),
            });
        });
    }

    /// Starts watching every worktree; setting up takes a moment, so it
    /// happens off the event loop.
    fn spawn_watch(&self) {
        let roots: Vec<PathBuf> = self
            .app
            .repo
            .worktrees
            .iter()
            .map(|w| w.path.clone())
            .collect();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let events = Mutex::new(tx.clone());
            let result = watch::watch(&roots, move |t| {
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
            let (todo, cancel, focus, tx, generation) = (
                Arc::clone(&todo),
                Arc::clone(&job.cancel),
                Arc::clone(&job.focus),
                self.tx.clone(),
                self.generation,
            );
            std::thread::spawn(move || {
                let repo = repo.to_thread_local();
                while !cancel.load(Ordering::Relaxed) {
                    let next = {
                        let Ok(mut todo) = todo.lock() else { return };
                        nearest(&todo, focus.load(Ordering::Relaxed))
                            .and_then(|i| todo[i].take().map(|c| (i, c)))
                    };
                    let Some((file, change)) = next else { return };
                    let started = Instant::now();
                    let body = app::compute_body(&repo, &root, &change);
                    if tx
                        .send(Msg::Diffed {
                            generation,
                            file,
                            body,
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

    /// Queues every worktree's rail counts, those near the rail cursor first.
    fn spawn_summaries(&self) {
        let n = self.app.repo.worktrees.len();
        let at = self.app.rail_cursor.min(n.saturating_sub(1));
        let order = (0..n).map(|d| (at + d) % n);
        for i in order {
            self.summaries
                .push(self.app.repo.worktrees[i].path.clone(), false);
        }
    }

    /// The selected worktree's rail entry follows its own stream once complete.
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
        if self.app.help {
            self.app.help = false;
            return;
        }
        match (key.code, ctrl) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), true) => self.quit = true,
            (KeyCode::Char('?'), _) => self.app.help = true,
            (KeyCode::Char('d'), true) => self.scroll(half),
            (KeyCode::Char('u'), true) => self.scroll(-half),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown | KeyCode::Char(' '), _) => {
                self.scroll(i64::from(rows.saturating_sub(2)));
            }
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => {
                self.scroll(-i64::from(rows.saturating_sub(2)));
            }
            (KeyCode::Tab, _) => {
                self.app.focus = match self.app.focus {
                    Focus::Worktrees => Focus::Files,
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Worktrees,
                };
            }
            (KeyCode::BackTab, _) => {
                self.app.focus = match self.app.focus {
                    Focus::Worktrees => Focus::Diff,
                    Focus::Files => Focus::Worktrees,
                    Focus::Diff => Focus::Files,
                };
            }
            (KeyCode::Char('h') | KeyCode::Left, _) => {
                self.app.focus = match self.app.focus {
                    Focus::Diff => Focus::Files,
                    _ => Focus::Worktrees,
                };
            }
            (KeyCode::Char('l') | KeyCode::Right, _) => {
                self.app.focus = match self.app.focus {
                    Focus::Worktrees => Focus::Files,
                    _ => Focus::Diff,
                };
            }
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
            (KeyCode::Enter, _) => match self.app.focus {
                Focus::Worktrees => self.switch(self.app.rail_cursor),
                Focus::Files => {
                    self.app.jump_to_file(self.app.files_cursor, rows);
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
            (KeyCode::Char('w'), _) => {
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
            (KeyCode::Char('1'), _) => self.app.settings.show_rail = !self.app.settings.show_rail,
            (KeyCode::Char('2'), _) => self.app.settings.show_files = !self.app.settings.show_files,
            (KeyCode::Char('r'), _) => self.start_load(),
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
            Focus::Worktrees => {
                let n = self.app.repo.worktrees.len();
                self.app.rail_cursor = step(self.app.rail_cursor, delta, n);
            }
            Focus::Files => {
                let n = self.app.stream.as_ref().map_or(0, |s| s.files.len());
                self.app.files_cursor = step(self.app.files_cursor, delta, n);
            }
        }
    }

    fn mouse(&mut self, mouse: MouseEvent) -> bool {
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
            } else if let Some(i) = self.file_at_row(mouse.row) {
                self.app.focus = Focus::Files;
                self.app.jump_to_file(i, rows);
            }
        } else if areas.rail.contains(at) {
            if delta != 0 {
                self.app.focus = Focus::Worktrees;
                self.move_by(delta.signum());
            } else {
                let slot = usize::from(mouse.row.saturating_sub(areas.rail.y + 2) / 2);
                let visible = usize::from(areas.rail.height.saturating_sub(2) / 2);
                let start = self
                    .app
                    .rail_cursor
                    .saturating_sub(visible.saturating_sub(1));
                self.app.focus = Focus::Worktrees;
                self.switch(start + slot);
            }
        }
        true
    }

    /// The file listed at screen row `y`, mirroring the list's scrolling in `ui`.
    fn file_at_row(&self, y: u16) -> Option<usize> {
        let area = self.app.areas.files;
        let rows = usize::from(area.height.saturating_sub(2));
        let cursor_item = self
            .app
            .list
            .iter()
            .position(|it| *it == ListItem::File(self.app.files_cursor))
            .unwrap_or(0);
        let start = cursor_item
            .saturating_sub(rows / 2)
            .min(self.app.list.len().saturating_sub(rows));
        let slot = usize::from(y.checked_sub(area.y + 2)?);
        match self.app.list.get(start + slot)? {
            ListItem::File(i) => Some(*i),
            ListItem::Dir(_) => None,
        }
    }
}

/// Background threads computing rail counts, one worktree at a time each.
struct SummaryQueue {
    queue: Arc<(Mutex<VecDeque<PathBuf>>, Condvar)>,
}

impl SummaryQueue {
    fn new(threads: usize, tx: &Sender<Msg>) -> Self {
        let queue: Arc<(Mutex<VecDeque<PathBuf>>, Condvar)> = Arc::default();
        for _ in 0..threads {
            let (queue, tx) = (Arc::clone(&queue), tx.clone());
            std::thread::spawn(move || {
                loop {
                    let path = {
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
                    let summary = summarize(&path).map_err(|e| format!("{e:#}"));
                    if tx.send(Msg::Summary { path, summary }).is_err() {
                        return;
                    }
                }
            });
        }
        Self { queue }
    }

    /// Queues `path` unless it is already waiting; `urgent` puts it first.
    fn push(&self, path: PathBuf, urgent: bool) {
        let (lock, cv) = &*self.queue;
        let Ok(mut q) = lock.lock() else { return };
        if let Some(i) = q.iter().position(|p| *p == path) {
            if !urgent {
                return;
            }
            q.remove(i);
        }
        if urgent {
            q.push_front(path);
        } else {
            q.push_back(path);
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

pub(crate) fn summarize(path: &Path) -> Result<Summary> {
    let changes = repo::changes(path, Mode::Base)?;
    let repo = changes.repo.to_thread_local();
    let (mut added, mut removed) = (0, 0);
    for change in &changes.files {
        if let Body::Text(d) = app::compute_body(&repo, &changes.root, change) {
            added += d.added;
            removed += d.removed;
        }
    }
    Ok(Summary {
        added,
        removed,
        files: changes.files.len(),
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
