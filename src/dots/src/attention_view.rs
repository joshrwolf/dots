//! `dots attention view`: the user's interactive view of what goals need from
//! them (Attention) and of the goals' durable work (Work), over the same store
//! every other command reads.
//!
//! Viewing never claims, coordinates, resumes or starts anything. The only
//! write is an explicit answer, bound to the immutable request the user saw
//! and committed before the view stops showing the request as waiting.
//! Closing the view answers nothing. Drafts live only in this process.
use std::collections::HashMap;
use std::io::{self, IsTerminal as _};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail, ensure};
use dots_tasks::attention::{
    AnswerContent, AttentionList, ClosureKind, GoalAttention, Request, RequestKind, Scope, Source,
    StateFilter,
};
use dots_tasks::{Actor, GoalOverview, Hold, Owner, Project, RequestState, Status, Store, Task};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

/// The actor recorded for an answer given in this view. It names a human at a
/// UI, never an agent's coordination owner, and grants no goal authority.
const HUMAN: &str = "human";
/// How often an open view reads the store again.
const REFRESH: Duration = Duration::from_secs(2);
/// The store's bound on answer text.
const ANSWER_LIMIT: usize = 4096;
/// The store's bound on provenance.
const PROVENANCE_LIMIT: usize = 512;
/// Side by side from this width; stacked below it.
const WIDE: u16 = 100;
const LIST_WIDTH: u16 = 30;
const GOALS_WIDTH: u16 = 34;
const GAP: u16 = 3;
/// Below this the view asks for more room instead of drawing a broken layout.
const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;
/// `rex do` for the dismiss action: config actions get 30 seconds.
const DISMISS_TIMEOUT: &str = "30s";
const DISMISS_DEADLINE: Duration = Duration::from_secs(35);

/// Runs the view on this terminal until the user closes it, then runs the
/// `dismiss` Rex action (`views_dismiss`) so focus returns where it was.
pub(crate) fn run(scope: Scope, dismiss: Option<String>, json: bool) -> Result<String> {
    ensure!(
        !json,
        "attention view is interactive; use dots attention list --json for data"
    );
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "attention view needs a terminal; use dots attention list for data"
    );
    let mut session = Session::new(scope);
    session.reload();
    let drawn = terminal(&mut session);
    let dismissed = dismiss.map_or(Ok(()), |action| run_dismiss(&action));
    drawn?;
    dismissed?;
    Ok(String::new())
}

/// The store and what the view last read from it.
struct Session {
    scope: Scope,
    store: Option<Store>,
    viewer: Viewer,
    provenance: String,
}

impl Session {
    fn new(scope: Scope) -> Self {
        let provenance = provenance(
            &scope,
            std::env::var("REX_SESSION").ok().as_deref(),
            std::env::var("REX_BLOCK").ok().as_deref(),
        );
        let viewer = Viewer::new(&scope);
        Self {
            scope,
            store: None,
            viewer,
            provenance,
        }
    }

    /// Opens the catalog if needed, so a missing one stays an honest error
    /// on screen and a later refresh can find it.
    fn store(&mut self) -> Result<&mut Store> {
        if self.store.is_none() {
            self.store = Some(crate::project::open()?);
        }
        self.store.as_mut().context("catalog was just opened")
    }

    fn reload(&mut self) {
        let scope = self.scope.clone();
        let list = self
            .store()
            .and_then(|store| Ok(store.attention(&scope, StateFilter::Outstanding)?))
            .map_err(|error| format!("{error:#}"));
        if list.is_err() {
            self.store = None;
        }
        self.viewer.refresh(list);
        self.load_overview();
    }

    fn load_overview(&mut self) {
        let Some((project, goal)) = self.viewer.wanted_overview() else {
            return;
        };
        let overview = self
            .store()
            .and_then(|store| Ok(store.goal_overview(&project, &goal, false)?))
            .map_err(|error| format!("{error:#}"));
        self.viewer.set_overview(goal, overview);
    }

    fn apply(&mut self, effect: Effect) -> bool {
        match effect {
            Effect::None => {}
            Effect::Close => return false,
            Effect::Overview => self.load_overview(),
            Effect::Answer {
                project,
                request,
                choice,
                text,
            } => {
                let content = AnswerContent {
                    choice,
                    text,
                    source: Source::Ui,
                    provenance: self.provenance.clone(),
                    interpretation: None,
                };
                let now = time::OffsetDateTime::now_utc().unix_timestamp();
                let result = self.store().and_then(|store| {
                    let actor = Actor {
                        name: Some(Owner::try_new(HUMAN)?),
                        now,
                    };
                    Ok(store.answer_request(&project, &request, &content, actor)?)
                });
                self.viewer.answered(&request, result);
                self.reload();
            }
        }
        true
    }
}

/// Where an answer came from: this command and, inside Rex, its terminal.
fn provenance(scope: &Scope, rex_session: Option<&str>, rex_block: Option<&str>) -> String {
    let mut text = match scope {
        Scope::Global => "dots attention view --scope global".to_owned(),
        Scope::Goals(_) => "dots attention view --scope goals".to_owned(),
    };
    let clean = |value: Option<&str>| {
        value
            .map(|value| {
                value
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>()
            })
            .filter(|value| !value.is_empty())
    };
    if let Some(session) = clean(rex_session) {
        text.push_str(" rex:");
        text.push_str(&session);
    }
    if let Some(block) = clean(rex_block) {
        text.push_str(" block:");
        text.push_str(&block);
    }
    while text.len() > PROVENANCE_LIMIT {
        text.pop();
    }
    text
}

fn run_dismiss(action: &str) -> Result<()> {
    let mut command = Command::new("rex");
    command.args([
        "--timeout",
        DISMISS_TIMEOUT,
        "do",
        action,
        "--json",
        "--args",
        "{}",
    ]);
    let output = dots_process::output(command, DISMISS_DEADLINE, 64 * 1024)
        .with_context(|| format!("running rex do {action}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "rex do {action} failed ({}): {}",
            output.status,
            stderr.trim()
        );
    }
    Ok(())
}

/// Takes the terminal, runs the loop, and gives the terminal back whatever the
/// loop returned. Ratatui's hook restores it on a panic.
fn terminal(session: &mut Session) -> Result<()> {
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(error.into());
        }
    };
    let modes = match Modes::enable() {
        Ok(modes) => modes,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(error.into());
        }
    };
    let outcome = drive(&mut terminal, session);
    let modes_restored = modes.disable();
    let restored = ratatui::try_restore();
    outcome?;
    modes_restored?;
    restored?;
    Ok(())
}

fn drive(terminal: &mut ratatui::DefaultTerminal, session: &mut Session) -> Result<()> {
    let mut refreshed = Instant::now();
    let mut dirty = true;
    loop {
        if dirty {
            terminal.draw(|frame| session.viewer.draw(frame))?;
            dirty = false;
        }
        let left = REFRESH.saturating_sub(refreshed.elapsed());
        if !event::poll(left.min(Duration::from_millis(250)))? {
            if refreshed.elapsed() >= REFRESH {
                session.reload();
                refreshed = Instant::now();
                dirty = true;
            }
            continue;
        }
        dirty = true;
        let effect = match event::read()? {
            Event::Key(key) => session.viewer.key(key),
            Event::Paste(text) => session.viewer.paste(&text),
            Event::Mouse(mouse) => session.viewer.mouse(mouse),
            _ => Effect::None,
        };
        if !session.apply(effect) {
            return Ok(());
        }
    }
}

/// Bracketed paste and mouse reporting, which Ratatui does not manage itself.
struct Modes {
    enabled: bool,
}

impl Modes {
    fn enable() -> io::Result<Self> {
        ratatui::crossterm::execute!(io::stdout(), EnableBracketedPaste, EnableMouseCapture)?;
        Ok(Self { enabled: true })
    }

    fn disable(mut self) -> io::Result<()> {
        self.enabled = false;
        ratatui::crossterm::execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste)
    }
}

impl Drop for Modes {
    fn drop(&mut self) {
        if self.enabled {
            let _ = ratatui::crossterm::execute!(
                io::stdout(),
                DisableMouseCapture,
                DisableBracketedPaste
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Attention,
    Work,
}

/// Where keys go within Attention. Work has one list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Requests,
    Choices,
    Text,
    Context,
}

/// What the run loop does after an input.
#[derive(Debug, PartialEq, Eq)]
enum Effect {
    None,
    Close,
    /// The selected Work goal changed.
    Overview,
    Answer {
        project: Project,
        request: String,
        choice: Option<String>,
        text: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Info,
    Warning,
    Error,
}

/// A message about one request, or about the view.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Notice {
    request: Option<String>,
    text: String,
    tone: Tone,
}

/// A typed answer and its caret, as a byte offset on a grapheme boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Draft {
    text: String,
    cursor: usize,
}

impl Draft {
    /// Inserts typed or pasted text as one line, within the store's bound.
    fn insert(&mut self, typed: &str) {
        let mut room = ANSWER_LIMIT.saturating_sub(self.text.len());
        let mut clean = String::new();
        for grapheme in typed.graphemes(true) {
            let grapheme = if grapheme.chars().any(|c| matches!(c, '\n' | '\r' | '\t')) {
                " "
            } else if grapheme.chars().any(char::is_control) {
                continue;
            } else {
                grapheme
            };
            if grapheme.len() > room {
                break;
            }
            room -= grapheme.len();
            clean.push_str(grapheme);
        }
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    fn next(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |grapheme| self.cursor + grapheme.len())
    }

