//! The terminal half: draw, read a key, repeat.
//!
//! The key mapping is in [`super::keys`] and the list is in [`super::model`],
//! so what remains here is init, layout, the read loop, and the progress of a
//! chosen action. Asynchronous work (the live feed, previews, and actions)
//! runs on one [`Background`] runtime.

use std::io;
use std::thread;
use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEvent,
};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tokio::sync::oneshot;

use crate::background::Background;
use crate::live::{self, Feed, Notice};
use crate::model::{Model, Outcome};
use crate::preview::{self, Content, Previewer};
use crate::spec::{PreviewAnchor, PreviewPosition};
use crate::{Acting, Action, Actions, Choice, Entry, Error, Palette, Shortcut, columns, keys};

/// What Enter does, in the help and while its action runs.
const ACCEPT_LABEL: &str = "open";
/// An action's error wraps to at most this many lines.
const ALERT_LINES: usize = 3;

#[derive(Debug)]
enum Exit {
    /// The list ended without an action: Enter on a row, or an abort.
    List(Outcome),
    /// An action succeeded.
    Done,
    /// A background job panicked; shutting down reports it.
    Failed,
}

/// What keeps a running picker current: its live feed and how often the
/// selected preview re-runs; and how the preview is shown.
pub(crate) struct Live<T> {
    pub(crate) feed: Option<(live::Trigger, live::Fetch<T>)>,
    pub(crate) preview_every: Option<Duration>,
    pub(crate) preview_layout: PreviewLayout,
}

/// Where the preview pane sits and which end of its output it shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PreviewLayout {
    pub(crate) anchor: PreviewAnchor,
    pub(crate) position: PreviewPosition,
}

impl<T> Default for Live<T> {
    fn default() -> Self {
        Self {
            feed: None,
            preview_every: None,
            preview_layout: PreviewLayout::default(),
        }
    }
}

impl<T> std::fmt::Debug for Live<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Live")
            .field("feed", &self.feed.is_some())
            .field("preview_every", &self.preview_every)
            .field("preview_layout", &self.preview_layout)
            .finish()
    }
}

/// How a picker ended.
#[derive(Debug)]
pub(crate) enum Ended<T> {
    /// Enter chose a row; only without actions.
    Chosen(T),
    /// An action succeeded.
    Acted,
    /// The user closed the picker without choosing.
    Cancelled,
}

/// Runs the picker. Without actions, Enter on a row returns it; with them,
/// Enter and shortcuts run them in place, and the picker ends once one succeeds.
pub(crate) fn run<T: Entry + Send + 'static, A>(
    model: Model<T>,
    prompt: &str,
    actions: Option<&Actions<T, A>>,
    live: Live<T>,
) -> Result<Ended<T>, Error> {
    let mut background = Background::start()?;
    let mut page = Page {
        model,
        prompt: prompt.to_owned(),
        live,
    };
    let session = session(&mut background, &mut page, actions);
    // The terminal is restored; now stop the jobs.
    let exit = conclude(session, background.shutdown())?;
    Ok(match exit {
        Exit::List(Outcome::Accept) => Ended::Chosen(
            page.model
                .take_selected()
                .expect("the model accepts only with a row selected"),
        ),
        Exit::List(Outcome::Abort) => Ended::Cancelled,
        Exit::Done => Ended::Acted,
        Exit::Failed => unreachable!("shutdown resumes the failed job's panic"),
    })
}

/// A job's panic outranks whatever the terminal session returned: its
/// selection may have been made while the job was failing, and an error
/// restoring the terminal is secondary to the defect.
fn conclude(session: Result<Exit, Error>, jobs: thread::Result<()>) -> Result<Exit, Error> {
    if let Err(panic) = jobs {
        std::panic::resume_unwind(panic);
    }
    session
}

/// Takes the terminal, runs the loop, and gives the terminal back, whatever
/// the loop returned.
struct Page<T: Entry> {
    model: Model<T>,
    prompt: String,
    live: Live<T>,
}

impl<T: Entry + Send + 'static> Page<T> {
    fn start(&self, background: &mut Background) -> (Option<Previewer>, Option<Feed<T>>) {
        let preview = (self.model.has_previews() || self.live.feed.is_some())
            .then(|| Previewer::spawn(background, preview::commands(), self.live.preview_every));
        let feed = self.live.feed.as_ref().map(|(trigger, fetch)| {
            let watch = match trigger {
                live::Trigger::Command(argv) => live::Watch::Command(argv.clone()),
                live::Trigger::Interval(period) => live::Watch::Interval(*period),
            };
            Feed::spawn(
                background,
                live::Spec {
                    watch,
                    fetch: fetch.clone(),
                },
            )
        });
        (preview, feed)
    }

    fn navigate(&mut self, parents: &mut Vec<Self>, action: Action<T>) -> Result<bool, Error> {
        match action {
            Action::Push(picker) => {
                let next = Self::from_picker(picker)?;
                parents.push(std::mem::replace(self, next));
            }
            Action::Replace(picker) => *self = Self::from_picker(picker)?,
            Action::Back => match parents.pop() {
                Some(previous) => *self = previous,
                None => return Ok(false),
            },
            Action::Close | Action::Stay => return Ok(false),
        }
        Ok(true)
    }

    fn from_picker(picker: crate::Picker<T>) -> Result<Self, Error> {
        let (model, prompt, live) = picker.prepare()?;
        Ok(Self {
            model,
            prompt,
            live,
        })
    }
}

fn session<T: Entry + Send + 'static, A>(
    background: &mut Background,
    page: &mut Page<T>,
    actions: Option<&Actions<T, A>>,
) -> Result<Exit, Error> {
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(error.into());
        }
    };
    let paste = match BracketedPaste::enable() {
        Ok(paste) => paste,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(error.into());
        }
    };
    let outcome = drive(&mut terminal, background, page, actions);
    let paste_restored = paste.disable();
    let restored = ratatui::try_restore();
    let outcome = outcome?;
    paste_restored?;
    restored?;
    Ok(outcome)
}

