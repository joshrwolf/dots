//! Development tools, run as the crate's examples: one frame as text and
//! a PNG (`render`), and in-process measurements of the code paths the
//! viewer runs (`bench`). Never part of the installed program.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{self, App, Body, Settings};
use crate::highlight;
use crate::repo::{self, Mode, Reader};
use crate::stats::{self, Stats};
use crate::theme::{Detected, Palette};
use crate::{Look, ui};

/// One frame of the viewer, drawn without a terminal.
#[derive(Debug, Clone)]
pub struct Frame {
    pub path: PathBuf,
    pub worktree: Option<PathBuf>,
    pub file: Option<String>,
    pub width: u16,
    pub height: u16,
    pub ansi: bool,
    /// Light colours unless the look fixes them.
    pub look: Look,
    /// Lines to scroll down from the file (or the top).
    pub scroll: u32,
    /// Also draw the frame as a PNG here.
    pub png: Option<PathBuf>,
    /// Open the worktree switcher with this query.
    pub switcher: Option<String>,
    /// Sidebar folders to collapse, by path.
    pub collapse: Vec<String>,
    /// Mark files under these path prefixes reviewed.
    pub review: Vec<String>,
    /// Focus the sidebar with its cursor on this file or folder path.
    pub select: Option<String>,
    /// Mark moved code.
    pub moves: bool,
}

/// Loads a worktree's changes with every diff computed.
fn loaded(
    path: &Path,
    worktree: Option<&Path>,
    theme: Detected,
    settings: Settings,
    moves: bool,
) -> Result<App> {
    let repo = repo::discover(path)?;
    let current = match worktree {
        Some(w) => repo
            .index_of(w)
            .with_context(|| format!("{} is not a worktree of this repository", w.display()))?,
        None => repo.index_of(path).unwrap_or(0),
    };
    let mode = settings.mode;
    let mut app = App::new(repo, current, Palette::new(theme), settings);
    let problem = app.set_agents(crate::agents::Snapshot::read());
    let wt = app.worktree_path().clone();
    let (changes, review) = crate::run::load(&wt, mode, false);
    let changes = changes.map_err(anyhow::Error::msg)?;
    let mut reader = Reader::new(changes.repo.to_thread_local(), changes.root.clone());
    let bodies: Vec<_> = changes
        .files
        .iter()
        .map(|c| app::compute_body(&mut reader, c))
        .collect();
    let sources = app.attribution_sources();
    let attribution = (!sources.is_empty())
        .then(|| crate::attribution::Sessions::default().attribute(&wt, &sources));
    app.message = app.install(wt, mode, changes, review).or(problem);
    app.set_attribution(attribution);
    for (i, (body, texts)) in bodies.into_iter().enumerate() {
        app.set_body(i, body, texts);
    }
    if moves && let Some(inputs) = app.move_inputs() {
        app.moves = Some(Arc::new(crate::moves::search(&mut reader, &inputs)));
        app.show_moves = true;
    }
    Ok(app)
}

/// Highlights, synchronously, what the viewer would ask for at this position.
fn highlight_visible(app: &mut App, hl: &mut arborium::Highlighter, rows: u16) -> Vec<Duration> {
    let mut times = Vec::new();
    for job in app.wanted_highlights(rows, 0) {
        let t = Instant::now();
        let result = highlight::highlight(hl, job.key.1, &job.text)
            .ok()
            .map(Arc::new);
        times.push(t.elapsed());
        app.cache.keep_highlight(job.key, result);
    }
    times
}