    fn word_start(&self) -> usize {
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end();
        trimmed.rfind(char::is_whitespace).map_or(0, |at| {
            at + trimmed[at..].chars().next().map_or(1, char::len_utf8)
        })
    }

    /// Applies an editing key; false for keys that do not edit.
    fn edit(&mut self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left => self.cursor = self.previous(),
            KeyCode::Right => self.cursor = self.next(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.len(),
            KeyCode::Char('a') if control => self.cursor = 0,
            KeyCode::Char('e') if control => self.cursor = self.text.len(),
            KeyCode::Char('u') if control => {
                self.text.replace_range(..self.cursor, "");
                self.cursor = 0;
            }
            KeyCode::Char('k') if control => self.text.truncate(self.cursor),
            KeyCode::Char('w') if control => {
                let start = self.word_start();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            KeyCode::Backspace => {
                let start = self.previous();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            KeyCode::Delete => {
                let end = self.next();
                self.text.replace_range(self.cursor..end, "");
            }
            KeyCode::Char(c) if !control => self.insert(c.encode_utf8(&mut [0; 4])),
            _ => return false,
        }
        true
    }
}

/// One row under a request's question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Choice(usize),
    Other,
    Context,
}

/// What a click or scroll lands on.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Tab(Tab),
    Request(String),
    Item(usize),
    Input,
    Send,
    Back,
    Close,
    Goal(usize),
    /// A scrollable region; scrolling moves its selection or text.
    Region(Region),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Requests,
    Items,
    Context,
    Goals,
    Tasks,
}

/// The view's whole state: a pure function of inputs and store reads.
#[derive(Debug)]
struct Viewer {
    /// Exact scope with no goals: nothing was asked for, which is not "nothing
    /// needs you".
    nothing_selected: bool,
    scope_label: String,
    list: Option<AttentionList>,
    /// Why the last read failed; the last good rows stay on screen.
    unavailable: Option<String>,
    tab: Tab,
    focus: Focus,
    selected: Option<String>,
    items: HashMap<String, usize>,
    drafts: HashMap<String, Draft>,
    notice: Option<Notice>,
    context_scroll: usize,
    goal: Option<(Project, String)>,
    overview: Option<(String, std::result::Result<GoalOverview, String>)>,
    task_scroll: usize,
    hits: Vec<(Rect, Target)>,
    pressed: Option<Target>,
}

impl Viewer {
    fn new(scope: &Scope) -> Self {
        let (nothing_selected, scope_label) = match scope {
            Scope::Global => (false, "All projects".to_owned()),
            Scope::Goals(goals) => (
                goals.is_empty(),
                match goals.len() {
                    1 => "1 selected goal".to_owned(),
                    count => format!("{count} selected goals"),
                },
            ),
        };
        Self {
            nothing_selected,
            scope_label,
            list: None,
            unavailable: None,
            tab: Tab::Attention,
            focus: Focus::Requests,
            selected: None,
            items: HashMap::new(),
            drafts: HashMap::new(),
            notice: None,
            context_scroll: 0,
            goal: None,
            overview: None,
            task_scroll: 0,
            hits: Vec::new(),
            pressed: None,
        }
    }

    fn goals(&self) -> &[GoalAttention] {
        self.list.as_ref().map_or(&[], |list| list.goals.as_slice())
    }

    /// Waiting requests first, then answered ones awaiting their coordinator,
    /// each oldest first.
    fn requests(&self) -> Vec<(&GoalAttention, &Request)> {
        let mut requests: Vec<_> = self
            .goals()
            .iter()
            .flat_map(|goal| goal.requests.iter().map(move |request| (goal, request)))
            .filter(|(_, request)| request.state != RequestState::Closed)
            .collect();
        requests
            .sort_by_key(|(_, request)| (request.state != RequestState::Open, request.created_at));
        requests
    }

    fn waiting(&self) -> usize {
        self.requests()
            .iter()
            .filter(|(_, request)| request.state == RequestState::Open)
            .count()
    }

    fn current(&self) -> Option<(&GoalAttention, &Request)> {
        let id = self.selected.as_deref()?;
        self.requests()
            .into_iter()
            .find(|(_, request)| request.id == id)
    }

    fn find(&self, id: &str) -> Option<&Request> {
        self.goals()
            .iter()
            .flat_map(|goal| &goal.requests)
            .find(|request| request.id == id)
    }

    /// Reads a new store snapshot, keeping selection, drafts and the user's
    /// place. A request replaced while the user was looking at it hands its
    /// draft to the replacement, which the user must still send.
    fn refresh(&mut self, list: std::result::Result<AttentionList, String>) {
        let list = match list {
            Ok(list) => list,
            Err(error) => {
                self.unavailable = Some(error);
                return;
            }
        };
        self.unavailable = None;
        let before = self.selected.as_deref().and_then(|id| {
            self.find(id)
                .map(|request| (request.id.clone(), request.title.clone(), request.state))
        });
        let index = self.selected.as_deref().and_then(|id| {
            self.requests()
                .iter()
                .position(|(_, request)| request.id == id)
        });
        self.list = Some(list);
        if let Some((id, title, state)) = before {
            self.follow(&id, &title, state, index);
        }
        if self.current().is_none() {
            self.selected = self
                .requests()
                .first()
                .map(|(_, request)| request.id.clone());
            self.focus = Focus::Requests;
        }
        let goals = self.goals();
        let kept = self.goal.as_ref().filter(|(project, goal)| {
            goals
                .iter()
                .any(|known| &known.project == project && &known.goal == goal)
        });
        if kept.is_none() {
            self.goal = goals
                .first()
                .map(|goal| (goal.project.clone(), goal.goal.clone()));
            self.task_scroll = 0;
        }
    }

    /// Keeps the user's place after the selected request changed in the store.
    fn follow(&mut self, id: &str, title: &str, state: RequestState, index: Option<usize>) {
        if let Some((_, now)) = self.current() {
            if state == RequestState::Open
                && now.state != RequestState::Open
                && matches!(self.focus, Focus::Choices | Focus::Text)
            {
                self.focus = Focus::Requests;
                self.notice = Some(Notice {
                    request: Some(id.to_owned()),
                    text: "This was answered elsewhere while you were here; your draft is kept."
                        .into(),
                    tone: Tone::Warning,
                });
            }
            return;
        }
        let replacement = self
            .requests()
            .iter()
            .find(|(_, request)| request.supersedes.as_deref() == Some(id))
            .map(|(_, request)| request.id.clone());
        if let Some(replacement) = replacement {
            self.carry_draft(id, &replacement);
            self.selected = Some(replacement.clone());
            if self.focus == Focus::Text {
                self.focus = Focus::Choices;
            }
            self.notice = Some(Notice {
                request: Some(replacement),
                text: "The question changed while you were reading it. Nothing was sent; review the new question. Your draft is kept.".into(),
                tone: Tone::Warning,
            });
            return;
        }
        let requests = self.requests();
        self.selected = index
            .and_then(|index| requests.get(index.min(requests.len().saturating_sub(1))))
            .map(|(_, request)| request.id.clone());
        self.focus = Focus::Requests;
        self.notice = Some(Notice {
            request: None,
            text: format!("“{}” no longer waits for you.", clean_line(title)),
            tone: Tone::Info,
        });
    }

    fn carry_draft(&mut self, from: &str, to: &str) {
        if let Some(draft) = self.drafts.get(from).cloned()
            && !draft.text.is_empty()
        {
            self.drafts.entry(to.to_owned()).or_insert(draft);
        }
    }

    /// The outcome of an answer the user sent. Only success removes the draft;
    /// a stale answer is never moved onto a replacement.
    fn answered(&mut self, id: &str, result: Result<Request>) {
        self.focus = Focus::Requests;
        let error = match result {
            Ok(request) => {
                self.drafts.remove(id);
                self.items.remove(id);
                // On to the next question still waiting, if any.
                let next = self
                    .requests()
                    .iter()
                    .find(|(_, other)| other.id != id && other.state == RequestState::Open)
                    .map(|(_, other)| other.id.clone());
                if let Some(next) = next {
                    self.select_request(next);
                }
                self.notice = Some(Notice {
                    request: None,
                    text: format!("Saved your answer to “{}”.", clean_line(&request.title)),
                    tone: Tone::Info,
                });
                return;
            }
            Err(error) => error,
        };
        let stale = matches!(
            error.downcast_ref::<dots_tasks::Error>(),
            Some(dots_tasks::Error::RequestNotOpen { .. })
        );
        let (text, replacement) = match error.downcast_ref::<dots_tasks::Error>() {
            Some(dots_tasks::Error::RequestNotOpen {
                replacement: Some(replacement),
                ..
            }) => (
                "The question changed before your answer was saved. Nothing was sent; review the new question. Your draft is kept.".to_owned(),
                Some(replacement.clone()),
            ),
            Some(dots_tasks::Error::RequestNotOpen {
                state: RequestState::Answered,
                ..
            }) => (
                "This was already answered elsewhere; yours was not saved. Your draft is kept."
                    .to_owned(),
                None,
            ),
            Some(dots_tasks::Error::RequestNotOpen { closure, .. }) => (
                format!(
                    "This request was {} before your answer was saved; nothing was sent.",
                    match closure {
                        Some(ClosureKind::Withdrawn) => "withdrawn",
                        Some(ClosureKind::Superseded) => "replaced",
                        Some(ClosureKind::Incorporated) | None => "closed",
                    }
                ),
                None,
            ),
            _ => (
                format!("Not saved: {error:#}. Your draft is kept."),
                None,
            ),
        };
        if let Some(replacement) = &replacement {
            self.carry_draft(id, replacement);
            self.selected = Some(replacement.clone());
            self.focus = Focus::Choices;
        }
        self.notice = Some(Notice {
            request: Some(replacement.unwrap_or_else(|| id.to_owned())),
            text,
            tone: if stale { Tone::Warning } else { Tone::Error },
        });
    }

