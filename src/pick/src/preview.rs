//! Previews of the selected row: a command's output beside the list.
//!
//! A selection must settle before its command starts; changing selection cancels
//! pending or running work. Recent command outcomes, including failures, are
//! cached for immediate display. Successful cached commands run again only with
//! an interval; failures retry on selection. Results name the command they
//! answer, and the pane shows only the current selection's result.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::{mpsc, watch};

use crate::background::Background;

/// A preview command: the program, then its arguments.
pub(crate) type Argv = Vec<String>;

/// What the pane shows for one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Content {
    Loading,
    Text(Vec<Line<'static>>),
    Failed(String),
}

type Outcome = Result<String, String>;

/// Runs one preview command. Dropping the future must stop the command.
pub(crate) type Run =
    Arc<dyn Fn(Argv, (u16, u16)) -> Pin<Box<dyn Future<Output = Outcome> + Send>> + Send + Sync>;

const DEADLINE: Duration = Duration::from_secs(3);
const LIMIT: usize = 512 * 1024;
const DEBOUNCE: Duration = Duration::from_millis(120);
const CACHE_LIMIT: usize = 64;

/// Commands through `dots-process`, bounded in time and output.
pub(crate) fn commands() -> Run {
    Arc::new(|argv: Argv, (columns, lines)| {
        Box::pin(async move {
            let (program, args) = argv.split_first().ok_or("empty preview command")?;
            let mut command = std::process::Command::new(program);
            command
                .args(args)
                .env("COLUMNS", columns.to_string())
                .env("LINES", lines.to_string());
            let output = dots_process::Runner::new(DEADLINE, LIMIT)
                .run(command)
                .await
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
            }
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(if stderr.is_empty() {
                format!("{program} failed ({})", output.status)
            } else {
                stderr
            })
        })
    })
}

#[derive(Debug, Clone)]
struct Request {
    argv: Argv,
    size: (u16, u16),
    generation: u64,
    run: bool,
}

#[derive(Debug)]
pub(crate) struct Previewer {
    requests: watch::Sender<Option<Request>>,
    results: mpsc::UnboundedReceiver<(u64, Argv, Outcome)>,
    requested: Option<Argv>,
    size: (u16, u16),
    generation: u64,
    shown: Option<Content>,
    cache: VecDeque<(Argv, Content)>,
    every: Option<Duration>,
}

impl Previewer {
    /// Starts the preview job on `background`. It ends when this is dropped.
    pub(crate) fn spawn(background: &mut Background, run: Run, every: Option<Duration>) -> Self {
        let (requests, receiver) = watch::channel(None);
        let (sender, results) = mpsc::unbounded_channel();
        background.spawn(serve(run, receiver, sender, every));
        Self {
            requests,
            results,
            requested: None,
            size: (0, 0),
            generation: 0,
            shown: None,
            cache: VecDeque::new(),
            every,
        }
    }

    /// A new row set or pane size invalidates cached and in-flight results.
    pub(crate) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.requests.send_replace(None);
        self.requested = None;
        self.shown = None;
        self.cache.clear();
    }

    pub(crate) fn set_size(&mut self, size: (u16, u16)) {
        if self.size != size {
            self.size = size;
            self.invalidate();
        }
    }

    /// Asks for the selection's preview, if it changed. `None` clears it.
    pub(crate) fn select(&mut self, argv: Option<&[String]>) {
        if self.requested.as_deref() == argv {
            return;
        }
        self.requested = argv.map(<[String]>::to_vec);
        self.shown = self.requested.clone().and_then(|argv| self.cached(&argv));
        let request = self.requested.as_ref().map(|argv| Request {
            argv: argv.clone(),
            size: self.size,
            generation: self.generation,
            run: self.every.is_some() || !matches!(self.shown, Some(Content::Text(_))),
        });
        self.requests.send_replace(request);
    }

    /// Takes finished previews. Returns whether the pane changed.
    pub(crate) fn update(&mut self) -> bool {
        let mut changed = false;
        while let Ok((generation, argv, outcome)) = self.results.try_recv() {
            if generation != self.generation {
                continue;
            }
            let content = match outcome {
                Ok(text) => Content::Text(styled(&text)),
                Err(message) => Content::Failed(sanitize(&message)),
            };
            if self.requested.as_ref() == Some(&argv) {
                changed |= self.shown.as_ref() != Some(&content);
                self.shown = Some(content.clone());
            }
            self.cache.retain(|(key, _)| key != &argv);
            if self.cache.len() == CACHE_LIMIT {
                self.cache.pop_front();
            }
            self.cache.push_back((argv, content));
        }
        changed
    }

    // A hit becomes the most recently used entry, even if its command is not run.
    fn cached(&mut self, argv: &Argv) -> Option<Content> {
        let index = self.cache.iter().position(|(key, _)| key == argv)?;
        let entry = self
            .cache
            .remove(index)
            .expect("the position was found in the cache");
        let content = entry.1.clone();
        self.cache.push_back(entry);
        Some(content)
    }

    /// What to draw for the current selection.
    pub(crate) fn content(&self) -> Option<Content> {
        self.requested.as_ref()?;
        Some(self.shown.clone().unwrap_or(Content::Loading))
    }
}