/// The frame as text, with ANSI colours when asked, and as a PNG when asked.
pub fn frame(options: &Frame) -> Result<String> {
    let mut app = loaded(
        &options.path,
        options.worktree.as_deref(),
        options.look.theme().unwrap_or_else(Detected::light),
        options.look.settings(),
        options.moves,
    )?;
    let mut terminal = Terminal::new(TestBackend::new(options.width, options.height))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let rows = app.areas.diff.height;
    if let Some(file) = &options.file {
        let i = app
            .find_file(file)
            .with_context(|| format!("{file} has no changes"))?;
        app.jump_to_file(i, rows);
    }
    app.scroll_by(i64::from(options.scroll), rows);
    for prefix in &options.review {
        app.mark_reviewed(prefix);
    }
    for dir in &options.collapse {
        let path = format!("{}/", dir.trim_end_matches('/'));
        app.sidebar.set_collapsed(&path, true);
    }
    app.sync_files_cursor();
    if let Some(select) = &options.select {
        let row = app
            .sidebar
            .rows()
            .iter()
            .position(|it| match it {
                app::ListItem::Dir { path, .. } => {
                    path.trim_end_matches('/') == select.trim_end_matches('/')
                }
                app::ListItem::File { index, .. } => app
                    .stream
                    .as_ref()
                    .is_some_and(|s| s.files[*index].change.path == *select),
            })
            .with_context(|| format!("{select} is not in the file tree"))?;
        app.select_row(row, rows);
        app.focus = app::Focus::Files;
    }
    if let Some(query) = &options.switcher {
        let stats = Stats::open(app.worktree_path());
        for wt in app.repo.worktrees.clone() {
            if let Ok(s) = stats::summarize(&stats, &wt.path) {
                app.summaries.insert(wt.path, s);
            }
        }
        let mut sw = crate::switcher::Switcher::new(&app);
        for c in query.chars() {
            sw.push(&app, c);
        }
        app.mode = app::Mode::Switcher(sw);
    }
    let mut hl = arborium::Highlighter::new();
    highlight_visible(&mut app, &mut hl, rows);
    terminal.draw(|f| ui::draw(f, &mut app))?;
    if let Some(png) = &options.png {
        crate::shot::picture(terminal.backend().buffer(), png)?;
        crate::graphics::header_picture(
            &app,
            app.areas.diff.width,
            &png.with_extension("header.png"),
        )?;
    }
    Ok(crate::shot::dump(terminal.backend().buffer(), options.ansi))
}

/// What to measure: worktrees of the repository at `path`.
#[derive(Debug, Clone)]
pub struct Bench {
    pub path: PathBuf,
    pub worktrees: Vec<PathBuf>,
    pub width: u16,
    pub height: u16,
    pub summaries: bool,
    /// Count without the on-disk cache, as on a first run.
    pub cold: bool,
}

fn ms(d: Duration) -> String {
    format!("{:.1}ms", d.as_secs_f64() * 1000.0)
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a percentile index of a short sample list"
    )]
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