    fn wanted_overview(&self) -> Option<(Project, String)> {
        (self.tab == Tab::Work).then(|| self.goal.clone())?
    }

    fn set_overview(&mut self, goal: String, overview: std::result::Result<GoalOverview, String>) {
        self.overview = Some((goal, overview));
    }

    fn items_for(request: &Request) -> Vec<Item> {
        let mut items = Vec::new();
        if request.state == RequestState::Open {
            items.extend((0..request.choices.len()).map(Item::Choice));
            items.push(Item::Other);
        }
        items.push(Item::Context);
        items
    }

    fn item_index(&self, id: &str, count: usize) -> usize {
        self.items
            .get(id)
            .copied()
            .unwrap_or(0)
            .min(count.saturating_sub(1))
    }

    fn select_request(&mut self, id: String) {
        if self.selected.as_deref() != Some(&id) {
            self.context_scroll = 0;
            if self
                .notice
                .as_ref()
                .is_some_and(|notice| notice.request.is_some())
            {
                self.notice = None;
            }
        }
        self.selected = Some(id);
    }

    fn move_request(&mut self, delta: isize) {
        let requests = self.requests();
        if requests.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_deref()
            .and_then(|id| requests.iter().position(|(_, request)| request.id == id))
            .unwrap_or(0);
        let next = at.saturating_add_signed(delta).min(requests.len() - 1);
        let id = requests[next].1.id.clone();
        self.select_request(id);
    }

    fn move_item(&mut self, delta: isize) {
        let Some((_, request)) = self.current() else {
            return;
        };
        let count = Self::items_for(request).len();
        let id = request.id.clone();
        let at = self.item_index(&id, count);
        self.items
            .insert(id, at.saturating_add_signed(delta).min(count - 1));
    }

    fn move_goal(&mut self, delta: isize) -> Effect {
        let goals = self.goals();
        if goals.is_empty() {
            return Effect::None;
        }
        let at = self.goal_index().unwrap_or(0);
        let next = at.saturating_add_signed(delta).min(goals.len() - 1);
        self.select_goal(next)
    }

    fn goal_index(&self) -> Option<usize> {
        let (project, goal) = self.goal.as_ref()?;
        self.goals()
            .iter()
            .position(|known| &known.project == project && &known.goal == goal)
    }

    fn select_goal(&mut self, index: usize) -> Effect {
        let Some(goal) = self.goals().get(index) else {
            return Effect::None;
        };
        let wanted = (goal.project.clone(), goal.goal.clone());
        if self.goal.as_ref() == Some(&wanted) {
            return Effect::None;
        }
        self.goal = Some(wanted);
        self.task_scroll = 0;
        Effect::Overview
    }

    fn switch(&mut self, tab: Tab) -> Effect {
        let changed = self.tab != tab;
        self.tab = tab;
        self.focus = Focus::Requests;
        if tab == Tab::Work && changed {
            if let Some((_, goal)) = self.current() {
                let wanted = (goal.project.clone(), goal.goal.clone());
                if self.goal.as_ref() != Some(&wanted) {
                    self.goal = Some(wanted);
                    self.task_scroll = 0;
                }
            }
            return Effect::Overview;
        }
        Effect::None
    }

    /// Esc: one step back, closing from the request list.
    fn back(&mut self) -> Effect {
        match (self.tab, self.focus) {
            (Tab::Work, _) => return self.switch(Tab::Attention),
            (_, Focus::Text | Focus::Context) => self.focus = Focus::Choices,
            (_, Focus::Choices) => self.focus = Focus::Requests,
            (_, Focus::Requests) => return Effect::Close,
        }
        Effect::None
    }

    fn open_choices(&mut self) {
        if self.current().is_some() {
            self.focus = Focus::Choices;
        }
    }

    /// Enter on the selected row under the question.
    fn activate(&mut self) -> Effect {
        let Some((goal, request)) = self.current() else {
            return Effect::None;
        };
        let items = Self::items_for(request);
        let item = items[self.item_index(&request.id, items.len())];
        match item {
            Item::Context => {
                self.focus = Focus::Context;
                self.context_scroll = 0;
                Effect::None
            }
            Item::Other => {
                let id = request.id.clone();
                self.focus = Focus::Text;
                self.drafts.entry(id).or_default();
                Effect::None
            }
            Item::Choice(index) => Effect::Answer {
                project: goal.project.clone(),
                request: request.id.clone(),
                choice: Some(request.choices[index].id.clone()),
                text: None,
            },
        }
    }

    fn send_text(&mut self) -> Effect {
        let Some((goal, request)) = self.current() else {
            return Effect::None;
        };
        if request.state != RequestState::Open {
            return Effect::None;
        }
        let text = self
            .drafts
            .get(&request.id)
            .map(|draft| draft.text.trim().to_owned())
            .unwrap_or_default();
        if text.is_empty() {
            let id = request.id.clone();
            self.notice = Some(Notice {
                request: Some(id),
                text: "Type an answer first; nothing was sent.".into(),
                tone: Tone::Info,
            });
            return Effect::None;
        }
        Effect::Answer {
            project: goal.project.clone(),
            request: request.id.clone(),
            choice: None,
            text: Some(text),
        }
    }

    fn key(&mut self, key: KeyEvent) -> Effect {
        if key.kind == KeyEventKind::Release {
            return Effect::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Effect::Close;
        }
        if self.tab == Tab::Attention && self.focus == Focus::Text {
            match key.code {
                KeyCode::Esc => self.focus = Focus::Choices,
                KeyCode::Enter => return self.send_text(),
                _ => {
                    if let Some(id) = self.selected.clone() {
                        self.drafts.entry(id).or_default().edit(key);
                    }
                }
            }
            return Effect::None;
        }
        match key.code {
            KeyCode::Esc => return self.back(),
            KeyCode::Left => return self.switch(Tab::Attention),
            KeyCode::Right => return self.switch(Tab::Work),
            _ => {}
        }
        let step = match key.code {
            KeyCode::Up | KeyCode::Char('k') => -1,
            KeyCode::Down | KeyCode::Char('j') => 1,
            KeyCode::PageUp => -10,
            KeyCode::PageDown => 10,
            KeyCode::Home => isize::MIN / 2,
            KeyCode::End => isize::MAX / 2,
            _ => 0,
        };
        match (self.tab, self.focus) {
            (Tab::Work, _) => match key.code {
                KeyCode::PageUp => self.task_scroll = self.task_scroll.saturating_sub(10),
                KeyCode::PageDown => self.task_scroll = self.task_scroll.saturating_add(10),
                _ if step != 0 => return self.move_goal(step),
                _ => {}
            },
            (_, Focus::Context) => match key.code {
                KeyCode::Enter => self.focus = Focus::Choices,
                _ => self.context_scroll = self.context_scroll.saturating_add_signed(step),
            },
            (_, Focus::Requests) if key.code == KeyCode::Enter => self.open_choices(),
            (_, Focus::Requests) => self.move_request(step),
            (_, Focus::Choices) if key.code == KeyCode::Enter => return self.activate(),
            (_, Focus::Choices) => self.move_item(step),
            (_, Focus::Text) => {}
        }
        Effect::None
    }

    fn paste(&mut self, text: &str) -> Effect {
        if self.tab == Tab::Attention
            && self.focus == Focus::Text
            && let Some(id) = self.selected.clone()
        {
            self.drafts.entry(id).or_default().insert(text);
        }
        Effect::None
    }

    fn hit(&self, column: u16, row: u16) -> Option<&Target> {
        let at = Position::new(column, row);
        self.hits
            .iter()
            .rev()
            .find(|(area, target)| !matches!(target, Target::Region(_)) && area.contains(at))
            .map(|(_, target)| target)
    }

    fn region(&self, column: u16, row: u16) -> Option<Region> {
        let at = Position::new(column, row);
        self.hits
            .iter()
            .rev()
            .find_map(|(area, target)| match target {
                Target::Region(region) if area.contains(at) => Some(*region),
                _ => None,
            })
    }

    /// A press captures its target; the release over the same target acts.
    fn mouse(&mut self, mouse: MouseEvent) -> Effect {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.pressed = self.hit(mouse.column, mouse.row).cloned();
                Effect::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let pressed = self.pressed.take();
                match self.hit(mouse.column, mouse.row).cloned() {
                    Some(target) if pressed.as_ref() == Some(&target) => self.click(target),
                    _ => Effect::None,
                }
            }
            MouseEventKind::ScrollUp => self.scroll(mouse.column, mouse.row, -1),
            MouseEventKind::ScrollDown => self.scroll(mouse.column, mouse.row, 1),
            _ => Effect::None,
        }
    }

    fn scroll(&mut self, column: u16, row: u16, step: isize) -> Effect {
        match self.region(column, row) {
            Some(Region::Requests) => self.move_request(step),
            Some(Region::Items) => self.move_item(step),
            Some(Region::Context) => {
                self.context_scroll = self.context_scroll.saturating_add_signed(step);
            }
            Some(Region::Goals) => return self.move_goal(step),
            Some(Region::Tasks) => self.task_scroll = self.task_scroll.saturating_add_signed(step),
            None => {}
        }
        Effect::None
    }

    fn click(&mut self, target: Target) -> Effect {
        match target {
            Target::Tab(tab) => return self.switch(tab),
            Target::Request(id) => {
                self.select_request(id);
                self.open_choices();
            }
            Target::Item(index) => {
                if let Some(id) = self.selected.clone() {
                    self.items.insert(id, index);
                    self.focus = Focus::Choices;
                    return self.activate();
                }
            }
            Target::Input => self.focus = Focus::Text,
            Target::Send => return self.send_text(),
            Target::Back => return self.back(),
            Target::Close => return Effect::Close,
            Target::Goal(index) => return self.select_goal(index),
            Target::Region(_) => {}
        }
        Effect::None
    }
}