#[expect(
    clippy::too_many_lines,
    reason = "One loop owns page navigation, input and background resource lifetimes."
)]
fn drive<T: Entry + Send + 'static, A>(
    terminal: &mut ratatui::DefaultTerminal,
    background: &mut Background,
    current: &mut Page<T>,
    actions: Option<&Actions<T, A>>,
) -> Result<Exit, Error> {
    let shortcuts = actions.map_or(&[][..], |actions| actions.shortcuts.as_slice());
    let mut status = Status::default();
    let mut parents = Vec::new();
    let (mut previewer, mut feed) = current.start(background);
    let mut dirty = true;
    let mut page_size = 1;
    loop {
        if background.failed() {
            return Ok(Exit::Failed);
        }
        let mut navigation = None;
        match status.settle() {
            Settled::Unchanged => {}
            Settled::Done => {
                discard_input()?;
                return Ok(Exit::Done);
            }
            Settled::Failed => {
                discard_input()?;
                dirty = true;
            }
            Settled::Navigate => navigation = status.navigation.take(),
        }
        if let Some(action) = navigation {
            match current.navigate(&mut parents, action) {
                Ok(true) => {
                    drop(previewer.take());
                    drop(feed.take());
                    (previewer, feed) = current.start(background);
                }
                Ok(false) => {}
                Err(error) => {
                    discard_input()?;
                    status.alert = Some(error.to_string());
                }
            }
            dirty = true;
        }
        if let Some(feed) = feed.as_mut() {
            dirty |= feed.update(&mut current.model);
            if feed.take_replaced()
                && let Some(previewer) = previewer.as_mut()
            {
                previewer.refresh();
            }
        }
        if let Some(previewer) = previewer.as_mut() {
            let size = terminal.size()?;
            previewer.set_size(preview_size(
                Rect::new(0, 0, size.width, size.height),
                !shortcuts.is_empty(),
                status.alert.as_deref(),
                current.live.preview_layout.position,
            ));
            previewer.select(current.model.selected_entry().and_then(Entry::preview));
            dirty |= previewer.update();
        }
        if dirty {
            let preview = previewer.as_ref().and_then(Previewer::content);
            let chrome = Chrome {
                prompt: &current.prompt,
                shortcuts,
                notice: feed.as_ref().and_then(Feed::notice),
                status: &status,
                preview_layout: current.live.preview_layout,
                preview_offset: previewer.as_ref().and_then(|preview| preview.offset),
            };
            terminal.draw(|frame| {
                page_size = draw(frame, &mut current.model, &chrome, preview.as_ref());
            })?;
            dirty = false;
        }
        if status.running.is_some() {
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }
        if !event::poll(Duration::from_millis(25))? {
            continue;
        }
        dirty = true;
        match event::read()? {
            Event::Key(key) => {
                if scroll_preview(key, previewer.as_mut(), current.live.preview_layout.anchor) {
                    continue;
                }
                if key.kind != ratatui::crossterm::event::KeyEventKind::Release
                    && key.code == ratatui::crossterm::event::KeyCode::Esc
                    && status.running.is_none()
                    && !parents.is_empty()
                {
                    status.navigation = Some(Action::Back);
                    continue;
                }
                if let Some(exit) =
                    status.key(key, page_size, &mut current.model, actions, background)
                {
                    return Ok(exit);
                }
            }
            Event::Paste(text) => status.paste(&text, &mut current.model),
            Event::Mouse(mouse) if status.running.is_none() => {
                let size = terminal.size()?;
                pointer(
                    mouse,
                    Rect::new(0, 0, size.width, size.height),
                    current,
                    previewer.as_mut(),
                    !shortcuts.is_empty(),
                    status.alert.as_deref(),
                );
            }
            _ => {}
        }
    }
}

fn discard_input() -> io::Result<()> {
    for _ in 0..4096 {
        if !event::poll(Duration::ZERO)? {
            break;
        }
        event::read()?;
    }
    Ok(())
}

fn scroll_preview(
    key: ratatui::crossterm::event::KeyEvent,
    preview: Option<&mut Previewer>,
    anchor: PreviewAnchor,
) -> bool {
    use ratatui::crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
    if key.modifiers != KeyModifiers::SHIFT || key.kind == KeyEventKind::Release {
        return false;
    }
    let Some(preview) = preview else {
        return false;
    };
    let step = match key.code {
        KeyCode::Up => -1,
        KeyCode::Down => 1,
        KeyCode::PageUp => -isize::try_from(preview.size().1).expect("terminal height fits isize"),
        KeyCode::PageDown => isize::try_from(preview.size().1).expect("terminal height fits isize"),
        _ => return false,
    };
    preview.scroll(step, anchor);
    true
}

fn pointer<T: Entry>(
    mouse: ratatui::crossterm::event::MouseEvent,
    area: Rect,
    current: &mut Page<T>,
    previewer: Option<&mut Previewer>,
    has_shortcuts: bool,
    alert: Option<&str>,
) {
    use ratatui::crossterm::event::{MouseButton, MouseEventKind};
    let body = regions(area, has_shortcuts, alert).body;
    let split = previewer
        .as_deref()
        .and_then(Previewer::content)
        .and_then(|_| split(body, current.live.preview_layout.position));
    let list = split.as_ref().map_or(body, |split| split.list);
    let point = ratatui::layout::Position::new(mouse.column, mouse.row);
    let step = match mouse.kind {
        MouseEventKind::ScrollUp => -3,
        MouseEventKind::ScrollDown => 3,
        _ => 0,
    };
    if list.contains(point) {
        if step != 0 {
            let command = if step < 0 {
                crate::Command::Previous(3)
            } else {
                crate::Command::Next(3)
            };
            current.model.apply(command);
        } else if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            current.model.select_line(usize::from(mouse.row - list.y));
        }
    } else if step != 0
        && split.is_some_and(|split| split.pane.contains(point))
        && let Some(preview) = previewer
    {
        preview.scroll(step, current.live.preview_layout.anchor);
    }
}

/// A chosen action's progress: running, or why the last one failed.
#[derive(Debug)]
struct Status<T: Entry> {
    running: Option<Running<T>>,
    navigation: Option<Action<T>>,
    /// Shown until the query changes or another action starts.
    alert: Option<String>,
}

#[derive(Debug)]
struct Running<T: Entry> {
    label: String,
    outcome: oneshot::Receiver<Result<Action<T>, String>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Settled {
    Unchanged,
    Done,
    Failed,
    Navigate,
}

impl<T: Entry> Default for Status<T> {
    fn default() -> Self {
        Self {
            running: None,
            navigation: None,
            alert: None,
        }
    }
}

impl<T: Entry + Send + 'static> Status<T> {
    /// Handles a key: an edit, a movement, an exit, or the start of an
    /// action. While an action runs the choice is made, so keys are ignored
    /// until it finishes.
    fn key<A>(
        &mut self,
        key: KeyEvent,
        page: usize,
        model: &mut Model<T>,
        actions: Option<&Actions<T, A>>,
        background: &mut Background,
    ) -> Option<Exit> {
        if self.running.is_some() {
            return None;
        }
        if let Some(actions) = actions
            && let Some(shortcut) = actions.shortcuts.iter().find(|s| s.matches(key))
        {
            match shortcut.chosen(model) {
                Some(chosen) => {
                    let work = (actions.act)(Choice::Shortcut(&shortcut.value, chosen));
                    self.start(shortcut.label(), work, background);
                }
                None => self.alert = Some(shortcut.needs_input()),
            }
            return None;
        }
        let command = keys::command(key, page)?;
        let before = model.query().to_owned();
        let outcome = model.apply(command);
        if model.query() != before {
            self.alert = None;
        }
        match (outcome?, actions) {
            (Outcome::Accept, Some(actions)) => {
                let row = model
                    .selected_entry()
                    .expect("the model accepts only with a row selected");
                let work = (actions.act)(Choice::Accept(row));
                self.start(ACCEPT_LABEL, work, background);
                None
            }
            (outcome, _) => Some(Exit::List(outcome)),
        }
    }