/// Measures the viewer's work on real worktrees and prints a report.
#[expect(
    clippy::too_many_lines,
    reason = "one benchmark run, step by step, reported in order"
)]
pub fn bench(options: &Bench) -> Result<String> {
    let mut out = String::new();
    let t = Instant::now();
    let repo = repo::discover(&options.path)?;
    let _ = writeln!(
        out,
        "discover + enumerate worktrees: {} ({} worktrees)",
        ms(t.elapsed()),
        repo.worktrees.len()
    );
    let targets: Vec<PathBuf> = if options.worktrees.is_empty() {
        vec![
            repo.worktrees[repo.index_of(&options.path).unwrap_or(0)]
                .path
                .clone(),
        ]
    } else {
        options.worktrees.clone()
    };
    let mut hl = arborium::Highlighter::new();
    for wt in &targets {
        let Some(index) = repo.index_of(wt) else {
            bail!("{} is not a worktree of this repository", wt.display());
        };
        let _ = writeln!(out, "\nworktree #{index}");
        for mode in [Mode::Head, Mode::Base] {
            let t = Instant::now();
            let changes = repo::changes(wt, mode)?;
            let _ = writeln!(
                out,
                "  status scan ({mode:?}): {} → {} files vs {}",
                ms(t.elapsed()),
                changes.files.len(),
                redact(&changes.base)
            );
        }
        // First frame of a switch: load, diff the files on screen, draw.
        let settings = Settings::default();
        let switch = Instant::now();
        let changes = repo::changes(wt, Mode::Base)?;
        let loaded_at = switch.elapsed();
        let mut app = App::new(
            repo.clone(),
            index,
            Palette::new(Detected::light()),
            settings,
        );
        let files: Vec<repo::FileChange> = changes.files.clone();
        let (local, root) = (changes.repo.to_thread_local(), changes.root.clone());
        let mut reader = Reader::new(changes.repo.to_thread_local(), root.clone());
        let review = Err("the bench reads no review".to_owned());
        app.install(wt.clone(), Mode::Base, changes, review);
        let mut terminal = Terminal::new(TestBackend::new(options.width, options.height))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let rows = app.areas.diff.height;
        let mut first = 0;
        while first < files.len()
            && app
                .stream
                .as_ref()
                .and_then(|s| s.offset(first))
                .is_some_and(|o| o < u32::from(rows))
        {
            let (body, texts) = app::compute_body(&mut reader, &files[first]);
            app.set_body(first, body, texts);
            app.layout(app.areas.diff.width, app.areas.diff.height);
            first += 1;
        }
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let switch_frame = switch.elapsed();
        let _ = writeln!(
            out,
            "  switch to worktree: load {} → first frame with visible diffs {} ({first} files diffed)",
            ms(loaded_at),
            ms(switch_frame)
        );
        // Remaining diffs, sequentially, for the per-file cost.
        let t = Instant::now();
        let mut per_file = Vec::new();
        let (mut added, mut removed) = (0, 0);
        let mut biggest: Option<(u32, usize)> = None;
        for (i, f) in files.iter().enumerate().skip(first) {
            let ft = Instant::now();
            let (body, texts) = app::compute_body(&mut reader, f);
            per_file.push(ft.elapsed());
            if let Body::Text(d) = &body {
                added += d.added;
                removed += d.removed;
                if biggest.is_none_or(|(n, _)| d.new_len > n) {
                    biggest = Some((d.new_len, i));
                }
            }
            app.set_body(i, body, texts);
        }
        per_file.sort();
        let _ = writeln!(
            out,
            "  diff remaining {} files: {} total (p50 {}, p95 {}, max {}); stream +{added} −{removed}",
            per_file.len(),
            ms(t.elapsed()),
            ms(percentile(&per_file, 0.5)),
            ms(percentile(&per_file, 0.95)),
            ms(percentile(&per_file, 1.0)),
        );
        let t = Instant::now();
        app.layout(app.areas.diff.width, app.areas.diff.height);
        let total = app.stream.as_ref().map_or(0, super::app::Stream::total);
        let _ = writeln!(
            out,
            "  layout {} display lines at {} cols: {}",
            total,
            app.areas.diff.width,
            ms(t.elapsed())
        );
        // Highlight what the first screen needs.
        let hl_times = highlight_visible(&mut app, &mut hl, rows);
        let hl_total: Duration = hl_times.iter().sum();
        let _ = writeln!(
            out,
            "  highlight first screen: {} sides, {} total, max {}",
            hl_times.len(),
            ms(hl_total),
            ms(hl_times.iter().max().copied().unwrap_or_default())
        );
        // Scroll through the stream three lines per tick, highlighting as the
        // viewer would, and time the draw of each viewport.
        let mut ticks = Vec::new();
        let mut hl_all = Vec::new();
        let steps = (total / 3).min(3000);
        app.scroll_to(0, rows);
        for _ in 0..steps {
            app.scroll_by(3, rows);
            hl_all.extend(highlight_visible(&mut app, &mut hl, rows));
            let t = Instant::now();
            terminal.draw(|f| ui::draw(f, &mut app))?;
            ticks.push(t.elapsed());
        }
        ticks.sort();
        let _ = writeln!(
            out,
            "  scroll {} ticks (draw only): p50 {}, p95 {}, max {}",
            ticks.len(),
            ms(percentile(&ticks, 0.5)),
            ms(percentile(&ticks, 0.95)),
            ms(percentile(&ticks, 1.0))
        );
        hl_all.sort();
        let _ = writeln!(
            out,
            "  highlight while scrolling: {} sides, p50 {}, p95 {}, max {}",
            hl_all.len(),
            ms(percentile(&hl_all, 0.5)),
            ms(percentile(&hl_all, 0.95)),
            ms(percentile(&hl_all, 1.0))
        );
        if let Some((_, i)) = biggest
            && let (Body::Text(_), Some(texts)) = app::compute_body(&mut reader, &files[i])
            && let Some(lang) = highlight::language(&files[i].path)
        {
            let t = Instant::now();
            let ok = highlight::highlight(&mut hl, lang, &texts.new.1).is_ok();
            let _ = writeln!(
                out,
                "  highlight largest changed file ({} lines, {lang}): {} ok={ok}",
                texts.new.1.len(),
                ms(t.elapsed())
            );
        }
        let [texts_b, colour_b, emph_b] = app.cache.bytes();
        let _ = writeln!(
            out,
            "  memory after diffing and scrolling: footprint {}, resident {} (counts mapped packs), caches: texts {}, colour {}, emphasis {}",
            footprint().unwrap_or_else(|| "?".to_owned()),
            resident().map_or_else(|| "?".to_owned(), mib),
            mib(texts_b),
            mib(colour_b),
            mib(emph_b),
        );
        // A file event's work once it arrives: rescan the paths, diff, lay
        // out and draw. Files are only read, never written.
        let target = app.stream.as_ref().map(|s| s.target);
        let mut refresh = Vec::new();
        for f in files
            .iter()
            .filter(|f| matches!(f.new, repo::Side::Worktree(_)))
            .chain(files.iter())
            .take(5)
        {
            let Some(target) = target else { break };
            let t = Instant::now();
            let paths = vec![f.path.clone()];
            let renames = f
                .from
                .iter()
                .map(|from| (f.path.clone(), from.clone()))
                .collect();
            let found = repo::changes_for(&local, target, &paths, &renames)?;
            let scanned = t.elapsed();
            app.apply_paths(&paths, found);
            if let Some(i) = app
                .stream
                .as_ref()
                .and_then(|s| s.files.iter().position(|x| x.change.path == f.path))
            {
                let change = app.stream.as_ref().map(|s| s.files[i].change.clone());
                if let Some(change) = change {
                    let (body, texts) = app::compute_body(&mut reader, &change);
                    app.set_body(i, body, texts);
                }
                app.jump_to_file(i, rows);
            }
            terminal.draw(|f| ui::draw(f, &mut app))?;
            refresh.push((scanned, t.elapsed()));
        }
        refresh.sort_by_key(|r| r.1);
        if let Some(&(scan, total)) = refresh.get(refresh.len() / 2) {
            let _ = writeln!(
                out,
                "  file event → screen without event delivery, median of {}: {} (path-limited status {})",
                refresh.len(),
                ms(total),
                ms(scan)
            );
        }
    }
    if options.summaries {
        let _ = writeln!(out, "\nrail stats, one worktree at a time:");
        let t = Instant::now();
        let mut times = Vec::new();
        let stats = if options.cold {
            Stats::default()
        } else {
            let t = Instant::now();
            let stats = Stats::open(&options.path);
            let _ = writeln!(out, "  load line-count cache: {}", ms(t.elapsed()));
            stats
        };
        for (i, wt) in repo.worktrees.iter().enumerate() {
            let w = Instant::now();
            let s = stats::summarize(&stats, &wt.path);
            let took = w.elapsed();
            times.push(took);
            match s {
                Ok(s) => {
                    let _ = writeln!(
                        out,
                        "  #{i:<3} {:>9}  {} files +{} −{}  {}",
                        ms(took),
                        s.files,
                        s.added,
                        s.removed,
                        wt.path.display()
                    );
                }
                Err(e) => {
                    let _ = writeln!(out, "  #{i:<3} {:>9}  error: {e:#}", ms(took));
                }
            }
        }
        times.sort();
        let _ = writeln!(
            out,
            "  total {} for {} worktrees (p50 {}, p95 {}, max {})",
            ms(t.elapsed()),
            times.len(),
            ms(percentile(&times, 0.5)),
            ms(percentile(&times, 0.95)),
            ms(percentile(&times, 1.0))
        );
    }
    Ok(out)
}