/// Text from the store, safe to draw: no control characters, tabs as spaces.
fn clean(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            '\n' => Some('\n'),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

fn clean_line(text: &str) -> String {
    clean(text).replace('\n', " ")
}

fn width(text: &str) -> usize {
    text.width()
}

/// At most `max` cells, ending in `…` when cut.
fn fit(text: &str, max: usize) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let cells = grapheme.width();
        if used + cells + 1 > max {
            break;
        }
        out.push_str(grapheme);
        used += cells;
    }
    out.push('…');
    out
}

/// Word-wrapped lines of at most `max` cells; words longer than a line break
/// between graphemes.
fn wrap(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines = Vec::new();
    for paragraph in clean(text).split('\n') {
        let mut line = String::new();
        let mut used = 0;
        for word in paragraph.split_word_bounds() {
            let cells = width(word);
            if used + cells <= max {
                line.push_str(word);
                used += cells;
                continue;
            }
            if !line.is_empty() {
                lines.push(line.trim_end().to_owned());
                line = String::new();
                used = 0;
            }
            if word.trim().is_empty() {
                continue;
            }
            for grapheme in word.graphemes(true) {
                let cells = grapheme.width();
                if used + cells > max {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                line.push_str(grapheme);
                used += cells;
            }
        }
        lines.push(line.trim_end().to_owned());
    }
    lines
}

fn accent() -> Style {
    Style::new().fg(Color::Blue)
}

fn muted() -> Style {
    Style::new().fg(Color::DarkGray)
}

fn warning() -> Style {
    Style::new().fg(Color::Yellow)
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn tone(tone: Tone) -> Style {
    match tone {
        Tone::Info => muted(),
        Tone::Warning => warning(),
        Tone::Error => Style::new().fg(Color::Red),
    }
}

/// Lines drawn top to bottom into one area, clipped at its bottom.
struct Rows<'a> {
    buffer: &'a mut Buffer,
    area: Rect,
    next: u16,
}

impl<'a> Rows<'a> {
    fn new(buffer: &'a mut Buffer, area: Rect) -> Self {
        Self {
            buffer,
            area,
            next: 0,
        }
    }

    fn left(&self) -> usize {
        usize::from(self.area.height.saturating_sub(self.next))
    }

    fn width(&self) -> usize {
        usize::from(self.area.width)
    }

    /// Draws one line, returning its row, or `None` once the area is full.
    fn line(&mut self, line: &Line<'_>) -> Option<Rect> {
        if self.next >= self.area.height {
            return None;
        }
        let row = Rect::new(self.area.x, self.area.y + self.next, self.area.width, 1);
        self.buffer.set_line(row.x, row.y, line, row.width);
        self.next += 1;
        Some(row)
    }

    fn text(&mut self, text: &str, style: Style) -> Option<Rect> {
        let text = fit(text, self.width());
        self.line(&Line::from(Span::styled(text, style)))
    }

    fn blank(&mut self) {
        if self.next < self.area.height {
            self.next += 1;
        }
    }

    fn rest(&self) -> Rect {
        let used = self.next.min(self.area.height);
        Rect::new(
            self.area.x,
            self.area.y + used,
            self.area.width,
            self.area.height - used,
        )
    }
}

/// The selection marker and styles of one list row.
fn row_style(selected: bool, focused: bool) -> (Span<'static>, Style) {
    match (selected, focused) {
        (true, true) => (Span::styled("› ", accent()), bold()),
        (true, false) => (Span::raw("  "), muted()),
        (false, _) => (Span::raw("  "), Style::new()),
    }
}

fn state_glyph(request: &Request) -> Span<'static> {
    if request.state == RequestState::Open {
        Span::styled("? ", warning())
    } else {
        Span::styled("✓ ", Style::new().fg(Color::Green))
    }
}

fn other_label(request: &Request) -> &'static str {
    if !request.choices.is_empty() {
        "Type a different answer…"
    } else if request.kind == RequestKind::Action {
        "Send an update…"
    } else {
        "Type your answer…"
    }
}

fn answer_text(request: &Request) -> String {
    let Some(answer) = &request.answer else {
        return String::new();
    };
    let choice = answer.content.choice.as_deref().map(|id| {
        request
            .choices
            .iter()
            .find(|choice| choice.id == id)
            .map_or_else(|| id.to_owned(), |choice| clean_line(&choice.label))
    });
    match (choice, answer.content.text.as_deref()) {
        (Some(choice), Some(text)) => format!("{choice}: {}", clean_line(text)),
        (Some(choice), None) => choice,
        (None, Some(text)) => clean_line(text),
        (None, None) => String::new(),
    }
}

/// What happens to a saved answer. A claim says who is responsible, not that
/// its session is running.
fn coordination(goal: &GoalAttention) -> (String, Style) {
    match &goal.coordinator {
        Some(owner) => (
            format!(
                "Saved for the goal's coordinator (claimed by {}) to incorporate.",
                clean_line(owner)
            ),
            muted(),
        ),
        None => (
            "Saved. No session coordinates this goal; resume it in Pi to continue.".into(),
            warning(),
        ),
    }
}