    fn paste(&mut self, text: &str, model: &mut Model<T>) {
        if self.running.is_some() {
            return;
        }
        let before = model.query().len();
        model.insert_text(text);
        if model.query().len() != before {
            self.alert = None;
        }
    }

    fn start(&mut self, label: &str, work: Acting<T>, background: &mut Background) {
        let (sender, outcome) = oneshot::channel();
        background.spawn(async move {
            let _ = sender.send(work.await);
        });
        self.alert = None;
        self.running = Some(Running {
            label: label.to_owned(),
            outcome,
        });
    }

    /// Takes the running action's outcome once it has one: success ends the
    /// picker, and a failure becomes the alert.
    fn settle(&mut self) -> Settled {
        if self.navigation.is_some() {
            return Settled::Navigate;
        }
        let Some(running) = &mut self.running else {
            return Settled::Unchanged;
        };
        match running.outcome.try_recv() {
            Ok(Ok(Action::Close)) => Settled::Done,
            Ok(Ok(action)) => {
                self.running = None;
                self.navigation = Some(action);
                Settled::Navigate
            }
            Ok(Err(message)) => {
                let message = if message.trim().is_empty() {
                    format!("{} failed", running.label)
                } else {
                    message
                };
                self.running = None;
                self.alert = Some(message);
                Settled::Failed
            }
            // Work that ends without an outcome panicked, which
            // `Background::failed` reports.
            Err(_) => Settled::Unchanged,
        }
    }
}

/// Restores bracketed paste even when drawing or reading fails. Keeping this
/// separate from ratatui's terminal guard matters because ratatui does not
/// enable or disable this crossterm mode itself.
struct BracketedPaste {
    enabled: bool,
}

impl BracketedPaste {
    fn enable() -> io::Result<Self> {
        ratatui::crossterm::execute!(
            io::stdout(),
            EnableBracketedPaste,
            ratatui::crossterm::event::EnableMouseCapture
        )?;
        Ok(Self { enabled: true })
    }

    fn disable(mut self) -> io::Result<()> {
        ratatui::crossterm::execute!(
            io::stdout(),
            DisableBracketedPaste,
            ratatui::crossterm::event::DisableMouseCapture
        )?;
        self.enabled = false;
        Ok(())
    }
}

impl Drop for BracketedPaste {
    fn drop(&mut self) {
        if self.enabled {
            let _ = ratatui::crossterm::execute!(
                io::stdout(),
                DisableBracketedPaste,
                ratatui::crossterm::event::DisableMouseCapture
            );
        }
    }
}

/// Everything a frame shows besides the list and the preview.
struct Chrome<'a, A, T: Entry> {
    prompt: &'a str,
    shortcuts: &'a [Shortcut<A>],
    notice: Option<&'a Notice>,
    status: &'a Status<T>,
    preview_layout: PreviewLayout,
    preview_offset: Option<usize>,
}

/// Returns the page size, so a page key moves by exactly what is on screen.
fn draw<T: Entry, A>(
    frame: &mut Frame<'_>,
    model: &mut Model<T>,
    chrome: &Chrome<'_, A, T>,
    preview: Option<&Content>,
) -> usize {
    let palette = model.palette();
    let area = frame.area();
    frame.render_widget(
        Paragraph::default().style(Style::default().bg(palette.background)),
        area,
    );
    let alert_text = chrome.status.alert.as_deref();
    let Regions {
        roomy,
        head,
        rule,
        alert,
        help,
        body,
    } = regions(area, !chrome.shortcuts.is_empty(), alert_text);
    let width = usize::from(head.width);
    if roomy {
        let muted = Style::default().fg(palette.muted).bg(palette.background);
        // An action's error outranks the feed's notice while it is shown.
        let notice = chrome.notice.filter(|_| alert_text.is_none());
        let running = chrome.status.running.as_ref().map(|r| r.label.as_str());
        frame.render_widget(
            Paragraph::new(rule_line(
                model.scope_label(),
                running,
                notice,
                palette,
                width,
            ))
            .style(muted),
            rule,
        );
        let lines = help_lines(chrome.shortcuts, &model.scope_help(), palette);
        frame.render_widget(Paragraph::new(lines).style(muted), help);
    }
    if let Some(text) = alert_text {
        let lines: Vec<Line<'static>> = alert_lines(text, usize::from(alert.width))
            .into_iter()
            .map(Line::raw)
            .collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(palette.red).bg(palette.background)),
            alert,
        );
    }
    let position = chrome.preview_layout.position;
    let (list, pane) = match (preview, split(body, position)) {
        (Some(content), Some(Split { list, rule, pane })) => {
            let lines = match position {
                PreviewPosition::Right => vec![Line::raw(" │ "); usize::from(rule.height)],
                PreviewPosition::Below => vec![Line::raw("─".repeat(usize::from(rule.width)))],
            };
            let muted = Style::default().fg(palette.muted).bg(palette.background);
            frame.render_widget(
                Paragraph::new(lines).style(muted.add_modifier(Modifier::DIM)),
                rule,
            );
            (list, Some((pane, content)))
        }
        _ => (body, None),
    };

    let height = usize::from(list.height);
    model.scroll_into_view(height);
    frame.render_widget(
        Paragraph::new(model.lines(usize::from(list.width), height))
            .style(Style::default().bg(palette.background)),
        list,
    );
    if let Some((pane, content)) = pane {
        frame.render_widget(
            preview_paragraph(
                content,
                palette,
                pane,
                chrome.preview_layout.anchor,
                chrome.preview_offset,
            ),
            pane,
        );
    }
    let caret = columns::width(chrome.prompt) + model.caret_cells();
    let (prompt_line, input_width) = prompt_line(model, chrome.prompt, palette, width);
    frame.render_widget(
        Paragraph::new(prompt_line).style(Style::default().bg(palette.background)),
        head,
    );

    let caret = caret.min(input_width.saturating_sub(1));
    let caret = u16::try_from(caret).expect("the caret is clamped to the terminal width");
    let x = head.x.saturating_add(caret);
    if head.width > 0 && head.height > 0 {
        frame.set_cursor_position((x.min(head.right().saturating_sub(1)), head.y));
    }

    height
}