async fn serve(
    run: Run,
    mut requests: watch::Receiver<Option<Request>>,
    results: mpsc::UnboundedSender<(u64, Argv, Outcome)>,
    every: Option<Duration>,
) {
    loop {
        let Some(request) = requests.borrow_and_update().clone() else {
            if requests.changed().await.is_err() {
                return;
            }
            continue;
        };
        if !request.run {
            if requests.changed().await.is_err() {
                return;
            }
            continue;
        }
        // Start the debounce anew whenever a selection changes.
        tokio::select! {
            biased;
            changed = requests.changed() => {
                if changed.is_err() { return; }
                continue;
            }
            () = tokio::time::sleep(DEBOUNCE) => {}
        }
        loop {
            let outcome = tokio::select! {
                biased;
                changed = requests.changed() => {
                    if changed.is_err() { return; }
                    break;
                }
                outcome = run(request.argv.clone(), request.size) => outcome,
            };
            if results
                .send((request.generation, request.argv.clone(), outcome))
                .is_err()
            {
                return;
            }
            // Wait for a new selection, or re-run after the interval.
            tokio::select! {
                biased;
                changed = requests.changed() => {
                    if changed.is_err() { return; }
                    break;
                }
                () = rerun(every) => {}
            }
        }
    }
}

async fn rerun(every: Option<Duration>) {
    match every {
        Some(every) => tokio::time::sleep(every).await,
        None => std::future::pending().await,
    }
}

/// Plain notices and failures cannot carry terminal control sequences.
pub(crate) fn sanitize(text: &str) -> String {
    styled(text)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Interpret only SGR; every other terminal escape is discarded before ratatui
/// sees the text. Styles belong to spans, never to the terminal state.
pub(crate) fn styled(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut spans = Vec::new();
    let mut word = String::new();
    let mut style = Style::default();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    if let Some(params) = csi(&mut chars) {
                        flush(&mut spans, &mut word, style);
                        sgr(&params, &mut style);
                    }
                }
                Some(']' | 'P' | '_' | '^' | 'X') => skip_string(&mut chars),
                _ => {}
            },
            '\u{9b}' => {
                csi(&mut chars);
            }
            '\u{90}' | '\u{98}' | '\u{9d}' | '\u{9e}' | '\u{9f}' => skip_string(&mut chars),
            '\u{80}'..='\u{9f}' => {}
            '\r' | '\n' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                flush(&mut spans, &mut word, style);
                lines.push(Line::from(std::mem::take(&mut spans)));
            }
            '\t' => word.push_str("    "),
            ch if ch.is_control() => word.push('\u{fffd}'),
            ch => word.push(ch),
        }
    }
    flush(&mut spans, &mut word, style);
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}

fn flush(spans: &mut Vec<Span<'static>>, word: &mut String, style: Style) {
    if !word.is_empty() {
        spans.push(Span::styled(std::mem::take(word), style));
    }
}

// Only a CSI ending in m and containing parameter bytes can change style.
fn csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let mut params = String::new();
    let mut valid = true;
    for ch in chars.by_ref() {
        if ('@'..='~').contains(&ch) {
            return (ch == 'm' && valid).then_some(params);
        }
        valid &= ch.is_ascii_digit() || ch == ';' || ch == ':';
        if valid {
            params.push(ch);
        }
    }
    None
}

fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(ch) = chars.next() {
        if ch == '\u{7}' || ch == '\u{9c}' {
            break;
        }
        if ch == '\u{1b}' && chars.peek() == Some(&'\\') {
            chars.next();
            break;
        }
    }
}

fn sgr(params: &str, style: &mut Style) {
    // A colon-separated SGR has the same numeric fields for these forms.
    let normalized = params.replace("::", ":").replace(':', ";");
    let values: Vec<Option<u16>> = normalized
        .split(';')
        .map(|value| {
            if value.is_empty() {
                Some(0)
            } else {
                value.parse().ok()
            }
        })
        .collect();
    let mut parts = values.iter().copied();
    while let Some(param) = parts.next() {
        match param {
            Some(0) => *style = Style::default(),
            Some(1) => *style = style.add_modifier(Modifier::BOLD),
            Some(2) => *style = style.add_modifier(Modifier::DIM),
            Some(3) => *style = style.add_modifier(Modifier::ITALIC),
            Some(4) => *style = style.add_modifier(Modifier::UNDERLINED),
            Some(7) => *style = style.add_modifier(Modifier::REVERSED),
            Some(22) => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            Some(23) => *style = style.remove_modifier(Modifier::ITALIC),
            Some(24) => *style = style.remove_modifier(Modifier::UNDERLINED),
            Some(27) => *style = style.remove_modifier(Modifier::REVERSED),
            Some(39) => style.fg = None,
            Some(49) => style.bg = None,
            Some(30..=37 | 90..=97 | 40..=47 | 100..=107) => {
                let Some(code) = param else { continue };
                let color = ansi_color(code % 10, code >= 90);
                *style = if (40..=47).contains(&code) || (100..=107).contains(&code) {
                    style.bg(color)
                } else {
                    style.fg(color)
                };
            }
            Some(38 | 48) => {
                let color = match parts.next() {
                    Some(Some(5)) => parts
                        .next()
                        .flatten()
                        .and_then(|n| u8::try_from(n).ok())
                        .map(Color::Indexed),
                    Some(Some(2)) => {
                        let rgb = [
                            parts.next().flatten(),
                            parts.next().flatten(),
                            parts.next().flatten(),
                        ];
                        rgb.into_iter().collect::<Option<Vec<_>>>().and_then(|rgb| {
                            let [r, g, b] = <[u16; 3]>::try_from(rgb).ok()?;
                            Some(Color::Rgb(
                                u8::try_from(r).ok()?,
                                u8::try_from(g).ok()?,
                                u8::try_from(b).ok()?,
                            ))
                        })
                    }
                    _ => None,
                };
                if let Some(color) = color {
                    *style = if param == Some(38) {
                        style.fg(color)
                    } else {
                        style.bg(color)
                    };
                }
            }
            _ => {}
        }
    }
}