/// The rationale shown under a question when there is room for all of it.
fn rationale(request: &Request, max: usize) -> Vec<(String, Style)> {
    let mut lines = Vec::new();
    let mut block = |text: &str, style: Style| {
        if !lines.is_empty() {
            lines.push((String::new(), Style::new()));
        }
        lines.extend(wrap(text, max).into_iter().map(|line| (line, style)));
    };
    if let Some(context) = &request.context {
        block(context, Style::new());
    }
    if let Some(recommendation) = &request.recommendation {
        block(&format!("Recommend: {recommendation}"), Style::new());
    }
    if !request.tasks.is_empty() {
        let tasks = request
            .tasks
            .iter()
            .map(|task| format!("{} ({})", clean_line(&task.title), task.status.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        block(&format!("Waits on this: {tasks}"), muted());
    }
    lines
}

impl Viewer {
    fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        self.hits.clear();
        let buffer = frame.buffer_mut();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            let mut rows = Rows::new(buffer, area);
            rows.text("Enlarge this view to see attention.", muted());
            rows.text("Esc closes.", muted());
            return;
        }
        let inner = Rect::new(area.x + 1, area.y, area.width - 2, area.height);
        let header = Rect::new(inner.x, inner.y, inner.width, 1);
        let footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        let mut body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 2);
        self.header(buffer, header);
        if let Some(status) = self.status_line() {
            let mut rows = Rows::new(buffer, body);
            // Up to two lines: what failed must stay readable, the body must stay visible.
            let max = rows.width();
            for line in wrap(&status, max).iter().take(2) {
                rows.text(line, warning());
            }
            body = rows.rest();
        }
        let cursor = match self.tab {
            Tab::Attention => self.attention(buffer, body),
            Tab::Work => {
                self.work(buffer, body);
                None
            }
        };
        self.footer(buffer, footer);
        if let Some(position) = cursor {
            frame.set_cursor_position(position);
        }
    }

    fn header(&mut self, buffer: &mut Buffer, area: Rect) {
        let tabs = [
            (Tab::Attention, format!("← Attention ({})", self.waiting())),
            (Tab::Work, "Work →".to_owned()),
        ];
        let mut x = area.x;
        for (tab, label) in tabs {
            let active = self.tab == tab;
            let text = format!("{}[ {label} ]", if active { "› " } else { "" });
            let style = if active {
                accent().add_modifier(Modifier::BOLD)
            } else {
                muted()
            };
            let cells = u16::try_from(width(&text)).unwrap_or(u16::MAX);
            if x + cells > area.right() {
                break;
            }
            buffer.set_string(x, area.y, &text, style);
            self.hits
                .push((Rect::new(x, area.y, cells, 1), Target::Tab(tab)));
            x += cells + 2;
        }
        let label = fit(
            &self.scope_label,
            usize::from(area.right().saturating_sub(x)),
        );
        let cells = u16::try_from(width(&label)).unwrap_or(0);
        if cells > 0 {
            buffer.set_string(area.right() - cells, area.y, &label, muted());
        }
    }

    fn status_line(&self) -> Option<String> {
        if let Some(error) = &self.unavailable {
            return Some(if self.list.is_some() {
                format!(
                    "! Refresh failed; showing the last read. {}",
                    clean_line(error)
                )
            } else {
                format!("! Attention is unavailable: {}", clean_line(error))
            });
        }
        let errors = &self.list.as_ref()?.errors;
        let first = errors.first()?;
        let what = first.goal.as_deref().map_or_else(
            || format!("project {}", first.project),
            |goal| format!("{} goal {goal}", first.project),
        );
        Some(format!(
            "! {} unreadable: {what}: {}",
            match errors.len() {
                1 => "1 part of the scope is".to_owned(),
                count => format!("{count} parts of the scope are"),
            },
            clean_line(&first.error)
        ))
    }

    fn hints(&self) -> &'static str {
        match (self.tab, self.focus) {
            (Tab::Work, _) => "← Attention · ↑/↓ goal · PgUp/PgDn tasks · Esc back",
            (_, Focus::Text) => "Enter send · Esc back · ←/→ edit answer",
            (_, Focus::Context) => "↑/↓ scroll · Enter/Esc back",
            (_, Focus::Requests) => "→ Work · ↑/↓ request · Enter answer · Esc close",
            (_, Focus::Choices) => "← Attention · → Work · ↑/↓ choice · Enter select · Esc back",
        }
    }

    fn footer(&mut self, buffer: &mut Buffer, area: Rect) {
        let mut buttons = Vec::new();
        if !(self.tab == Tab::Attention && self.focus == Focus::Requests) {
            buttons.push(("[ Back ]", Target::Back));
        }
        buttons.push(("[ Close ]", Target::Close));
        let mut x = area.x;
        for (label, target) in buttons {
            let cells = u16::try_from(width(label)).unwrap_or(u16::MAX);
            if x + cells > area.right() {
                return;
            }
            buffer.set_string(x, area.y, label, muted());
            self.hits.push((Rect::new(x, area.y, cells, 1), target));
            x += cells + 2;
        }
        let hints = fit(self.hints(), usize::from(area.right().saturating_sub(x)));
        buffer.set_string(x, area.y, hints, muted());
    }

    /// Draws Attention, returning where the caret goes while typing.
    fn attention(&mut self, buffer: &mut Buffer, area: Rect) -> Option<Position> {
        let requests = self.requests();
        if requests.is_empty() {
            let mut rows = Rows::new(buffer, area);
            let message = if self.list.is_none() {
                "Nothing can be shown until the store can be read. Nothing was changed."
            } else if self.nothing_selected {
                "No goals are selected."
            } else if self
                .list
                .as_ref()
                .is_some_and(|list| !list.errors.is_empty())
            {
                "Nothing needs you in the parts of the scope that could be read."
            } else {
                "Nothing needs you."
            };
            rows.text(message, bold());
            let goals = self.goals().len();
            if goals > 0 {
                rows.blank();
                rows.text(
                    &format!(
                        "Work → shows {goals} {}.",
                        if goals == 1 { "goal" } else { "goals" }
                    ),
                    muted(),
                );
            }
            return None;
        }
        if self.focus == Focus::Context {
            self.context(buffer, area);
            return None;
        }
        let selected = self.selected.clone();
        if area.width >= WIDE {
            let list = Rect::new(area.x, area.y, LIST_WIDTH, area.height);
            let detail = Rect::new(
                area.x + LIST_WIDTH + GAP,
                area.y,
                area.width - LIST_WIDTH - GAP,
                area.height,
            );
            let entries: Vec<(String, Span<'static>, String)> = requests
                .iter()
                .map(|(_, request)| {
                    (
                        request.id.clone(),
                        state_glyph(request),
                        clean_line(&request.title),
                    )
                })
                .collect();
            drop(requests);
            self.request_list(buffer, list, &entries, selected.as_deref());
            self.detail(buffer, detail)
        } else {
            let position = selected
                .as_deref()
                .and_then(|id| requests.iter().position(|(_, request)| request.id == id))
                .unwrap_or(0);
            let count = requests.len();
            let title = clean_line(&requests[position].1.title);
            drop(requests);
            let mut rest = area;
            if self.focus == Focus::Requests {
                let mut rows = Rows::new(buffer, area);
                let counter = format!("{}/{count} · ", position + 1);
                let title = fit(&title, rows.width().saturating_sub(width(&counter)));
                if let Some(row) = rows.line(&Line::from(vec![
                    Span::styled(counter, muted()),
                    Span::styled(title, accent().add_modifier(Modifier::BOLD)),
                ])) {
                    self.hits.push((row, Target::Region(Region::Requests)));
                }
                rest = rows.rest();
            }
            self.detail(buffer, rest)
        }
    }

    fn request_list(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        entries: &[(String, Span<'static>, String)],
        selected: Option<&str>,
    ) {
        let focused = self.focus == Focus::Requests;
        self.hits.push((area, Target::Region(Region::Requests)));
        let mut rows = Rows::new(buffer, area);
        rows.text("Requests", if focused { accent() } else { muted() });
        let visible = rows.left().max(1);
        let at = entries
            .iter()
            .position(|(id, _, _)| Some(id.as_str()) == selected)
            .unwrap_or(0);
        let first = at.saturating_sub(visible - 1);
        for (id, glyph, title) in entries.iter().skip(first).take(visible) {
            let is_selected = Some(id.as_str()) == selected;
            let (marker, style) = row_style(is_selected, focused);
            let title = fit(title, rows.width().saturating_sub(4));
            if let Some(row) = rows.line(&Line::from(vec![
                marker,
                glyph.clone(),
                Span::styled(title, style),
            ])) {
                self.hits.push((row, Target::Request(id.clone())));
            }
        }
    }

    /// The selected request: its question first, then what the user can do,
    /// with the rationale only when all of it fits.
    #[expect(
        clippy::too_many_lines,
        reason = "one row budget decides how the question, rationale and actions share the area"
    )]
    fn detail(&mut self, buffer: &mut Buffer, area: Rect) -> Option<Position> {
        let (goal, request) = self.current()?;
        let max = usize::from(area.width);
        let heading = format!(
            "{} · {}",
            clean_line(&goal.project_name),
            clean_line(&goal.title)
        );
        let question = wrap(&request.question, max);
        let notice = self
            .notice
            .as_ref()
            .filter(|notice| {
                notice.request.is_none() || notice.request.as_deref() == Some(&request.id)
            })
            .map(|notice| (wrap(&notice.text, max), tone(notice.tone)));
        let open = request.state == RequestState::Open;
        let typing = self.focus == Focus::Text && open;
        let items = Self::items_for(request);
        let item_at = self.item_index(&request.id, items.len());
        let item_labels: Vec<(String, Option<String>)> = items
            .iter()
            .map(|item| match item {
                Item::Choice(index) => {
                    let choice = &request.choices[*index];
                    (
                        clean_line(&choice.label),
                        choice.detail.as_deref().map(clean_line),
                    )
                }
                Item::Other => (other_label(request).to_owned(), None),
                Item::Context => ("Read context…".to_owned(), None),
            })
            .collect();
        let saved = (!open).then(|| (answer_text(request), coordination(goal)));
        let rationale = if typing {
            Vec::new()
        } else {
            rationale(request, max)
        };
        let draft = self.drafts.get(&request.id).cloned().unwrap_or_default();

        let action_rows = match &saved {
            Some((answer, (note, _))) => {
                wrap(&format!("Answer saved: {answer}"), max).len() + wrap(note, max).len() + 1
            }
            None if typing => 2,
            None => item_labels.len(),
        };
        let notice_rows = notice.as_ref().map_or(0, |(lines, _)| lines.len() + 1);
        let fixed = 1 + action_rows + notice_rows;
        let room = usize::from(area.height).saturating_sub(fixed);
        let question_rows = question.len().min(room.max(1));
        let show_rationale = !rationale.is_empty() && room >= question_rows + rationale.len() + 2;

        let mut rows = Rows::new(buffer, area);
        rows.text(&heading, muted());
        for (index, line) in question.iter().take(question_rows).enumerate() {
            let line = if index + 1 == question_rows && question_rows < question.len() {
                fit(&format!("{line} …"), max)
            } else {
                line.clone()
            };
            rows.text(&line, bold());
        }
        if show_rationale {
            rows.blank();
            for (line, style) in &rationale {
                rows.text(line, *style);
            }
            rows.blank();
        }
        if let Some((lines, style)) = &notice {
            rows.blank();
            for line in lines {
                rows.text(line, *style);
            }
        }
        if let Some((answer, (note, style))) = saved {
            rows.blank();
            for line in wrap(&format!("Answer saved: {answer}"), max) {
                rows.text(&line, muted());
            }
            for line in wrap(&note, max) {
                rows.text(&line, style);
            }
            if let Some(row) = rows.text(
                "› Read context…",
                if self.focus == Focus::Choices {
                    bold()
                } else {
                    Style::new()
                },
            ) {
                self.hits.push((row, Target::Item(0)));
            }
            return None;
        }
        if typing {
            let prompt = "Answer: ";
            let field = max.saturating_sub(width(prompt)).max(1);
            let (shown, caret) = window(&draft, field);
            let text = if draft.text.is_empty() {
                Span::styled("Your answer", muted())
            } else {
                Span::raw(shown)
            };
            let row = rows.line(&Line::from(vec![Span::styled(prompt, muted()), text]));
            let send = rows.text("[ Send answer ]", accent());
            if let Some(row) = row {
                self.hits.push((row, Target::Input));
            }
            if let Some(send) = send {
                let cells = u16::try_from(width("[ Send answer ]")).unwrap_or(0);
                self.hits
                    .push((Rect::new(send.x, send.y, cells, 1), Target::Send));
            }
            return row.map(|row| {
                let x = row.x + u16::try_from(width(prompt) + caret).unwrap_or(0);
                Position::new(x.min(row.right().saturating_sub(1)), row.y)
            });
        }
        let items_area = rows.rest();
        let focused = self.focus == Focus::Choices;
        self.hits.push((items_area, Target::Region(Region::Items)));
        let visible = rows.left().max(1);
        let first = item_at.saturating_sub(visible - 1);
        for (index, (label, detail)) in item_labels.iter().enumerate().skip(first).take(visible) {
            let selected = focused && index == item_at;
            let (marker, style) = if selected {
                (Span::styled("› ", accent()), bold())
            } else {
                (Span::raw("  "), Style::new())
            };
            let label = fit(label, max.saturating_sub(2));
            let mut spans = vec![marker, Span::styled(label.clone(), style)];
            if let Some(detail) = detail {
                let left = max.saturating_sub(2 + width(&label) + 3);
                if left > 8 {
                    spans.push(Span::styled(format!(" · {}", fit(detail, left)), muted()));
                }
            }
            if let Some(row) = rows.line(&Line::from(spans)) {
                self.hits.push((row, Target::Item(index)));
            }
        }
        None
    }

    fn context(&mut self, buffer: &mut Buffer, area: Rect) {
        let Some((goal, request)) = self.current() else {
            return;
        };
        let max = usize::from(area.width);
        let mut lines: Vec<(String, Style)> = vec![(
            format!(
                "{} · {}",
                clean_line(&goal.project_name),
                clean_line(&goal.title)
            ),
            muted(),
        )];
        lines.extend(
            wrap(&request.title, max)
                .into_iter()
                .map(|line| (line, bold())),
        );
        lines.push((String::new(), Style::new()));
        lines.extend(
            wrap(&request.question, max)
                .into_iter()
                .map(|line| (line, Style::new())),
        );
        let rationale = rationale(request, max);
        if !rationale.is_empty() {
            lines.push((String::new(), Style::new()));
            lines.extend(rationale);
        }
        let scroll = self
            .context_scroll
            .min(lines.len().saturating_sub(usize::from(area.height)));
        self.context_scroll = scroll;
        self.hits.push((area, Target::Region(Region::Context)));
        let mut rows = Rows::new(buffer, area);
        for (line, style) in lines.iter().skip(scroll) {
            rows.text(line, *style);
        }
    }

    fn work(&mut self, buffer: &mut Buffer, area: Rect) {
        let goals = self.goals();
        if goals.is_empty() {
            let mut rows = Rows::new(buffer, area);
            let message = if self.nothing_selected {
                "No goals are selected."
            } else if self.list.is_none() {
                "Nothing can be shown until the store can be read."
            } else {
                "No goal has open requests or unfinished agreed work."
            };
            rows.text(message, bold());
            return;
        }
        let at = self.goal_index().unwrap_or(0);
        let entries: Vec<(Span<'static>, String)> = goals
            .iter()
            .map(|goal| (goal_glyph(goal), clean_line(&goal.title)))
            .collect();
        let count = entries.len();
        let detail = if area.width >= WIDE {
            let list = Rect::new(area.x, area.y, GOALS_WIDTH, area.height);
            self.hits.push((list, Target::Region(Region::Goals)));
            let mut rows = Rows::new(buffer, list);
            rows.text("Goals", accent());
            let visible = rows.left().max(1);
            let first = at.saturating_sub(visible - 1);
            for (index, (glyph, title)) in entries.iter().enumerate().skip(first).take(visible) {
                let (marker, style) = row_style(index == at, true);
                let title = fit(title, rows.width().saturating_sub(4));
                if let Some(row) = rows.line(&Line::from(vec![
                    marker,
                    glyph.clone(),
                    Span::styled(title, style),
                ])) {
                    self.hits.push((row, Target::Goal(index)));
                }
            }
            Rect::new(
                area.x + GOALS_WIDTH + GAP,
                area.y,
                area.width - GOALS_WIDTH - GAP,
                area.height,
            )
        } else {
            let mut rows = Rows::new(buffer, area);
            let counter = format!("{}/{count} · ", at + 1);
            let title = fit(
                &entries[at].1,
                rows.width().saturating_sub(width(&counter) + 2),
            );
            if let Some(row) = rows.line(&Line::from(vec![
                Span::styled(counter, muted()),
                entries[at].0.clone(),
                Span::styled(title, accent().add_modifier(Modifier::BOLD)),
            ])) {
                self.hits.push((row, Target::Region(Region::Goals)));
            }
            rows.rest()
        };
        self.goal_detail(buffer, detail, at);
    }

    fn goal_detail(&mut self, buffer: &mut Buffer, area: Rect, at: usize) {
        let Some(goal) = self.goals().get(at) else {
            return;
        };
        let max = usize::from(area.width);
        let mut head: Vec<(String, Style)> = Vec::new();
        head.extend(
            wrap(&goal.title, max)
                .into_iter()
                .map(|line| (line, bold())),
        );
        head.push((
            format!(
                "{} · {}",
                clean_line(&goal.project_name),
                goal.status.as_str()
            ),
            muted(),
        ));
        let (coordination, style) = match (&goal.coordinator, goal.status) {
            (_, Status::Held) => (
                "Paused: the goal is held until someone resumes it.".to_owned(),
                warning(),
            ),
            (_, status) if status.is_closed() => (format!("Goal is {}.", status.as_str()), muted()),
            (Some(owner), _) => (
                format!(
                    "Coordinator claim: {} (a claim does not show the session is running)",
                    clean_line(owner)
                ),
                muted(),
            ),
            (None, _) => (
                "No coordinator: nothing advances until someone resumes the goal in Pi.".to_owned(),
                warning(),
            ),
        };
        head.extend(
            wrap(&coordination, max)
                .into_iter()
                .map(|line| (line, style)),
        );
        let waiting = goal
            .requests
            .iter()
            .filter(|request| request.state == RequestState::Open)
            .count();
        let answered = goal
            .requests
            .iter()
            .filter(|request| request.state == RequestState::Answered)
            .count();
        if waiting + answered > 0 {
            let mut parts = Vec::new();
            if waiting > 0 {
                parts.push(format!("{waiting} waiting for you"));
            }
            if answered > 0 {
                parts.push(format!("{answered} answered, not yet incorporated"));
            }
            head.push((parts.join(" · "), warning()));
        }
        if let Some(mandate) = &goal.mandate {
            let first = mandate
                .decision
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("");
            head.push((fit(&format!("Agreed: {}", clean_line(first)), max), muted()));
        }
        let lines: Vec<(String, Style)> = match &self.overview {
            Some((id, Ok(overview))) if *id == goal.goal => task_lines(&goal.goal, overview, max),
            Some((id, Err(error))) if *id == goal.goal => {
                wrap(&format!("Tasks unavailable: {error}"), max)
                    .into_iter()
                    .map(|line| (line, warning()))
                    .collect()
            }
            _ => vec![("Reading tasks…".to_owned(), muted())],
        };
        let mut rows = Rows::new(buffer, area);
        for (line, style) in &head {
            rows.text(line, *style);
        }
        rows.blank();
        let tasks_area = rows.rest();
        self.hits.push((tasks_area, Target::Region(Region::Tasks)));
        let scroll = self
            .task_scroll
            .min(lines.len().saturating_sub(usize::from(tasks_area.height)));
        self.task_scroll = scroll;
        let mut rows = Rows::new(buffer, tasks_area);
        for (line, style) in lines.iter().skip(scroll) {
            rows.text(line, *style);
        }
    }
}

fn goal_glyph(goal: &GoalAttention) -> Span<'static> {
    if goal
        .requests
        .iter()
        .any(|request| request.state == RequestState::Open)
    {
        Span::styled("? ", warning())
    } else if goal.status == Status::Held {
        Span::styled("! ", warning())
    } else if goal.coordinator.is_none() && !goal.status.is_closed() {
        Span::styled("○ ", muted())
    } else {
        Span::styled("● ", accent())
    }
}