/// Where each part of the picker goes: the prompt, a rule carrying the
/// scope, an action's error, the help, then the list. The error and the help
/// sit above the list, so a host that draws less than the terminal's full
/// height clips list rows rather than them.
struct Regions {
    roomy: bool,
    head: Rect,
    rule: Rect,
    alert: Rect,
    help: Rect,
    body: Rect,
}

fn regions(area: Rect, has_shortcuts: bool, alert: Option<&str>) -> Regions {
    let roomy = area.height >= 9 && area.width >= 12;
    let content = if roomy {
        area.inner(Margin::new(1, 0))
    } else {
        area
    };
    let chrome = u16::from(roomy);
    // The view's own shortcuts get a help line of their own, first, so they
    // stay visible however long the generic help is.
    let help_height = 1 + u16::from(has_shortcuts);
    let alert_height = alert.map_or(0, |text| {
        alert_lines(text, usize::from(content.width)).len()
    });
    let alert_height = u16::try_from(alert_height).expect("an alert has a few lines");
    let [_, head, rule, alert, help, body] = Layout::vertical([
        Constraint::Length(chrome),
        Constraint::Length(1),
        Constraint::Length(chrome),
        Constraint::Length(alert_height),
        Constraint::Length(help_height * chrome),
        Constraint::Min(0),
    ])
    .areas(content);
    Regions {
        roomy,
        head,
        rule,
        alert,
        help,
        body,
    }
}

/// `text` as at most [`ALERT_LINES`] lines of `width` cells, broken between
/// words where it can and ending in `…` when cut short. Host text passes
/// through [`preview::sanitize`], so it cannot reach the terminal as escapes.
fn alert_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for paragraph in preview::sanitize(text).lines() {
        let mut line = String::new();
        for mut word in paragraph.split_whitespace() {
            while !word.is_empty() {
                let gap = usize::from(!line.is_empty());
                if columns::width(&line) + gap + columns::width(word) <= width {
                    if gap == 1 {
                        line.push(' ');
                    }
                    line.push_str(word);
                    break;
                }
                if line.is_empty() {
                    let (head, rest) = columns::split(word, width);
                    lines.push(head.to_owned());
                    word = rest;
                } else {
                    lines.push(std::mem::take(&mut line));
                }
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        if lines.len() > ALERT_LINES {
            break;
        }
    }
    if lines.len() > ALERT_LINES {
        lines.truncate(ALERT_LINES);
        if let Some(last) = lines.last_mut() {
            *last = columns::truncate(&format!("{last}…"), width).into_owned();
        }
    }
    lines
}

/// The help: the view's shortcuts on a line of their own with the keys in
/// the accent colour and the input a shortcut takes in italics, then the
/// generic keys.
fn help_lines<A>(
    shortcuts: &[Shortcut<A>],
    scopes: &[(String, String)],
    palette: Palette,
) -> Vec<Line<'static>> {
    let mut help = Vec::with_capacity(2);
    if !shortcuts.is_empty() {
        let key = Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD);
        let label = Style::default().fg(palette.strong);
        let hint = Style::default()
            .fg(palette.green)
            .add_modifier(Modifier::ITALIC);
        let mut spans = Vec::with_capacity(shortcuts.len() * 4);
        for (i, shortcut) in shortcuts.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::styled(shortcut.key_label(), key));
            spans.push(Span::styled(format!(" {}", shortcut.label()), label));
            if let Some(text) = shortcut.hint() {
                spans.push(Span::styled(format!(" ‹{text}›"), hint));
            }
        }
        help.push(Line::from(spans));
    }
    // Keys stand out from what they do; scope sigils take the match colour
    // because they are typed into the query.
    let key = Style::default().fg(palette.strong);
    let sigil = Style::default()
        .fg(palette.matched)
        .add_modifier(Modifier::BOLD);
    let what = Style::default().fg(palette.muted);
    let dot = what.add_modifier(Modifier::DIM);
    let generic = [
        ("enter", ACCEPT_LABEL),
        ("esc", "back/close"),
        ("shift+↑/↓", "preview"),
        ("↑/↓ ctrl+j/k", "move"),
        ("tab/shift+tab", "scope"),
        ("ctrl+u", "clear"),
    ]
    .map(|(keys, what)| (keys.to_owned(), what.to_owned(), key));
    let sigils = scopes
        .iter()
        .map(|(sigil_text, label)| (sigil_text.clone(), label.clone(), sigil));
    let mut spans = Vec::new();
    for (i, (keys, does, style)) in generic.into_iter().chain(sigils).enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", dot));
        }
        spans.push(Span::styled(keys, style));
        spans.push(Span::styled(format!(" {does}"), what));
    }
    help.push(Line::from(spans));
    help
}

/// The rule under the prompt, labelled with the scope and with the action
/// that is running or why the rows may be out of date: a failed refresh in
/// red, a stopped feed in yellow.
fn rule_line(
    label: &str,
    running: Option<&str>,
    notice: Option<&Notice>,
    palette: Palette,
    width: usize,
) -> Line<'static> {
    let dim = Style::default()
        .fg(palette.muted)
        .add_modifier(Modifier::DIM);
    let mut spans = vec![
        Span::styled("── ", dim),
        Span::styled("Scope · ", Style::default().fg(palette.muted)),
        Span::styled(
            label.to_owned(),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    let note = match (running, notice) {
        (Some(action), _) => Some((format!("running {action}…"), palette.muted)),
        (None, Some(Notice::Stale(message))) => {
            Some((format!("not refreshed: {message}"), palette.red))
        }
        (None, Some(Notice::Stopped(message))) => {
            Some((format!("not updating: {message}"), palette.yellow))
        }
        (None, None) => None,
    };
    if let Some((text, colour)) = note {
        spans.push(Span::raw(" · "));
        spans.push(Span::styled(
            preview::sanitize(&text).replace('\n', " "),
            Style::default().fg(colour),
        ));
    }
    spans.push(Span::raw(" "));
    let used: usize = spans.iter().map(|span| columns::width(&span.content)).sum();
    spans.push(Span::styled("─".repeat(width.saturating_sub(used)), dim));
    Line::from(spans)
}

/// Narrower than this, the list needs every column it has, so a preview on
/// the right is hidden.
const MIN_PREVIEW_WIDTH: u16 = 80;
/// A preview below the list leaves the list at least this many rows.
const MIN_LIST_ROWS: u16 = 5;
/// Shorter than this, a preview below the list is hidden.
const MIN_BELOW_PREVIEW_ROWS: u16 = 5;
/// The share of the body, in percent, that the list gets above a preview.
const BELOW_LIST_PERCENT: u32 = 40;

/// The body divided between the list, the rule, and the preview pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Split {
    list: Rect,
    rule: Rect,
    pane: Rect,
}