/// Reports name the base by its kind only, so output can be shared.
fn redact(base: &str) -> String {
    base.split_whitespace()
        .next()
        .map_or_else(String::new, |s| {
            if s == "HEAD" {
                "HEAD".to_owned()
            } else {
                "the base".to_owned()
            }
        })
}

fn mib(bytes: usize) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "byte counts stay far below f64's exact range"
    )]
    let m = bytes as f64 / (1024.0 * 1024.0);
    format!("{m:.1} MiB")
}

/// This process's resident memory, as `ps` reports it.
fn resident() -> Option<usize> {
    let mut command = std::process::Command::new("ps");
    command.args(["-o", "rss=", "-p", &std::process::id().to_string()]);
    let out = dots_process::output(command, Duration::from_secs(5), 4096).ok()?;
    let kib: usize = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kib * 1024)
}

/// This process's physical footprint (dirty and compressed memory, without
/// clean file-backed pages such as mapped packs), as `footprint` reports it.
fn footprint() -> Option<String> {
    let mut command = std::process::Command::new("footprint");
    command.arg(std::process::id().to_string());
    let out = dots_process::output(command, Duration::from_secs(20), 1 << 20).ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("Footprint:"))?;
    Some(line.split("Footprint:").nth(1)?.trim().to_owned())
}