/// A task's state as the goal popup in Pi names it, with holds first.
fn task_state(task: &Task) -> (&'static str, Style) {
    let open = matches!(task.status, Status::Open | Status::Review);
    if open
        && task
            .holds
            .iter()
            .any(|hold| matches!(hold, Hold::Request { .. }))
    {
        return ("? Needs you", warning());
    }
    if open
        && task
            .holds
            .iter()
            .any(|hold| matches!(hold, Hold::HeldAncestor { .. }))
    {
        return ("! Paused", warning());
    }
    match task.status {
        Status::Done => ("✓ Done", Style::new().fg(Color::Green)),
        Status::Held => ("! Held", warning()),
        Status::Dropped => ("− Dropped", muted()),
        Status::Review => ("◇ Review", accent()),
        Status::Open if task.owner.is_some() => ("● Claimed", accent()),
        Status::Open if !task.blocked_by.is_empty() => ("◌ Waiting", muted()),
        Status::Open => ("○ Open", muted()),
    }
}

/// The goal's tasks as an outline under the root, oldest first.
fn task_lines(root: &str, overview: &GoalOverview, max: usize) -> Vec<(String, Style)> {
    let p = &overview.goal.progress;
    let mut lines = vec![(
        if p.total == 0 {
            "No tasks yet.".to_owned()
        } else {
            format!(
                "{}/{} done · {} open · {} claimed",
                p.done, p.total, p.open, p.claimed
            )
        },
        muted(),
    )];
    let mut children: HashMap<&str, Vec<&Task>> = HashMap::new();
    for task in &overview.tasks {
        children
            .entry(task.parent.as_deref().unwrap_or(root))
            .or_default()
            .push(task);
    }
    let mut stack: Vec<(&Task, usize)> = children
        .get(root)
        .map(|tasks| tasks.iter().rev().map(|task| (*task, 0)).collect())
        .unwrap_or_default();
    let mut seen = 0;
    while let Some((task, depth)) = stack.pop() {
        seen += 1;
        let (label, style) = task_state(task);
        let indent = "  ".repeat(depth.min(6));
        let owner = task
            .owner
            .as_deref()
            .map(|owner| format!(" · {}", clean_line(owner)))
            .unwrap_or_default();
        let label = format!("{indent}{label:<11} ");
        let title = fit(
            &format!("{}{owner}", clean_line(&task.title)),
            max.saturating_sub(width(&label)),
        );
        lines.push((format!("{label}{title}"), style));
        if let Some(below) = children.get(task.id.as_str()) {
            stack.extend(below.iter().rev().map(|task| (*task, depth + 1)));
        }
    }
    if seen < overview.tasks.len() {
        lines.push((
            format!("{} tasks outside the outline", overview.tasks.len() - seen),
            warning(),
        ));
    }
    if overview.hidden_closed > 0 {
        lines.push((
            format!("{} closed tasks hidden", overview.hidden_closed),
            muted(),
        ));
    }
    lines
}