/// The one split of the body that drawing and preview sizes share, or
/// `None` when the body has no room for a preview at `position`.
///
/// Below the list, the list gets a fixed share rather than the rows it
/// needs, so the pane keeps its size while the query filters rows: a
/// resize would invalidate cached previews and re-run the command.
fn split(body: Rect, position: PreviewPosition) -> Option<Split> {
    let [list, rule, pane] = match position {
        PreviewPosition::Right => {
            if body.width < MIN_PREVIEW_WIDTH {
                return None;
            }
            Layout::horizontal([
                Constraint::Percentage(55),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .areas(body)
        }
        PreviewPosition::Below => {
            if body.height < MIN_LIST_ROWS + 1 + MIN_BELOW_PREVIEW_ROWS {
                return None;
            }
            let share = u32::from(body.height) * BELOW_LIST_PERCENT / 100;
            let share = u16::try_from(share).expect("a share of the body fits its height");
            Layout::vertical([
                Constraint::Length(share.max(MIN_LIST_ROWS)),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(body)
        }
    };
    Some(Split { list, rule, pane })
}

/// Match the draw layout: commands receive the pane's current inner cells.
fn preview_size(
    area: Rect,
    has_shortcuts: bool,
    alert: Option<&str>,
    position: PreviewPosition,
) -> (u16, u16) {
    let body = regions(area, has_shortcuts, alert).body;
    split(body, position).map_or((0, 0), |split| (split.pane.width, split.pane.height))
}

/// Show the beginning of a document or the live end of a terminal screen.
fn preview_paragraph(
    content: &Content,
    palette: Palette,
    pane: Rect,
    anchor: PreviewAnchor,
    offset: Option<usize>,
) -> Paragraph<'static> {
    let width = usize::from(pane.width);
    let height = usize::from(pane.height);
    let (text, color) = match content {
        Content::Loading => (vec![Line::raw("loading…")], palette.muted),
        Content::Text(lines) => (lines.clone(), palette.strong),
        Content::Failed(message) => (message.lines().map(Line::raw).collect(), palette.red),
    };
    let start = offset
        .unwrap_or_else(|| match anchor {
            PreviewAnchor::Start => 0,
            PreviewAnchor::End => text.len().saturating_sub(height),
        })
        .min(text.len().saturating_sub(height));
    let shown = text
        .into_iter()
        .skip(start)
        .take(height)
        .map(|line| {
            let mut left = width;
            let spans = line
                .spans
                .into_iter()
                .map_while(|span| {
                    if left == 0 {
                        return None;
                    }
                    let content = columns::truncate(&span.content, left).into_owned();
                    left = left.saturating_sub(columns::width(&content));
                    Some(Span::styled(content, span.style))
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    Paragraph::new(shown).style(Style::default().fg(color).bg(palette.background))
}

fn prompt_line<T: Entry>(
    model: &Model<T>,
    prompt: &str,
    palette: Palette,
    width: usize,
) -> (Line<'static>, usize) {
    let count = format!("{}/{}", model.matched(), model.candidates());
    let prompt_width = columns::width(prompt);
    let count_width = columns::width(&count);
    // The count is secondary to an editable prompt. Only reserve it when the
    // complete prompt and the separating cell fit as well.
    let show_count = prompt_width.saturating_add(1).saturating_add(count_width) <= width;
    let input_width = if show_count {
        width.saturating_sub(count_width + 1)
    } else {
        width
    };

    let shown_prompt = columns::truncate(prompt, input_width);
    let query_width = input_width.saturating_sub(columns::width(&shown_prompt));
    let shown_query = columns::truncate(model.query(), query_width);
    let used = columns::width(&shown_prompt) + columns::width(&shown_query);
    let padding = input_width.saturating_sub(used) + usize::from(show_count);

    let mut spans = vec![
        Span::styled(shown_prompt.into_owned(), Style::default().fg(palette.blue)),
        Span::styled(
            shown_query.into_owned(),
            Style::default().fg(palette.strong),
        ),
        Span::raw(" ".repeat(padding)),
    ];
    if show_count {
        spans.push(Span::styled(count, Style::default().fg(palette.muted)));
    }
    (Line::from(spans), input_width)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::{Backend as _, TestBackend};
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::style::Color;

    use super::*;
    use crate::{Cell, Column, Group, Picker};

    #[derive(Debug)]
    struct Item;

    impl Entry for Item {
        type Group = ();

        fn group(&self) {}

        fn cells(&self, _palette: &Palette) -> Vec<Cell> {
            vec![Cell::new("something searchable", Color::White)]
        }
    }

    fn model() -> Model<Item> {
        Picker::new(vec![Item])
            .group(Group::new((), "items", vec![Column::fill()]))
            .build()
            .unwrap()
    }

    #[test]
    fn inspection_pages_keep_the_parent_query_and_selection_and_reject_invalid_pages() {
        let make = || Picker::new(vec![Item]).group(Group::new((), "items", vec![Column::fill()]));
        let mut current = Page::from_picker(make().prompt("Goals > ").query("search")).unwrap();
        let mut parents = Vec::new();
        assert!(
            current
                .navigate(&mut parents, Action::Push(make().prompt("Tasks > ")))
                .unwrap()
        );
        assert_eq!(current.prompt, "Tasks > ");
        assert_eq!(current.model.query(), "");
        assert_eq!(parents.len(), 1);
        assert!(!current.navigate(&mut parents, Action::Stay).unwrap());
        assert_eq!(parents.len(), 1);
        assert_eq!(current.prompt, "Tasks > ");
        let invalid = Picker::new(vec![Item]);
        assert!(
            current
                .navigate(&mut parents, Action::Push(invalid))
                .is_err()
        );
        assert_eq!(parents.len(), 1);
        assert_eq!(current.prompt, "Tasks > ");
        assert!(current.navigate(&mut parents, Action::Back).unwrap());
        assert_eq!(current.prompt, "Goals > ");
        assert_eq!(current.model.query(), "search");
        assert_eq!(current.model.selected(), Some(0));
        assert_eq!(parents.len(), 0);
        assert!(!current.navigate(&mut parents, Action::Back).unwrap());
    }

    /// One frame of `model` with the given chrome; returns the page size.
    fn render<A>(
        terminal: &mut Terminal<TestBackend>,
        model: &mut Model<Item>,
        shortcuts: &[Shortcut<A>],
        status: &Status<Item>,
        notice: Option<&Notice>,
        preview: Option<&Content>,
        preview_layout: PreviewLayout,
    ) -> usize {
        let chrome = Chrome {
            prompt: "› ",
            shortcuts,
            notice,
            status,
            preview_layout,
            preview_offset: None,
        };
        let mut page = 0;
        terminal
            .draw(|frame| page = draw(frame, model, &chrome, preview))
            .unwrap();
        page
    }

    /// A frame drawn with nothing but `shortcuts` and `status`.
    fn plain_frame<A>(
        terminal: &mut Terminal<TestBackend>,
        model: &mut Model<Item>,
        shortcuts: &[Shortcut<A>],
        status: &Status<Item>,
    ) {
        render(
            terminal,
            model,
            shortcuts,
            status,
            None,
            None,
            PreviewLayout::default(),
        );
    }

    fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        rows(terminal).join("\n")
    }

    fn review() -> Shortcut<()> {
        Shortcut::control('r', "review PR", ()).takes_line(Some("URL".to_owned()))
    }

    /// Actions whose every run returns `outcome`, counting the runs.
    fn acting(outcome: Result<(), String>) -> (Actions<Item, ()>, Arc<AtomicUsize>) {
        let runs = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&runs);
        let act: crate::Act<Item, ()> = Box::new(move |_choice| {
            counter.fetch_add(1, Ordering::SeqCst);
            let outcome = outcome.clone();
            Box::pin(async move { outcome.map(|()| Action::Close) })
        });
        let actions = Actions {
            shortcuts: vec![review()],
            act,
        };
        (actions, runs)
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn ctrl(ch: char) -> KeyEvent {
        press(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    /// Waits for the running action's outcome.
    fn settled(status: &mut Status<Item>) -> Settled {
        let start = Instant::now();
        loop {
            match status.settle() {
                Settled::Unchanged => {
                    assert!(start.elapsed() < Duration::from_secs(5), "never settled");
                    std::thread::sleep(Duration::from_millis(5));
                }
                settled => return settled,
            }
        }
    }

    #[test]
    fn enter_runs_the_action_in_place_and_success_ends_the_picker() {
        let (actions, runs) = acting(Ok(()));
        let mut background = Background::start().unwrap();
        let mut model = model();
        let mut status = Status::default();
        let enter = press(KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            status
                .key(enter, 1, &mut model, Some(&actions), &mut background)
                .is_none(),
            "the picker stays while the action runs"
        );
        assert_eq!(
            status.running.as_ref().map(|r| r.label.as_str()),
            Some("open")
        );
        let typed = press(KeyCode::Char('x'), KeyModifiers::NONE);
        for key in [typed, enter, ctrl('c')] {
            let exit = status.key(key, 1, &mut model, Some(&actions), &mut background);
            assert!(exit.is_none(), "keys wait for the action");
        }
        status.paste("pasted", &mut model);
        assert_eq!(model.query(), "");
        assert_eq!(settled(&mut status), Settled::Done);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(background.shutdown().is_ok());
    }

    #[test]
    fn a_failed_action_keeps_the_query_and_shows_its_message() {
        let (actions, runs) = acting(Err("pr:3 is not a GitHub PR URL".to_owned()));
        let mut background = Background::start().unwrap();
        let mut model = model();
        model.insert_text("some");
        let mut status = Status::default();
        assert!(
            status
                .key(ctrl('r'), 1, &mut model, Some(&actions), &mut background)
                .is_none()
        );
        assert_eq!(
            status.running.as_ref().map(|r| r.label.as_str()),
            Some("review PR")
        );
        assert_eq!(settled(&mut status), Settled::Failed);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(status.running.is_none());
        assert_eq!(status.alert.as_deref(), Some("pr:3 is not a GitHub PR URL"));
        assert_eq!(model.query(), "some");
        assert!(model.selected_entry().is_some(), "the selection stays");

        // Moving keeps the message; editing the query clears it.
        let down = press(KeyCode::Down, KeyModifiers::NONE);
        status.key(down, 1, &mut model, Some(&actions), &mut background);
        assert!(status.alert.is_some());
        let typed = press(KeyCode::Char('x'), KeyModifiers::NONE);
        status.key(typed, 1, &mut model, Some(&actions), &mut background);
        assert_eq!((status.alert.as_deref(), model.query()), (None, "somex"));

        // Esc closes the picker as usual.
        let esc = press(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(
            status.key(esc, 1, &mut model, Some(&actions), &mut background),
            Some(Exit::List(Outcome::Abort))
        ));
        assert!(background.shutdown().is_ok());
    }

    #[test]
    fn another_action_clears_the_last_error() {
        let (actions, _) = acting(Err("first".to_owned()));
        let mut background = Background::start().unwrap();
        let mut model = model();
        model.insert_text("x");
        let mut status = Status::default();
        status.key(ctrl('r'), 1, &mut model, Some(&actions), &mut background);
        assert_eq!(settled(&mut status), Settled::Failed);
        status.key(ctrl('r'), 1, &mut model, Some(&actions), &mut background);
        assert!(status.alert.is_none() && status.running.is_some());
        assert_eq!(settled(&mut status), Settled::Failed);
        assert!(background.shutdown().is_ok());
    }

    #[test]
    fn a_blank_line_asks_for_input_without_running_the_action() {
        let (actions, runs) = acting(Ok(()));
        let mut background = Background::start().unwrap();
        let mut model = model();
        model.insert_text("  ");
        let mut status = Status::default();
        assert!(
            status
                .key(ctrl('r'), 1, &mut model, Some(&actions), &mut background)
                .is_none()
        );
        assert!(status.running.is_none());
        assert_eq!(
            status.alert.as_deref(),
            Some("ctrl+r review PR needs ‹URL›")
        );
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(runs.load(Ordering::SeqCst), 0);
        assert!(background.shutdown().is_ok());
    }

    #[test]
    fn the_hint_follows_its_label_in_the_help() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let shortcuts = [review(), Shortcut::control('o', "open", ())];
        plain_frame(&mut terminal, &mut model(), &shortcuts, &Status::default());
        let shown = rows(&terminal);
        let help = shown
            .iter()
            .position(|row| row.contains("ctrl+r review PR ‹URL›   ctrl+o open "))
            .unwrap_or_else(|| panic!("{shown:#?}"));
        let buffer = terminal.backend().buffer();
        let hint = (0..80)
            .find(|&x| buffer[(x, u16::try_from(help).unwrap())].symbol() == "‹")
            .unwrap();
        let cell = &buffer[(hint, u16::try_from(help).unwrap())];
        assert!(cell.modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn an_action_error_sits_under_the_rule_above_the_help_in_red() {
        let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
        let mut model = model();
        let palette = model.palette();
        let status = Status {
            navigation: None,
            running: None,
            alert: Some(
                "This message is long enough that it has to wrap \
                 across far more than three lines of a narrow picker, and the rest \
                 of it is cut short so the help and list stay where they were"
                    .to_owned(),
            ),
        };
        let stale = Notice::Stale("rex is down".to_owned());
        render(
            &mut terminal,
            &mut model,
            &[review()],
            &status,
            Some(&stale),
            None,
            PreviewLayout::default(),
        );
        let shown = rows(&terminal);
        let prompt = shown.iter().position(|row| row.contains('›')).unwrap();
        let help = shown
            .iter()
            .position(|row| row.contains("ctrl+r review PR"))
            .unwrap();
        assert!(shown[prompt + 1].contains("Scope · all"));
        assert!(
            !shown[prompt + 1].contains("not refreshed"),
            "the error outranks the feed notice"
        );
        assert_eq!(help, prompt + 5, "three error lines between: {shown:#?}");
        assert!(shown[prompt + 2].contains("This message"));
        assert!(shown[prompt + 4].trim_end().ends_with('…'), "{shown:#?}");
        let buffer = terminal.backend().buffer();
        let y = u16::try_from(prompt + 2).unwrap();
        assert_eq!(buffer[(1, y)].fg, palette.red);

        // Once the error is gone, the notice is back and help moves up.
        plain_frame(&mut terminal, &mut model, &[review()], &Status::default());
        let shown = rows(&terminal);
        assert!(shown[prompt + 2].contains("ctrl+r review PR"));
    }

    #[test]
    fn a_running_action_is_named_on_the_rule() {
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
        let (_sender, outcome) = oneshot::channel();
        let status = Status {
            navigation: None,
            running: Some(Running {
                label: "review PR".to_owned(),
                outcome,
            }),
            alert: None,
        };
        let stale = Notice::Stale("rex is down".to_owned());
        render(
            &mut terminal,
            &mut model(),
            &[review()],
            &status,
            Some(&stale),
            None,
            PreviewLayout::default(),
        );
        let shown = rows(&terminal);
        assert!(
            shown
                .iter()
                .any(|row| row.contains("Scope · all · running review PR…")),
            "{shown:#?}"
        );
    }

    #[test]
    fn alerts_wrap_between_words_split_long_ones_and_drop_escapes() {
        assert_eq!(
            alert_lines("paste a GitHub PR URL", 12),
            ["paste a", "GitHub PR", "URL"]
        );
        assert_eq!(
            alert_lines("see https://github.com/o/r", 10),
            ["see", "https://gi", "thub.com/…"]
        );
        assert_eq!(
            alert_lines("\u{1b}[31mred\u{1b}[0m\nsecond line", 40),
            ["red", "second line"]
        );
        assert_eq!(alert_lines("anything", 0), Vec::<String>::new());
    }

    #[test]
    fn preview_environment_tracks_the_visible_pane() {
        let (columns, lines) = preview_size(
            Rect::new(0, 0, 100, 20),
            false,
            None,
            PreviewPosition::Right,
        );
        assert!(columns > 0);
        assert_eq!(lines, 16);
        assert_eq!(
            preview_size(Rect::new(0, 0, 100, 20), true, None, PreviewPosition::Right),
            (columns, 15),
            "a shortcut line takes a body row"
        );
        assert_eq!(
            preview_size(
                Rect::new(0, 0, 100, 20),
                true,
                Some("failed"),
                PreviewPosition::Right
            ),
            (columns, 14),
            "so does an error line"
        );
        assert_eq!(
            preview_size(Rect::new(0, 0, 60, 20), false, None, PreviewPosition::Right),
            (0, 0)
        );
        assert_ne!(
            (columns, lines),
            preview_size(
                Rect::new(0, 0, 120, 25),
                false,
                None,
                PreviewPosition::Right
            )
        );
    }

    #[test]
    fn a_preview_below_gets_the_body_under_a_share_of_list_rows() {
        let body = Rect::new(1, 5, 60, 30);
        let below = split(body, PreviewPosition::Below).unwrap();
        assert_eq!(below.list, Rect::new(1, 5, 60, 12), "40% of the body");
        assert_eq!(below.rule, Rect::new(1, 17, 60, 1));
        assert_eq!(below.pane, Rect::new(1, 18, 60, 17));

        // A short body keeps the list's minimum; shorter still, no preview.
        let short = split(Rect::new(0, 0, 60, 11), PreviewPosition::Below).unwrap();
        assert_eq!((short.list.height, short.pane.height), (5, 5));
        assert_eq!(split(Rect::new(0, 0, 60, 10), PreviewPosition::Below), None);

        // Below works on terminals too narrow for a preview on the right.
        let narrow = Rect::new(0, 0, 40, 24);
        assert_eq!(split(narrow, PreviewPosition::Right), None);
        assert!(split(narrow, PreviewPosition::Below).is_some());
    }

    #[test]
    fn the_preview_size_is_the_pane_below_the_list() {
        let area = Rect::new(0, 0, 50, 40);
        let body = regions(area, true, None).body;
        let pane = split(body, PreviewPosition::Below).unwrap().pane;
        assert_eq!(
            preview_size(area, true, None, PreviewPosition::Below),
            (pane.width, pane.height)
        );
        assert_eq!(pane.width, body.width, "the pane spans the body");
        assert_eq!(pane.bottom(), body.bottom());
        assert_eq!(
            preview_size(Rect::new(0, 0, 50, 12), true, None, PreviewPosition::Below),
            (0, 0),
            "too short for a preview"
        );
    }

    #[test]
    fn a_preview_below_draws_under_a_rule_and_pages_by_the_list() {
        let content = Content::Text(vec![Line::raw("preview text")]);
        let mut terminal = Terminal::new(TestBackend::new(50, 30)).unwrap();
        let none: &[Shortcut<()>] = &[];
        let page = render(
            &mut terminal,
            &mut model(),
            none,
            &Status::default(),
            None,
            Some(&content),
            PreviewLayout {
                position: PreviewPosition::Below,
                ..PreviewLayout::default()
            },
        );
        let shown = rows(&terminal);
        let row = shown
            .iter()
            .position(|line| line.contains("something searchable"))
            .unwrap();
        let preview = shown
            .iter()
            .position(|line| line.contains("preview text"))
            .unwrap_or_else(|| panic!("{shown:#?}"));
        let rule = preview - 1;
        assert!(row < rule, "{shown:#?}");
        assert!(
            shown[rule].trim().chars().all(|c| c == '\u{2500}'),
            "{shown:#?}"
        );
        let body = regions(Rect::new(0, 0, 50, 30), false, None).body;
        let list = split(body, PreviewPosition::Below).unwrap().list;
        assert_eq!(page, usize::from(list.height), "a page is the list's rows");
        assert_eq!(usize::from(list.bottom()), rule);
    }

    #[test]
    fn preview_anchor_selects_the_visible_end_of_the_output() {
        let content = Content::Text((1..=40).map(|n| Line::raw(format!("line {n}"))).collect());
        let status = Status::default();
        for (anchor, visible, hidden) in [
            (PreviewAnchor::End, "line 40", "line 1 "),
            (PreviewAnchor::Start, "line 1 ", "line 40"),
        ] {
            let mut wide = Terminal::new(TestBackend::new(100, 20)).unwrap();
            let none: &[Shortcut<()>] = &[];
            render(
                &mut wide,
                &mut model(),
                none,
                &status,
                None,
                Some(&content),
                PreviewLayout {
                    anchor,
                    ..PreviewLayout::default()
                },
            );
            let shown = screen(&wide);
            assert!(
                shown.contains(visible) && !shown.contains(hidden),
                "{anchor:?}: {shown}"
            );
            assert!(shown.contains("something searchable"), "{shown}");

            let mut narrow = Terminal::new(TestBackend::new(60, 20)).unwrap();
            render(
                &mut narrow,
                &mut model(),
                none,
                &status,
                None,
                Some(&content),
                PreviewLayout {
                    anchor,
                    ..PreviewLayout::default()
                },
            );
            assert!(
                !screen(&narrow).contains(visible),
                "too narrow for a preview"
            );
        }
    }

    #[test]
    fn preview_colors_are_rendered_without_restyling_picker() {
        let content = Content::Text(preview::styled(
            "\u{1b}[38;2;27;124;131mcolored\u{1b}[0m plain",
        ));
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut model = model();
        let palette = model.palette();
        let none: &[Shortcut<()>] = &[];
        render(
            &mut terminal,
            &mut model,
            none,
            &Status::default(),
            None,
            Some(&content),
            PreviewLayout::default(),
        );
        let buffer = terminal.backend().buffer();
        let colored = buffer
            .content()
            .windows(7)
            .find(|cells| {
                cells
                    .iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    == "colored"
            })
            .unwrap();
        assert_eq!(colored.first().unwrap().fg, Color::Rgb(27, 124, 131));
        let plain = buffer
            .content()
            .windows(5)
            .find(|cells| {
                cells
                    .iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    == "plain"
            })
            .unwrap();
        assert_eq!(plain.first().unwrap().fg, palette.strong);
        let row = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "s")
            .unwrap();
        assert_ne!(row.fg, Color::Rgb(27, 124, 131));
    }

    #[test]
    fn prompt_never_exceeds_the_terminal_width() {
        let mut model = model();
        for ch in "a long query".chars() {
            model.apply(crate::Command::Insert(ch));
        }
        for width in 0..20 {
            let (line, input_width) = prompt_line(&model, "> ", model.palette(), width);
            let drawn: usize = line
                .spans
                .iter()
                .map(|span| columns::width(&span.content))
                .sum();
            assert_eq!(drawn, width, "prompt at width {width}");
            assert!(input_width <= width);
        }
    }

    #[test]
    fn chrome_and_cursor_fit_resized_terminals() {
        let failed = Status {
            navigation: None,
            running: None,
            alert: Some("a failure wide enough to wrap 一 👩‍💻".to_owned()),
        };
        for status in [Status::default(), failed] {
            for width in [0, 1, 11, 12, 40, 120] {
                for height in [0, 1, 8, 9, 24] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    let mut model = model();
                    model.insert_text("cafe\u{301} 一 👩‍💻");
                    plain_frame(&mut terminal, &mut model, &[review()], &status);
                    if width > 0 && height > 0 {
                        let (x, y) = terminal.backend_mut().get_cursor_position().unwrap().into();
                        assert!(x < width && y < height);
                    }
                }
            }
        }
    }

    #[test]
    fn shortcut_hint_is_visible_even_when_nothing_matches() {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        let mut model = model();
        model.insert_text("no-matching-destination");
        assert_eq!(model.matched(), 0);
        plain_frame(&mut terminal, &mut model, &[review()], &Status::default());
        assert!(screen(&terminal).contains("ctrl+r review PR"));
    }

    #[test]
    fn shortcuts_get_their_own_help_line_below_the_prompt_rule() {
        let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
        let mut model = model();
        let shortcuts = [
            Shortcut::control('r', "PR", ()),
            Shortcut::control('t', "issue", ()),
            Shortcut::control('s', "new", ()),
        ];
        plain_frame(&mut terminal, &mut model, &shortcuts, &Status::default());
        let rows = rows(&terminal);
        let at = rows
            .iter()
            .position(|row| row.contains("ctrl+r PR"))
            .expect("a help line lists the shortcuts");
        let prompt = rows
            .iter()
            .position(|row| row.contains('›'))
            .expect("the prompt is drawn");
        assert_eq!(at, prompt + 2, "the shortcuts sit below the prompt's rule");
        assert!(rows[prompt + 1].contains('\u{2500}'));
        let line = &rows[at];
        assert!(line.contains("ctrl+t issue") && line.contains("ctrl+s new"));
        assert!(!line.contains("enter open"));
    }

    /// A real job panic that the loop's last check could not have seen.
    fn panicked_after_the_last_check() -> thread::Result<()> {
        let mut background = Background::start().unwrap();
        let (go, unwinding) = crate::background::tests::panics_on(&mut background);
        assert!(!background.failed());
        go.send(()).unwrap();
        unwinding.recv_timeout(Duration::from_secs(5)).unwrap();
        background.shutdown()
    }

    #[test]
    #[should_panic(expected = "job invariant")]
    fn a_job_panic_outranks_a_selection_made_while_it_failed() {
        let _ = conclude(
            Ok(Exit::List(Outcome::Accept)),
            panicked_after_the_last_check(),
        );
    }

    #[test]
    #[should_panic(expected = "job invariant")]
    fn a_job_panic_outranks_an_error_restoring_the_terminal() {
        let restore = Error::Terminal(io::Error::other("leaving the alternate screen"));
        let _ = conclude(Err(restore), panicked_after_the_last_check());
    }

    #[test]
    fn without_a_job_panic_the_session_result_stands() {
        let restore = Error::Terminal(io::Error::other("leaving the alternate screen"));
        assert!(matches!(
            conclude(Err(restore), Ok(())),
            Err(Error::Terminal(_))
        ));
        assert!(matches!(conclude(Ok(Exit::Done), Ok(())), Ok(Exit::Done)));
    }

    #[test]
    fn page_size_is_the_complete_visible_body() {
        let mut terminal = Terminal::new(TestBackend::new(40, 7)).unwrap();
        let none: &[Shortcut<()>] = &[];
        let page = render(
            &mut terminal,
            &mut model(),
            none,
            &Status::default(),
            None,
            None,
            PreviewLayout::default(),
        );
        assert_eq!(page, 6, "one header row leaves six body rows");
    }
}