fn ansi_color(index: u16, bright: bool) -> Color {
    match (index, bright) {
        (0, false) => Color::Black,
        (1, false) => Color::Red,
        (2, false) => Color::Green,
        (3, false) => Color::Yellow,
        (4, false) => Color::Blue,
        (5, false) => Color::Magenta,
        (6, false) => Color::Cyan,
        (7, false) => Color::Gray,
        (0, true) => Color::DarkGray,
        (1, true) => Color::LightRed,
        (2, true) => Color::LightGreen,
        (3, true) => Color::LightYellow,
        (4, true) => Color::LightBlue,
        (5, true) => Color::LightMagenta,
        (6, true) => Color::LightCyan,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use super::*;

    fn argv(text: &str) -> Argv {
        vec![text.to_owned()]
    }

    fn wait_until(mut ready: impl FnMut() -> bool) {
        let start = Instant::now();
        while !ready() {
            assert!(start.elapsed() < Duration::from_secs(10), "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for(previewer: &mut Previewer, want: &Content) {
        wait_until(|| {
            previewer.update();
            previewer.content().as_ref() == Some(want)
        });
    }

    fn text(value: impl AsRef<str>) -> Content {
        Content::Text(styled(value.as_ref()))
    }

    fn runner(f: impl Fn(Argv) -> Outcome + Send + Sync + 'static) -> Run {
        let f = Arc::new(f);
        Arc::new(move |argv, _size| {
            let f = Arc::clone(&f);
            Box::pin(async move { f(argv) })
        })
    }

    #[test]
    fn command_receives_the_preview_pane_size() {
        let mut background = Background::start().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        background.spawn(async move {
            let result = commands()(vec!["/usr/bin/env".into()], (37, 12)).await;
            let _ = sender.send(result);
        });
        let output = receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        assert!(output.lines().any(|line| line == "COLUMNS=37"));
        assert!(output.lines().any(|line| line == "LINES=12"));
    }

    #[test]
    fn a_failure_without_stderr_names_the_command_and_its_status() {
        let mut background = Background::start().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        background.spawn(async move {
            let failed = commands()(vec!["/usr/bin/false".into()], (1, 1)).await;
            let empty = commands()(vec!["/usr/bin/true".into()], (1, 1)).await;
            let _ = sender.send((failed, empty));
        });
        let (failed, empty) = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            failed,
            Err("/usr/bin/false failed (exit status: 1)".to_owned())
        );
        assert_eq!(empty, Ok(String::new()), "empty output is a success");
    }

    #[test]
    fn replacement_and_resize_force_a_fresh_debounced_preview() {
        let mut background = Background::start().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let run: Run = Arc::new(move |_argv, size| {
            let captured = Arc::clone(&captured);
            Box::pin(async move {
                let mut seen = captured.lock().unwrap();
                seen.push(size);
                Ok(format!("version {}", seen.len()))
            })
        });
        let mut previewer = Previewer::spawn(&mut background, run, None);
        let command = argv("same command");
        previewer.set_size((30, 8));
        previewer.select(Some(&command));
        wait_for(&mut previewer, &text("version 1"));
        previewer.invalidate(); // successful live row replacement
        previewer.select(Some(&command));
        assert_eq!(previewer.content(), Some(Content::Loading));
        wait_for(&mut previewer, &text("version 2"));
        previewer.set_size((50, 10));
        previewer.select(Some(&command));
        assert_eq!(previewer.content(), Some(Content::Loading));
        wait_for(&mut previewer, &text("version 3"));
        assert_eq!(*seen.lock().unwrap(), [(30, 8), (30, 8), (50, 10)]);
    }

    #[test]
    fn escape_sequences_and_controls_cannot_reach_the_screen() {
        let text = "\u{1b}[31mred\u{1b}[0m\r\n\u{1b}]0;title\u{7}tab\there\u{1b}_rex1;1;{}\u{1b}\\\u{8}end";
        assert_eq!(sanitize(text), "red\ntab    here\u{fffd}end");
        assert_eq!(
            styled(text)
                .first()
                .unwrap()
                .spans
                .first()
                .unwrap()
                .style
                .fg,
            Some(Color::Red)
        );
    }

    #[test]
    fn sgr_styles_colors_and_resets() {
        let lines =
            styled("\u{1b}[1;2;3;4;7;31;44mA\u{1b}[22;23;24;27;39;49mB\u{1b}[91;104mC\u{1b}[mD");
        let spans = &lines.first().unwrap().spans;
        assert_eq!(spans.first().unwrap().content, "A");
        assert_eq!(spans.first().unwrap().style.fg, Some(Color::Red));
        assert_eq!(spans.first().unwrap().style.bg, Some(Color::Blue));
        assert!(spans.first().unwrap().style.add_modifier.contains(
            Modifier::BOLD
                | Modifier::DIM
                | Modifier::ITALIC
                | Modifier::UNDERLINED
                | Modifier::REVERSED
        ));
        assert_eq!(spans.get(1).unwrap().style.fg, None);
        assert_eq!(spans.get(1).unwrap().style.bg, None);
        assert!(!spans.get(1).unwrap().style.add_modifier.intersects(
            Modifier::BOLD
                | Modifier::DIM
                | Modifier::ITALIC
                | Modifier::UNDERLINED
                | Modifier::REVERSED
        ));
        assert_eq!(spans.get(2).unwrap().style.fg, Some(Color::LightRed));
        assert_eq!(spans.get(2).unwrap().style.bg, Some(Color::LightBlue));
        assert_eq!(spans.get(3).unwrap().style, Style::default());
    }

    #[test]
    fn extended_colors_unknown_parameters_and_unicode() {
        let lines = styled("é\u{1b}[999;38;5;196;48;2;27;124;131m一\u{1b}[38:2:1:2:3m👩‍💻\u{1b}[0m!");
        let spans = &lines.first().unwrap().spans;
        assert_eq!(spans.first().unwrap().content, "é");
        assert_eq!(spans.get(1).unwrap().content, "一");
        assert_eq!(spans.get(1).unwrap().style.fg, Some(Color::Indexed(196)));
        assert_eq!(
            spans.get(1).unwrap().style.bg,
            Some(Color::Rgb(27, 124, 131))
        );
        assert_eq!(spans.get(2).unwrap().content, "👩‍💻");
        assert_eq!(spans.get(2).unwrap().style.fg, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(spans.get(3).unwrap().style, Style::default());
    }

    #[test]
    fn untrusted_escapes_are_discarded() {
        let input = "a\u{1b}[2J\u{1b}[?25l\u{1b}]title\u{7}\u{1b}Pdata\u{1b}\\\u{1b}^pm\u{1b}\\\u{1b}Xsos\u{1b}\\\u{9b}2J\u{9d}osc\u{9c}b\u{1b}[38;2;1;2";
        assert_eq!(sanitize(input), "ab");
        assert_eq!(sanitize("a\r\nb\rc\n\tend\u{1b}"), "a\nb\nc\n    end");
    }

    #[test]
    fn only_the_current_selection_is_shown() {
        let mut background = Background::start().unwrap();
        let run = runner(|argv| Ok(format!("{} out", argv.join(" "))));
        let mut previewer = Previewer::spawn(&mut background, run, None);
        previewer.select(Some(&argv("a")));
        assert_eq!(previewer.content(), Some(Content::Loading));
        wait_for(&mut previewer, &text("a out"));
        previewer.select(Some(&argv("b")));
        assert_eq!(
            previewer.content(),
            Some(Content::Loading),
            "never another row's preview"
        );
        wait_for(&mut previewer, &text("b out"));
        previewer.select(None);
        assert_eq!(previewer.content(), None);
    }

    #[test]
    fn failures_are_shown_as_failures() {
        let mut background = Background::start().unwrap();
        let run = runner(|_| Err("no such block".into()));
        let mut previewer = Previewer::spawn(&mut background, run, None);
        previewer.select(Some(&argv("x")));
        wait_for(&mut previewer, &Content::Failed("no such block".into()));
    }

    #[test]
    fn a_new_selection_cancels_a_preview_still_running() {
        let mut background = Background::start().unwrap();
        let cancelled = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&cancelled);
        let began = Arc::clone(&started);
        let run: Run = Arc::new(move |argv: Argv, _size| {
            let count = Arc::clone(&count);
            let began = Arc::clone(&began);
            Box::pin(async move {
                if argv == ["slow"] {
                    struct Cancelled(Arc<AtomicUsize>);
                    impl Drop for Cancelled {
                        fn drop(&mut self) {
                            self.0.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                    let _cancelled = Cancelled(count);
                    began.fetch_add(1, Ordering::SeqCst);
                    std::future::pending::<()>().await;
                }
                Ok(argv.join(" "))
            })
        });
        let mut previewer = Previewer::spawn(&mut background, run, None);
        previewer.select(Some(&argv("slow")));
        wait_until(|| started.load(Ordering::SeqCst) == 1);
        previewer.select(Some(&argv("fast")));
        wait_for(&mut previewer, &text("fast"));
        assert_eq!(cancelled.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn rapid_selection_changes_do_not_start_commands_for_passed_rows() {
        let mut background = Background::start().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&calls);
        let run = runner(move |argv| {
            seen.lock().unwrap().push(argv.clone());
            Ok(argv.join(" "))
        });
        let mut previewer = Previewer::spawn(&mut background, run, None);
        previewer.select(Some(&argv("passed a")));
        std::thread::sleep(Duration::from_millis(25));
        previewer.select(Some(&argv("passed b")));
        std::thread::sleep(Duration::from_millis(25));
        previewer.select(Some(&argv("settled")));
        wait_for(&mut previewer, &text("settled"));
        assert_eq!(*calls.lock().unwrap(), vec![argv("settled")]);
    }

    #[test]
    fn cached_success_is_immediate_and_does_not_run_again_without_interval() {
        let mut background = Background::start().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let run = runner(move |argv| {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(argv.join(" "))
        });
        let mut previewer = Previewer::spawn(&mut background, run, None);
        previewer.select(Some(&argv("cached")));
        wait_for(&mut previewer, &text("cached"));
        previewer.select(None);
        previewer.select(Some(&argv("cached")));
        assert_eq!(previewer.content(), Some(text("cached")));
        std::thread::sleep(DEBOUNCE + Duration::from_millis(80));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cached_success_with_interval_shows_immediately_then_runs_again() {
        let mut background = Background::start().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let run = runner(move |_| {
            let n = seen.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(format!("run {n}"))
        });
        let mut previewer = Previewer::spawn(&mut background, run, Some(Duration::from_secs(5)));
        previewer.select(Some(&argv("cached")));
        wait_for(&mut previewer, &text("run 1"));
        previewer.select(None);
        previewer.select(Some(&argv("cached")));
        assert_eq!(previewer.content(), Some(text("run 1")));
        wait_for(&mut previewer, &text("run 2"));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cached_failure_is_immediate_but_retried() {
        let mut background = Background::start().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let run = runner(move |_| {
            let n = seen.fetch_add(1, Ordering::SeqCst) + 1;
            Err(format!("failure {n}"))
        });
        let mut previewer = Previewer::spawn(&mut background, run, None);
        previewer.select(Some(&argv("failed")));
        wait_for(&mut previewer, &Content::Failed("failure 1".into()));
        previewer.select(None);
        previewer.select(Some(&argv("failed")));
        assert_eq!(
            previewer.content(),
            Some(Content::Failed("failure 1".into()))
        );
        wait_for(&mut previewer, &Content::Failed("failure 2".into()));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cache_evicts_least_recently_used_command() {
        let mut background = Background::start().unwrap();
        let run = runner(|argv| Ok(argv.join(" ")));
        let mut previewer = Previewer::spawn(&mut background, run, None);
        for n in 0..CACHE_LIMIT {
            let command = argv(&format!("row {n}"));
            previewer.select(Some(&command));
            wait_for(&mut previewer, &text(format!("row {n}")));
        }
        previewer.select(Some(&argv("row 0")));
        assert_eq!(previewer.content(), Some(text("row 0")));
        previewer.select(Some(&argv("extra")));
        wait_for(&mut previewer, &text("extra"));
        assert_eq!(previewer.cache.len(), CACHE_LIMIT);
        previewer.select(Some(&argv("row 1")));
        assert_eq!(previewer.content(), Some(Content::Loading));
        previewer.select(Some(&argv("row 0")));
        assert_eq!(previewer.content(), Some(text("row 0")));
    }

    #[test]
    fn an_interval_reruns_the_selection_after_each_answer() {
        let mut background = Background::start().unwrap();
        let runs = Arc::new(Mutex::new(0));
        let count = Arc::clone(&runs);
        let run = runner(move |_| {
            let mut runs = count.lock().unwrap();
            *runs += 1;
            Ok(format!("run {runs}"))
        });
        let mut previewer = Previewer::spawn(&mut background, run, Some(Duration::from_millis(20)));
        previewer.select(Some(&argv("screen")));
        wait_for(&mut previewer, &text("run 3"));

        let still_runs = Arc::new(Mutex::new(0));
        let count = Arc::clone(&still_runs);
        let run = runner(move |_| {
            *count.lock().unwrap() += 1;
            Ok(String::new())
        });
        let mut still = Previewer::spawn(&mut background, run, None);
        still.select(Some(&argv("screen")));
        wait_for(&mut still, &text(""));
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(*still_runs.lock().unwrap(), 1, "no interval, no rerun");
    }
}