/// The part of a draft that fits `max` cells with its caret visible, and the
/// caret's cell offset within it.
fn window(draft: &Draft, max: usize) -> (String, usize) {
    let before: Vec<&str> = draft.text[..draft.cursor].graphemes(true).collect();
    let mut start = before.len();
    let mut used = 0;
    while start > 0 && used + before[start - 1].width() < max {
        start -= 1;
        used += before[start].width();
    }
    let shown_before: String = before[start..].concat();
    let caret = width(&shown_before);
    let mut shown = shown_before;
    let mut cells = caret;
    for grapheme in draft.text[draft.cursor..].graphemes(true) {
        if cells + grapheme.width() > max {
            break;
        }
        shown.push_str(grapheme);
        cells += grapheme.width();
    }
    (shown, caret)
}

#[cfg(test)]
mod tests {
    use dots_tasks::attention::{Answer, Choice, RequestTask, ScopeError};
    use dots_tasks::{Goal, GoalProgress};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn project() -> Project {
        Project::try_new("p-00000000000000000000000000000001").unwrap()
    }

    fn request(id: &str, title: &str, choices: &[(&str, &str)]) -> Request {
        Request {
            id: id.into(),
            project: project(),
            goal: "t-goal".into(),
            key: id.into(),
            kind: RequestKind::Decision,
            title: title.into(),
            question: format!("{title}: which way should this go?"),
            context: Some("This decides the public API after the refactor.".into()),
            recommendation: Some("Update consumers together.".into()),
            choices: choices
                .iter()
                .map(|(id, label)| Choice {
                    id: (*id).into(),
                    label: (*label).into(),
                    detail: None,
                })
                .collect(),
            tasks: vec![RequestTask {
                id: "t-api".into(),
                title: "Adapt the API".into(),
                status: Status::Open,
            }],
            state: RequestState::Open,
            created_at: 1,
            created_by: Some("pi-coordinator".into()),
            supersedes: None,
            superseded_by: None,
            answer: None,
            closure: None,
        }
    }

    fn goal(requests: Vec<Request>, coordinator: Option<&str>) -> GoalAttention {
        GoalAttention {
            project: project(),
            project_name: "fixture".into(),
            goal: "t-goal".into(),
            title: "Refactor exports".into(),
            status: Status::Open,
            coordinator: coordinator.map(Into::into),
            mandate: None,
            requests,
        }
    }

    fn list(goals: Vec<GoalAttention>) -> AttentionList {
        AttentionList {
            goals,
            errors: Vec::new(),
        }
    }

    fn two() -> AttentionList {
        let mut second = request("r-2", "CLI output format", &[("text", "Readable text")]);
        second.created_at = 2;
        list(vec![goal(
            vec![
                request(
                    "r-1",
                    "Public export names",
                    &[
                        ("update", "Update consumers together"),
                        ("keep", "Keep aliases"),
                    ],
                ),
                second,
            ],
            Some("pi-coordinator"),
        )])
    }

    fn viewer(loaded: AttentionList) -> Viewer {
        let mut viewer = Viewer::new(&Scope::Global);
        viewer.refresh(Ok(loaded));
        viewer
    }

