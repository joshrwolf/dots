//! Non-interactive output: one frame as text (`render`) and in-process
//! measurements of the same code paths the viewer runs (`bench`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::app::{self, App, Body, Settings};
use crate::highlight;
use crate::repo::{self, Mode};
use crate::run::summarize;
use crate::theme::{Detected, Palette};
use crate::ui;

#[derive(Debug, Clone)]
pub(crate) struct Frame {
    pub(crate) path: PathBuf,
    pub(crate) worktree: Option<PathBuf>,
    pub(crate) file: Option<String>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) ansi: bool,
    pub(crate) theme: Detected,
    pub(crate) settings: Settings,
    /// Lines to scroll down from the file (or the top).
    pub(crate) scroll: u32,
    /// Also draw the frame as a PNG here.
    pub(crate) png: Option<PathBuf>,
    /// Open the worktree switcher with this query.
    pub(crate) switcher: Option<String>,
}

/// Loads a worktree's changes with every diff computed.
fn loaded(
    path: &Path,
    worktree: Option<&Path>,
    theme: Detected,
    settings: Settings,
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
    let wt = app.worktree_path().clone();
    let changes = repo::changes(&wt, mode)?;
    let source = changes.repo.to_thread_local();
    let bodies: Vec<Body> = changes
        .files
        .iter()
        .map(|c| app::compute_body(&source, &changes.root, c))
        .collect();
    app.load(wt, mode, changes.base, changes.files);
    for (i, body) in bodies.into_iter().enumerate() {
        app.set_body(i, body);
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
        app.highlights.insert(job.key, result);
    }
    times
}

pub(crate) fn frame(options: &Frame) -> Result<String> {
    let mut app = loaded(
        &options.path,
        options.worktree.as_deref(),
        options.theme,
        options.settings.clone(),
    )?;
    let mut terminal = Terminal::new(TestBackend::new(options.width, options.height))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let rows = app.areas.diff.height;
    if let Some(file) = &options.file {
        let i = app
            .stream
            .as_ref()
            .and_then(|s| {
                s.files
                    .iter()
                    .position(|f| f.change.path == *file || f.change.path.ends_with(file.as_str()))
            })
            .with_context(|| format!("{file} has no changes"))?;
        app.jump_to_file(i, rows);
    }
    app.scroll_by(i64::from(options.scroll), rows);
    if let Some(query) = &options.switcher {
        for wt in app.repo.worktrees.clone() {
            if let Ok(s) = crate::run::summarize(&wt.path) {
                app.summaries.insert(wt.path, s);
            }
        }
        let mut sw = crate::switcher::Switcher::new(&app.repo, app.current);
        for c in query.chars() {
            sw.push(&app.repo, c);
        }
        app.switcher = Some(sw);
    }
    let mut hl = arborium::Highlighter::new();
    highlight_visible(&mut app, &mut hl, rows);
    terminal.draw(|f| ui::draw(f, &mut app))?;
    if let Some(png) = &options.png {
        picture(terminal.backend().buffer(), png)?;
        crate::graphics::header_picture(
            &app,
            app.areas.diff.width,
            &png.with_extension("header.png"),
        )?;
    }
    Ok(dump(terminal.backend().buffer(), options.ansi))
}

/// The buffer as lines of text, with SGR colours when `ansi`.
pub(crate) fn dump(buf: &Buffer, ansi: bool) -> String {
    let mut out = String::new();
    let area = buf.area;
    for y in area.top()..area.bottom() {
        let mut last = None;
        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];
            if ansi {
                let style = (cell.fg, cell.bg, cell.modifier);
                if last != Some(style) {
                    out.push_str("\x1b[0");
                    if let Color::Rgb(r, g, b) = cell.fg {
                        let _ = write!(out, ";38;2;{r};{g};{b}");
                    }
                    if let Color::Rgb(r, g, b) = cell.bg {
                        let _ = write!(out, ";48;2;{r};{g};{b}");
                    }
                    if cell.modifier.contains(Modifier::BOLD) {
                        out.push_str(";1");
                    }
                    if cell.modifier.contains(Modifier::ITALIC) {
                        out.push_str(";3");
                    }
                    out.push('m');
                    last = Some(style);
                }
            }
            out.push_str(cell.symbol());
        }
        if ansi {
            out.push_str("\x1b[0m");
        }
        let trimmed = out.trim_end_matches(' ').len();
        if !ansi {
            out.truncate(trimmed);
        }
        out.push('\n');
    }
    out
}

#[derive(Debug, Clone)]
pub(crate) struct Bench {
    pub(crate) path: PathBuf,
    pub(crate) worktrees: Vec<PathBuf>,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) summaries: bool,
}