    fn screen(viewer: &mut Viewer, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| viewer.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(viewer: &mut Viewer, code: KeyCode) -> Effect {
        viewer.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(viewer: &mut Viewer, text: &str) {
        for c in text.chars() {
            press(viewer, KeyCode::Char(c));
        }
    }

    fn click(viewer: &mut Viewer, column: u16, row: u16) -> Effect {
        let at = |kind| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        viewer.mouse(at(MouseEventKind::Down(MouseButton::Left)));
        viewer.mouse(at(MouseEventKind::Up(MouseButton::Left)))
    }

    /// Where `text` starts on the screen.
    fn locate(screen: &str, text: &str) -> (u16, u16) {
        for (row, line) in screen.lines().enumerate() {
            if let Some(at) = line.find(text) {
                let column = line[..at].width();
                return (u16::try_from(column).unwrap(), u16::try_from(row).unwrap());
            }
        }
        panic!("{text:?} is not on screen:\n{screen}");
    }

    #[test]
    fn the_selected_question_and_its_choices_show_at_once_narrow_and_wide() {
        let mut view = viewer(two());
        let narrow = screen(&mut view, 40, 16);
        assert!(narrow.contains("1/2 · Public export names"), "{narrow}");
        assert!(narrow.contains("which way"), "{narrow}");
        assert!(narrow.contains("Update consumers together"), "{narrow}");
        assert!(narrow.contains("Attention (2)"), "{narrow}");
        let wide = screen(&mut view, 120, 40);
        assert!(wide.contains("Requests"), "{wide}");
        assert!(wide.contains("CLI output format"), "{wide}");
        assert!(
            wide.contains("Recommend: Update consumers together."),
            "{wide}"
        );
        assert!(
            wide.contains("Waits on this: Adapt the API (open)"),
            "{wide}"
        );
        let (list_column, _) = locate(&wide, "? CLI output format");
        let (detail_column, _) = locate(&wide, "Keep aliases");
        assert!(detail_column > list_column + 20, "side by side:\n{wide}");
    }

    #[test]
    fn typing_keeps_the_question_and_arrows_edit_instead_of_switching_views() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Enter);
        for _ in 0..2 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.focus, Focus::Text);
        type_text(&mut view, "use both");
        press(&mut view, KeyCode::Left);
        press(&mut view, KeyCode::Left);
        type_text(&mut view, "X");
        assert_eq!(view.tab, Tab::Attention);
        assert_eq!(view.drafts["r-1"].text, "use boXth");
        let typing = screen(&mut view, 40, 16);
        assert!(typing.contains("which way"), "{typing}");
        assert!(typing.contains("Answer: use boXth"), "{typing}");
        assert!(!typing.contains("Recommend"), "{typing}");
        press(&mut view, KeyCode::End);
        match press(&mut view, KeyCode::Enter) {
            Effect::Answer {
                request,
                choice,
                text,
                ..
            } => {
                assert_eq!(request, "r-1");
                assert_eq!(choice, None);
                assert_eq!(text.as_deref(), Some("use boXth"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_choice_answers_the_request_it_belongs_to() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Enter);
        match press(&mut view, KeyCode::Enter) {
            Effect::Answer {
                request, choice, ..
            } => {
                assert_eq!(request, "r-2");
                assert_eq!(choice.as_deref(), Some("text"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn views_switch_with_left_and_right_and_escape_closes_without_answering() {
        let mut view = viewer(two());
        assert_eq!(press(&mut view, KeyCode::Right), Effect::Overview);
        assert_eq!(view.tab, Tab::Work);
        assert_eq!(press(&mut view, KeyCode::Left), Effect::None);
        assert_eq!(view.tab, Tab::Attention);
        press(&mut view, KeyCode::Enter);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Enter);
        type_text(&mut view, "draft");
        let steps: Vec<Effect> = (0..3).map(|_| press(&mut view, KeyCode::Esc)).collect();
        assert_eq!(steps, [Effect::None, Effect::None, Effect::Close]);
        assert_eq!(view.drafts["r-1"].text, "draft");
    }

    #[test]
    fn a_question_replaced_during_a_refresh_hands_over_the_draft_but_sends_nothing() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Enter);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Enter);
        type_text(&mut view, "half typed");
        let mut replaced = two();
        let mut replacement = request("r-3", "Public export names", &[("update", "Update")]);
        replacement.supersedes = Some("r-1".into());
        replaced.goals[0].requests[0] = replacement;
        view.refresh(Ok(replaced));
        assert_eq!(view.selected.as_deref(), Some("r-3"));
        assert_eq!(view.focus, Focus::Choices);
        assert_eq!(view.drafts["r-3"].text, "half typed");
        assert_eq!(view.drafts["r-1"].text, "half typed");
        let shown = screen(&mut view, 60, 20);
        assert!(shown.contains("question changed"), "{shown}");
    }

    #[test]
    fn a_stale_answer_shows_the_replacement_and_keeps_the_draft() {
        let mut view = viewer(two());
        view.drafts.insert(
            "r-1".into(),
            Draft {
                text: "mine".into(),
                cursor: 4,
            },
        );
        view.answered(
            "r-1",
            Err(dots_tasks::Error::RequestNotOpen {
                id: "r-1".into(),
                state: RequestState::Closed,
                closure: Some(ClosureKind::Superseded),
                replacement: Some("r-2".into()),
            }
            .into()),
        );
        assert_eq!(view.selected.as_deref(), Some("r-2"));
        assert_eq!(view.drafts["r-1"].text, "mine");
        assert_eq!(view.drafts["r-2"].text, "mine");
        let notice = view.notice.clone().unwrap();
        assert_eq!(notice.request.as_deref(), Some("r-2"));
        assert!(notice.text.contains("Nothing was sent"), "{}", notice.text);

        view.answered(
            "r-1",
            Err(dots_tasks::Error::RequestNotOpen {
                id: "r-1".into(),
                state: RequestState::Answered,
                closure: None,
                replacement: None,
            }
            .into()),
        );
        assert!(view.notice.unwrap().text.contains("already answered"));
        assert_eq!(view.drafts["r-1"].text, "mine");
    }

    #[test]
    fn only_a_saved_answer_clears_its_draft_and_moves_on() {
        let mut view = viewer(two());
        view.drafts.insert("r-1".into(), Draft::default());
        view.answered("r-1", Err(anyhow::anyhow!("disk full")));
        assert!(view.drafts.contains_key("r-1"));
        assert!(
            view.notice
                .clone()
                .unwrap()
                .text
                .contains("Not saved: disk full")
        );
        let saved = two().goals.remove(0).requests.remove(0);
        view.answered("r-1", Ok(saved));
        assert!(!view.drafts.contains_key("r-1"));
        assert_eq!(
            view.selected.as_deref(),
            Some("r-2"),
            "on to the next question"
        );
    }

    #[test]
    fn an_answered_request_says_who_incorporates_it_and_whether_anyone_does() {
        let mut answered = request("r-1", "Public export names", &[("update", "Update")]);
        answered.state = RequestState::Answered;
        answered.answer = Some(Answer {
            content: AnswerContent {
                choice: Some("update".into()),
                text: None,
                source: Source::Ui,
                provenance: "dots attention view --scope global".into(),
                interpretation: None,
            },
            at: 2,
            by: Some(HUMAN.into()),
        });
        let mut view = viewer(list(vec![goal(vec![answered.clone()], None)]));
        let shown = screen(&mut view, 80, 20);
        assert!(shown.contains("Answer saved: Update"), "{shown}");
        assert!(
            shown.contains("No session coordinates this goal"),
            "{shown}"
        );
        assert!(shown.contains("Attention (0)"), "{shown}");
        let mut view = viewer(list(vec![goal(vec![answered], Some("pi-x"))]));
        let shown = screen(&mut view, 80, 20);
        assert!(shown.contains("claimed by pi-x"), "{shown}");
    }

    #[test]
    fn unavailable_and_empty_states_never_read_as_nothing_needing_you() {
        let mut view = Viewer::new(&Scope::Global);
        view.refresh(Err("workspace absent".into()));
        let shown = screen(&mut view, 60, 12);
        assert!(
            shown.contains("Attention is unavailable: workspace absent"),
            "{shown}"
        );
        assert!(!shown.contains("Nothing needs you"), "{shown}");

        let mut view = viewer(two());
        view.refresh(Err("database is locked".into()));
        let shown = screen(&mut view, 60, 16);
        assert!(shown.contains("Refresh failed"), "{shown}");
        assert!(shown.contains("Public export names"), "{shown}");

        let mut view = Viewer::new(&Scope::Goals(Vec::new()));
        view.refresh(Ok(list(Vec::new())));
        assert!(screen(&mut view, 60, 12).contains("No goals are selected."));

        let mut partial = list(Vec::new());
        partial.errors.push(ScopeError {
            project: "p-00000000000000000000000000000002".into(),
            goal: None,
            error: "unsupported schema".into(),
        });
        let mut view = viewer(partial);
        let shown = screen(&mut view, 100, 12);
        assert!(shown.contains("unsupported"), "{shown}");
        assert!(
            shown.contains("parts of the scope that could be read"),
            "{shown}"
        );
    }

    #[test]
    fn mouse_clicks_select_views_and_answer_while_the_wheel_moves_lists() {
        let mut view = viewer(two());
        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "Work →");
        assert_eq!(click(&mut view, column + 1, row), Effect::Overview);
        assert_eq!(view.tab, Tab::Work);
        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "Attention (2)");
        click(&mut view, column, row);
        assert_eq!(view.tab, Tab::Attention);

        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "? Public export names");
        view.mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(view.selected.as_deref(), Some("r-2"));
        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "Readable text");
        match click(&mut view, column, row) {
            Effect::Answer {
                request, choice, ..
            } => {
                assert_eq!(request, "r-2");
                assert_eq!(choice.as_deref(), Some("text"));
            }
            other => panic!("{other:?}"),
        }
        // A press that ends elsewhere does nothing.
        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "[ Close ]");
        view.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
        let released = view.mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(released, Effect::None);
    }

    #[test]
    fn every_size_draws_within_bounds_and_tiny_ones_ask_for_room() {
        let mut view = viewer(two());
        for tab in [Tab::Attention, Tab::Work] {
            view.tab = tab;
            for width in 1..=130 {
                for height in [1, 3, 5, 6, 8, 12, 16, 24, 45] {
                    screen(&mut view, width, height);
                }
            }
        }
        view.tab = Tab::Attention;
        assert!(screen(&mut view, 20, 5).contains("Enlarge"));
    }

    fn task(id: &str, parent: &str, title: &str, status: Status) -> Task {
        Task {
            id: id.into(),
            parent: Some(parent.into()),
            title: title.into(),
            body: None,
            result: None,
            result_event: None,
            evidence: Vec::new(),
            purged_evidence: Vec::new(),
            status,
            references: Vec::new(),
            branch: None,
            owner: None,
            created_at: 1,
            updated_at: 1,
            closed_at: None,
            blocked_by: Vec::new(),
            prerequisites: Vec::new(),
            children: 0,
            unclosed_children: 0,
            last_event: None,
            goal: Some("t-goal".into()),
            holds: Vec::new(),
        }
    }

    #[test]
    fn work_shows_stopped_goals_and_tasks_waiting_on_the_user() {
        let mut view = viewer(list(vec![goal(Vec::new(), None)]));
        press(&mut view, KeyCode::Right);
        let mut root = task("t-goal", "t-goal", "Refactor exports", Status::Open);
        root.parent = None;
        let mut group = task("t-group", "t-goal", "API", Status::Open);
        group.children = 1;
        let mut held = task("t-api", "t-group", "Adapt the API", Status::Open);
        held.holds.push(Hold::Request {
            request: "r-1".into(),
            task: "t-api".into(),
            state: RequestState::Open,
        });
        let mut claimed = task("t-docs", "t-goal", "Write docs", Status::Open);
        claimed.owner = Some("pi-worker".into());
        view.set_overview(
            "t-goal".into(),
            Ok(GoalOverview {
                goal: Goal {
                    root,
                    brief: "briefs/goal.md".into(),
                    progress: GoalProgress::default(),
                    last_activity: 1,
                },
                tasks: vec![group, held, claimed],
                hidden_closed: 2,
            }),
        );
        let shown = screen(&mut view, 120, 30);
        assert!(shown.contains("No coordinator"), "{shown}");
        assert!(shown.contains("○ Open      API"), "{shown}");
        assert!(shown.contains("  ? Needs you Adapt the API"), "{shown}");
        assert!(
            shown.contains("● Claimed   Write docs · pi-worker"),
            "{shown}"
        );
        assert!(shown.contains("2 closed tasks hidden"), "{shown}");
        assert!(!shown.contains("Nothing needs you"), "{shown}");
    }

    #[test]
    fn drafts_edit_by_grapheme_within_the_store_bound() {
        let mut draft = Draft::default();
        draft.insert("e\u{301}👍🏽 x\ty\nz\u{7}");
        assert_eq!(draft.text, "e\u{301}👍🏽 x y z");
        draft.cursor = "e\u{301}👍🏽".len();
        draft.edit(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(draft.text, "e\u{301} x y z");
        draft.edit(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(draft.cursor, 0);
        draft.edit(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(draft.text, "e\u{301} x y z");
        draft.edit(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        draft.edit(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(draft.text, "e\u{301} x y ");
        draft.insert(&"a".repeat(ANSWER_LIMIT));
        assert_eq!(draft.text.len(), ANSWER_LIMIT);
        let (shown, caret) = window(&draft, 10);
        assert!(shown.width() <= 10);
        assert_eq!(caret, shown.width());
    }

    #[test]
    fn provenance_names_the_view_and_its_rex_terminal_within_bounds() {
        assert_eq!(
            provenance(&Scope::Global, Some("session:1"), Some("block:2\u{7}")),
            "dots attention view --scope global rex:session:1 block:block:2"
        );
        assert_eq!(
            provenance(&Scope::Goals(Vec::new()), None, Some(" ")),
            "dots attention view --scope goals"
        );
        let long = "x".repeat(1000);
        assert_eq!(
            provenance(&Scope::Global, Some(&long), None).len(),
            PROVENANCE_LIMIT
        );
    }

    #[test]
    fn wrapping_and_fitting_measure_cells() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("界界界界", 5), ["界界", "界界"]);
        assert_eq!(wrap("a\n\nb", 10), ["a", "", "b"]);
        assert_eq!(fit("界界界", 5), "界界…");
        assert_eq!(clean_line("a\u{1b}[31mb\nc"), "a[31mb c");
    }
}