fn ms(d: Duration) -> String {
    format!("{:.1}ms", d.as_secs_f64() * 1000.0)
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

/// Measures the viewer's work on real worktrees and prints a report.
#[allow(clippy::too_many_lines)]
pub(crate) fn bench(options: &Bench) -> Result<String> {
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
        let source = changes.repo.to_thread_local();
        let root = changes.root.clone();
        app.load(wt.clone(), Mode::Base, changes.base, changes.files);
        let mut terminal = Terminal::new(TestBackend::new(options.width, options.height))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let rows = app.areas.diff.height;
        let mut first = 0;
        while first < files.len()
            && app
                .stream
                .as_ref()
                .is_some_and(|s| s.offsets[first] < u32::from(rows))
        {
            app.set_body(first, app::compute_body(&source, &root, &files[first]));
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
        for (i, f) in files.iter().enumerate().skip(first) {
            let ft = Instant::now();
            let body = app::compute_body(&source, &root, f);
            per_file.push(ft.elapsed());
            if let Body::Text(d) = &body {
                added += d.added;
                removed += d.removed;
            }
            app.set_body(i, body);
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
        let biggest = files
            .iter()
            .filter_map(|f| f.new.map(|n| (n.len, f)))
            .max_by_key(|(n, _)| *n);
        if let Some((_, f)) = biggest
            && let Body::Text(d) = app::compute_body(&source, &root, f)
            && let Some(lang) = highlight::language(&f.path)
        {
            let t = Instant::now();
            let ok = highlight::highlight(&mut hl, lang, &d.new).is_ok();
            let _ = writeln!(
                out,
                "  highlight largest changed file ({} lines, {lang}): {} ok={ok}",
                d.new.len(),
                ms(t.elapsed())
            );
        }
    }
    if options.summaries {
        let _ = writeln!(out, "\nrail stats, one worktree at a time:");
        let t = Instant::now();
        let mut times = Vec::new();
        for (i, wt) in repo.worktrees.iter().enumerate() {
            let w = Instant::now();
            let s = summarize(&wt.path);
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
                "default-branch merge base".to_owned()
            }
        })
}

/// Draws the buffer as a PNG-like RGBA picture for eyeballing colours
/// without a terminal: cells as rectangles, text in a monospace font.
#[allow(clippy::many_single_char_names)]
pub(crate) fn picture(buf: &Buffer, path: &Path) -> Result<()> {
    use cosmic_text::{
        Attrs, Buffer as TextBuffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight,
    };
    let (cw, ch) = (9u32, 19u32);
    let area = buf.area;
    let (w, h) = (u32::from(area.width) * cw, u32::from(area.height) * ch);
    let mut pixmap = tiny_skia::Pixmap::new(w, h).context("picture size")?;
    let rgb = |c: Color, fallback: (u8, u8, u8)| match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => fallback,
    };
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let (r, g, b) = rgb(buf[(x, y)].bg, (255, 255, 255));
            let rect =
                tiny_skia::Rect::from_xywh(f32::from(x) * 9.0, f32::from(y) * 19.0, 9.0, 19.0)
                    .context("cell")?;
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(r, g, b, 255);
            pixmap.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
        }
    }
    let mut fonts = FontSystem::new();
    let mut cache = SwashCache::new();
    let data_w = pixmap.width();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &buf[(x, y)];
            let symbol = cell.symbol();
            if symbol.trim().is_empty() {
                continue;
            }
            let (r, g, b) = rgb(cell.fg, (0, 0, 0));
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                Weight::BOLD
            } else {
                Weight::NORMAL
            };
            let mut text = TextBuffer::new(&mut fonts, Metrics::new(14.0, 19.0));
            text.set_size(None, None);
            text.set_text(
                symbol,
                &Attrs::new().family(Family::Monospace).weight(weight),
                Shaping::Advanced,
                None,
            );
            text.shape_until_scroll(&mut fonts, false);
            let (ox, oy) = (i32::from(x) * 9, i32::from(y) * 19);
            let data = pixmap.data_mut();
            text.draw(
                &mut fonts,
                &mut cache,
                cosmic_text::Color::rgb(r, g, b),
                |gx, gy, gw, gh, color| {
                    let a = u32::from(color.a());
                    for yy in oy + gy..oy + gy + i32::try_from(gh).unwrap_or(0) {
                        for xx in ox + gx..ox + gx + i32::try_from(gw).unwrap_or(0) {
                            let (Ok(ux), Ok(uy)) = (u32::try_from(xx), u32::try_from(yy)) else {
                                continue;
                            };
                            if ux >= data_w || uy >= h {
                                continue;
                            }
                            let i = ((uy * data_w + ux) * 4) as usize;
                            let blend = |d: u8, s: u8| {
                                u8::try_from((u32::from(s) * a + u32::from(d) * (255 - a)) / 255)
                                    .unwrap_or(255)
                            };
                            data[i] = blend(data[i], color.r());
                            data[i + 1] = blend(data[i + 1], color.g());
                            data[i + 2] = blend(data[i + 2], color.b());
                        }
                    }
                },
            );
        }
    }
    pixmap
        .save_png(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
