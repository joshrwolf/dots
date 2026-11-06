//! `dots attention view`: the user's interactive view of what goals need from
//! them, with a session-oriented inbox and separate goal inspection over the same store
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

use anyhow::{Context as _, Result, ensure};
use dots_pick::Palette;
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
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Widget as _};
use serde::Deserialize;
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
const WIDE: u16 = 72;
const LIST_WIDTH: u16 = 34;
const GOALS_WIDTH: u16 = 34;
const GAP: u16 = 2;
/// Below this the view asks for more room instead of drawing a broken layout.
const MIN_WIDTH: u16 = 24;
const MIN_HEIGHT: u16 = 6;
/// `rex do` for the dismiss action: config actions get 30 seconds.
const SESSION_REFRESH: Duration = Duration::from_secs(5);

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
    let dismissed = if session.navigated {
        Ok(())
    } else {
        dismiss.map_or(Ok(()), |action| run_dismiss(&action))
    };
    drawn?;
    dismissed?;
    Ok(String::new())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Conversation {
    Reported {
        label: String,
        session: String,
        block: String,
    },
    Ambiguous,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Conversations {
    owners: HashMap<String, Conversation>,
}

/// The store and what the view last read from it.
struct Session {
    scope: Scope,
    store: Option<Store>,
    viewer: Viewer,
    provenance: String,
    sessions_at: Option<Instant>,
    navigated: bool,
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
            sessions_at: None,
            navigated: false,
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
        match Palette::load() {
            Ok(palette) => self.viewer.palette = palette,
            Err(error) => {
                self.viewer
                    .refresh(Err(format!("Cannot read the picker theme: {error:#}")));
                return;
            }
        }
        let selected = self
            .viewer
            .current()
            .map(|(_, request)| (request.project.clone(), request.id.clone()));
        let scope = self.scope.clone();
        let list = self
            .store()
            .and_then(|store| Ok(store.attention(&scope, StateFilter::Outstanding)?))
            .map_err(|error| format!("{error:#}"));
        if list.is_err() {
            self.store = None;
        }
        self.viewer.refresh(list);
        if self.viewer.unavailable.is_none()
            && let Some((project, id)) = selected
            && self
                .viewer
                .retired
                .iter()
                .any(|retired| retired.request.id == id && retired.unreadable.is_none())
        {
            let result = self
                .store()
                .and_then(|store| Ok(store.request(&project, &id)?));
            self.viewer.reconciled(&id, result);
        }
        self.load_overview();
        if !self.viewer.requests().is_empty()
            && std::env::var_os("REX_SESSION").is_some()
            && self
                .sessions_at
                .is_none_or(|at| at.elapsed() >= SESSION_REFRESH)
        {
            self.sessions_at = Some(Instant::now());
            match rex_action("attention_sessions", &serde_json::json!({}), 5).and_then(|bytes| {
                serde_json::from_slice::<Conversations>(&bytes)
                    .context("reading Rex conversation identities")
            }) {
                Ok(snapshot) => {
                    self.viewer.conversations = snapshot.owners;
                    self.viewer.session_error = None;
                }
                Err(error) => self.viewer.session_error = Some(format!("{error:#}")),
            }
        }
    }

    fn load_overview(&mut self) {
        let Some((project, goal)) = self.viewer.wanted_overview() else {
            return;
        };
        let overview = self
            .store()
            .and_then(|store| Ok(store.goal_overview(&project, &goal, true)?))
            .map_err(|error| format!("{error:#}"));
        self.viewer.set_overview(goal, overview);
    }

    fn apply(&mut self, effect: Effect) -> bool {
        match effect {
            Effect::None => {}
            Effect::Close => return false,
            Effect::Overview => self.load_overview(),
            Effect::Open {
                owner,
                session,
                block,
            } => {
                let result = rex_action(
                    "attention_open",
                    &serde_json::json!({"owner":owner,"session":session,"block":block}),
                    30,
                )
                .map(|_| ());
                match result {
                    Ok(()) => {
                        self.navigated = true;
                        return false;
                    }
                    Err(error) => {
                        self.viewer.notice = Some(Notice {
                            request: self.viewer.selected.clone(),
                            text: format!("Conversation not opened: {error:#}"),
                            tone: Tone::Error,
                        });
                    }
                }
            }
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
    rex_action(action, &serde_json::json!({}), 30).map(|_| ())
}

fn rex_action(action: &str, args: &serde_json::Value, timeout: u64) -> Result<Vec<u8>> {
    let mut command = Command::new("rex");
    command.args([
        "--timeout",
        &format!("{timeout}s"),
        "do",
        action,
        "--json",
        "--args",
        &serde_json::to_string(args)?,
    ]);
    let output = dots_process::output(command, Duration::from_secs(timeout + 5), 256 * 1024)
        .with_context(|| format!("running rex do {action}"))?;
    ensure!(
        output.status.success(),
        "rex do {action} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
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
    Open {
        owner: String,
        session: String,
        block: String,
    },
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
    Open,
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

struct InboxEntry {
    id: String,
    session: String,
    project: String,
    subject: String,
    glyph: Span<'static>,
}

/// A request the latest snapshot does not list: a closed copy of one that
/// stopped waiting while the user had it selected or drafted, keeping its
/// question and the draft under its own ID, or the last-read copy of one
/// whose goal could not be read.
#[derive(Debug)]
struct Retired {
    goal: GoalAttention,
    request: Request,
    outcome: String,
    /// Why its goal could not be read: the request is shown as last read,
    /// not closed, until a snapshot reads its goal again.
    unreadable: Option<String>,
}

/// The view's whole state: a pure function of inputs and store reads.
#[derive(Debug)]
struct Viewer {
    palette: Palette,
    conversations: HashMap<String, Conversation>,
    session_error: Option<String>,
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
    retired: Vec<Retired>,
    notice: Option<Notice>,
    context_scroll: usize,
    goal: Option<(Project, String)>,
    overview: Option<(String, std::result::Result<GoalOverview, String>)>,
    task_scroll: usize,
    hits: Vec<(Rect, Target)>,
    pressed: Option<(Option<String>, Target)>,
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
            palette: Palette::default(),
            conversations: HashMap::new(),
            session_error: None,
            nothing_selected,
            scope_label,
            list: None,
            unavailable: None,
            tab: Tab::Attention,
            focus: Focus::Requests,
            selected: None,
            items: HashMap::new(),
            drafts: HashMap::new(),
            retired: Vec::new(),
            notice: None,
            context_scroll: 0,
            goal: None,
            overview: None,
            task_scroll: 0,
            hits: Vec::new(),
            pressed: None,
        }
    }

    fn conversation_name(&self, goal: &GoalAttention) -> String {
        if self.session_error.is_some() {
            return "Rex unavailable".into();
        }
        match goal
            .coordinator
            .as_ref()
            .and_then(|owner| self.conversations.get(owner))
        {
            Some(Conversation::Reported { label, .. }) => clean_line(label),
            Some(Conversation::Ambiguous) => "Multiple reporting sessions".into(),
            None if goal.coordinator.is_none() => "No coordinator".into(),
            None => "Session not reported in Rex".into(),
        }
    }

    fn open_conversation(&mut self) -> Effect {
        if self.drafts.values().any(|draft| !draft.text.is_empty()) {
            self.notice = Some(Notice { request: self.selected.clone(), text: "Send or clear your unsent drafts before opening a conversation. Your drafts are kept here.".into(), tone: Tone::Warning });
            return Effect::None;
        }
        let Some((goal, _)) = self.current() else {
            return Effect::None;
        };
        if self.session_error.is_none()
            && let Some(owner) = &goal.coordinator
            && let Some(Conversation::Reported { session, block, .. }) =
                self.conversations.get(owner)
        {
            return Effect::Open {
                owner: owner.clone(),
                session: session.clone(),
                block: block.clone(),
            };
        }
        self.notice = Some(Notice {
            request: self.selected.clone(),
            text: "No exact Rex conversation is available. Nothing was started.".into(),
            tone: Tone::Warning,
        });
        Effect::None
    }

    fn goals(&self) -> &[GoalAttention] {
        self.list.as_ref().map_or(&[], |list| list.goals.as_slice())
    }

    /// Waiting requests first, then answered ones awaiting their coordinator,
    /// then retired ones, each oldest first.
    fn requests(&self) -> Vec<(&GoalAttention, &Request)> {
        let retired = |id: &str| self.retired.iter().any(|retired| retired.request.id == id);
        let mut requests: Vec<_> = self
            .goals()
            .iter()
            .flat_map(|goal| goal.requests.iter().map(move |request| (goal, request)))
            .filter(|(_, request)| request.state != RequestState::Closed && !retired(&request.id))
            .chain(
                self.retired
                    .iter()
                    .map(|retired| (&retired.goal, &retired.request)),
            )
            .collect();
        requests.sort_by_key(|(_, request)| {
            let rank = match request.state {
                RequestState::Open => 0,
                RequestState::Answered => 1,
                RequestState::Closed => 2,
            };
            (rank, request.created_at)
        });
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

    /// Reads a new store snapshot, keeping selection, drafts and the user's
    /// place. The snapshot is authoritative for every goal it could read: a
    /// request it lists is shown as listed. A request missing from a goal or
    /// project the store could not read stays as last read, marked
    /// unreadable. A request missing from a readable scope while selected or
    /// drafted is retired: it stays listed, closed, with its original question
    /// and draft, and nothing moves onto a replacement or selects one.
    fn refresh(&mut self, list: std::result::Result<AttentionList, String>) {
        let list = match list {
            Ok(list) => list,
            Err(error) => {
                self.unavailable = Some(error);
                return;
            }
        };
        self.unavailable = None;
        let before = self
            .current()
            .map(|(_, request)| (request.id.clone(), request.state));
        let watched: Vec<(GoalAttention, Request)> = self
            .requests()
            .into_iter()
            .filter(|(_, request)| request.state != RequestState::Closed)
            .map(|(goal, request)| (heading(goal), request.clone()))
            .collect();
        self.list = Some(list);
        let listed = |goals: &[GoalAttention], id: &str| {
            goals
                .iter()
                .flat_map(|goal| &goal.requests)
                .any(|request| request.id == id)
        };
        let goals = self
            .list
            .as_ref()
            .map_or(&[][..], |list| list.goals.as_slice());
        self.retired
            .retain(|retired| retired.unreadable.is_none() && !listed(goals, &retired.request.id));
        for (goal, request) in watched {
            if listed(self.goals(), &request.id) {
                continue;
            }
            if let Some(error) = self.scope_error(&request) {
                self.retired.push(Retired {
                    goal,
                    request,
                    outcome: String::new(),
                    unreadable: Some(error),
                });
            } else if self.selected.as_deref() == Some(request.id.as_str())
                || self.drafted(&request.id)
            {
                let replacement = self
                    .goals()
                    .iter()
                    .flat_map(|goal| &goal.requests)
                    .find(|now| now.supersedes.as_deref() == Some(request.id.as_str()))
                    .map(|now| now.id.clone());
                self.retire(goal, request, None, replacement);
            }
        }
        let drafts = &self.drafts;
        let selected = self.selected.as_deref();
        self.retired.retain(|retired| {
            retired.unreadable.is_some()
                || selected == Some(retired.request.id.as_str())
                || drafts
                    .get(&retired.request.id)
                    .is_some_and(|draft| !draft.text.is_empty())
        });
        if let Some((id, state)) = before {
            self.follow(&id, state);
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

    fn drafted(&self, id: &str) -> bool {
        self.drafts
            .get(id)
            .is_some_and(|draft| !draft.text.is_empty())
    }

    /// Why the last snapshot could not read the request's goal, if it could not.
    fn scope_error(&self, request: &Request) -> Option<String> {
        self.list
            .as_ref()?
            .errors
            .iter()
            .find(|error| {
                error.project == request.project.as_str()
                    && error.goal.as_ref().is_none_or(|goal| *goal == request.goal)
            })
            .map(|error| error.error.clone())
    }

    /// Keeps a request that no longer waits, closed and unanswerable, so its
    /// question and the user's draft stay readable under its own ID.
    fn retire(
        &mut self,
        goal: GoalAttention,
        mut request: Request,
        closure: Option<ClosureKind>,
        replacement: Option<String>,
    ) {
        let outcome = if replacement.is_some() {
            "Replaced by a newer question, listed separately; nothing was sent. Select the new question to answer it."
        } else if request.state == RequestState::Answered {
            "No longer listed; incorporation has not been confirmed."
        } else {
            match closure {
                Some(ClosureKind::Withdrawn) => "Withdrawn; nothing was sent.",
                Some(ClosureKind::Incorporated) => "Closed by the coordinator.",
                Some(ClosureKind::Superseded) | None => {
                    "No longer waits for you; nothing was sent."
                }
            }
        };
        request.state = RequestState::Closed;
        request.superseded_by = replacement;
        self.retired
            .retain(|retired| retired.request.id != request.id);
        self.retired.push(Retired {
            goal,
            request,
            outcome: outcome.to_owned(),
            unreadable: None,
        });
    }

    /// Canonical closure evidence, not disappearance from an outstanding query.
    fn reconciled(&mut self, id: &str, result: Result<Request>) {
        let Some(retired) = self
            .retired
            .iter_mut()
            .find(|retired| retired.request.id == id)
        else {
            return;
        };
        match result {
            Ok(request) if request.state != RequestState::Open => {
                retired.outcome = match request.closure.as_ref().map(|closure| closure.kind) {
                    Some(ClosureKind::Incorporated) => "✓ Incorporated. A decision is recorded; this does not confirm that work resumed.",
                    Some(ClosureKind::Withdrawn) => "Withdrawn; nothing was sent.",
                    Some(ClosureKind::Superseded) => "Replaced by a newer question, listed separately. Nothing was retargeted.",
                    None => "Reply saved. Incorporation has not been confirmed.",
                }.into();
                retired.request = request;
            }
            Ok(_) => {
                retired.outcome =
                    "The question is open but absent from this scope; refresh before answering."
                        .into();
            }
            Err(error) => {
                retired.outcome = format!(
                    "Cannot confirm the question's outcome: {error:#}. The original question and draft are kept."
                );
            }
        }
    }

    /// Tells the user the selected request changed in the store. Selection
    /// never moves, so no queued key can act on a different question.
    fn follow(&mut self, id: &str, state: RequestState) {
        let Some((_, now)) = self.current() else {
            return;
        };
        if state != RequestState::Open || now.state == RequestState::Open {
            return;
        }
        let what = if now.superseded_by.is_some() {
            "The question changed while you were here. Nothing was sent; the new question is listed separately."
        } else if now.state == RequestState::Answered {
            "This was answered elsewhere while you were here."
        } else {
            "This no longer waits for you; nothing was sent."
        };
        let drafted = self
            .drafts
            .get(id)
            .is_some_and(|draft| !draft.text.is_empty());
        if self.focus == Focus::Text {
            self.focus = Focus::Choices;
        }
        self.notice = Some(Notice {
            request: Some(id.to_owned()),
            text: if drafted {
                format!("{what} Your draft is kept here.")
            } else {
                what.to_owned()
            },
            tone: Tone::Warning,
        });
    }

    /// The outcome of an answer the user sent. Only success removes the draft;
    /// a stale answer is never moved onto a replacement, and the selection
    /// stays on the request the answer was for.
    fn answered(&mut self, id: &str, result: Result<Request>) {
        self.focus = Focus::Requests;
        let error = match result {
            Ok(request) => {
                self.drafts.remove(id);
                self.items.remove(id);
                self.notice = Some(Notice {
                    request: Some(id.to_owned()),
                    text: format!("Saved your answer to “{}”.", clean_line(&request.title)),
                    tone: Tone::Info,
                });
                // The committed request replaces the one the answer was sent
                // to, so the reload that follows reads as no change rather
                // than as an answer from elsewhere.
                let goals = self
                    .list
                    .iter_mut()
                    .flat_map(|list| list.goals.iter_mut())
                    .flat_map(|goal| goal.requests.iter_mut());
                let retired = self.retired.iter_mut().map(|retired| &mut retired.request);
                for slot in goals.chain(retired) {
                    if slot.id == request.id {
                        *slot = request.clone();
                    }
                }
                return;
            }
            Err(error) => error,
        };
        let Some(dots_tasks::Error::RequestNotOpen {
            state,
            closure,
            replacement,
            ..
        }) = error.downcast_ref::<dots_tasks::Error>()
        else {
            self.notice = Some(Notice {
                request: Some(id.to_owned()),
                text: format!("Not saved: {error:#}. Your draft is kept."),
                tone: Tone::Error,
            });
            return;
        };
        let text = if replacement.is_some() {
            "The question changed before your answer was saved. Nothing was sent; the new question is listed separately. Your draft is kept here.".to_owned()
        } else if *state == RequestState::Answered {
            "This was already answered elsewhere; yours was not saved. Your draft is kept here."
                .to_owned()
        } else {
            format!(
                "This request was {} before your answer was saved; nothing was sent.",
                match closure {
                    Some(ClosureKind::Withdrawn) => "withdrawn",
                    Some(ClosureKind::Superseded) => "replaced",
                    Some(ClosureKind::Incorporated) | None => "closed",
                }
            )
        };
        if *state == RequestState::Closed {
            let (closure, replacement) = (*closure, replacement.clone());
            if let Some((goal, request)) = self
                .requests()
                .into_iter()
                .find(|(_, request)| request.id == id)
                .map(|(goal, request)| (heading(goal), request.clone()))
            {
                self.retire(goal, request, closure, replacement);
            }
        }
        self.notice = Some(Notice {
            request: Some(id.to_owned()),
            text,
            tone: Tone::Warning,
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
            KeyCode::Char('g') => {
                return self.switch(if self.tab == Tab::Attention {
                    Tab::Work
                } else {
                    Tab::Attention
                });
            }
            KeyCode::Char('o') if self.tab == Tab::Attention => return self.open_conversation(),
            KeyCode::Char('r') if self.tab == Tab::Attention => {
                if let Some((_, request)) = self.current()
                    && request.state == RequestState::Open
                {
                    let id = request.id.clone();
                    self.focus = Focus::Text;
                    self.drafts.entry(id).or_default();
                }
                return Effect::None;
            }
            KeyCode::Char(c @ '1'..='8') if self.tab == Tab::Attention => {
                let index = usize::try_from(c.to_digit(10).expect("ASCII digit"))
                    .expect("single digit fits")
                    - 1;
                if self
                    .hits
                    .iter()
                    .any(|(_, target)| *target == Target::Item(index))
                    && let Some((_, request)) = self.current()
                    && request.state == RequestState::Open
                    && index < request.choices.len()
                {
                    let id = request.id.clone();
                    self.items.insert(id, index);
                    return self.activate();
                }
                return Effect::None;
            }
            KeyCode::Left if self.tab == Tab::Attention => {
                self.move_request(-1);
                self.focus = Focus::Requests;
                return Effect::None;
            }
            KeyCode::Right if self.tab == Tab::Attention => {
                self.move_request(1);
                self.focus = Focus::Requests;
                return Effect::None;
            }
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
                self.pressed = self
                    .hit(mouse.column, mouse.row)
                    .cloned()
                    .map(|target| (self.selected.clone(), target));
                Effect::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                let pressed = self.pressed.take();
                match self.hit(mouse.column, mouse.row).cloned() {
                    Some(target)
                        if pressed.as_ref() == Some(&(self.selected.clone(), target.clone())) =>
                    {
                        self.click(target)
                    }
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
            Target::Open => return self.open_conversation(),
            Target::Request(id) => {
                self.select_request(id);
                self.focus = Focus::Requests;
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

fn accent(palette: Palette) -> Style {
    Style::new().fg(palette.accent)
}

fn muted(palette: Palette) -> Style {
    Style::new().fg(palette.muted)
}

fn warning(palette: Palette) -> Style {
    Style::new().fg(palette.yellow)
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn tone(palette: Palette, tone: Tone) -> Style {
    match tone {
        Tone::Info => muted(palette),
        Tone::Warning => warning(palette),
        Tone::Error => Style::new().fg(palette.red),
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

    fn section(&mut self, label: &str, palette: Palette) {
        let tail = "─".repeat(self.width().saturating_sub(width(label) + 4));
        self.line(&Line::from(vec![
            Span::styled("── ", muted(palette)),
            Span::styled(label, accent(palette).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {tail}"), muted(palette)),
        ]));
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
fn row_style(palette: Palette, selected: bool, focused: bool) -> (Span<'static>, Style) {
    match (selected, focused) {
        (true, true) => (Span::styled("› ", accent(palette)), bold()),
        (true, false) => (Span::raw("  "), muted(palette)),
        (false, _) => (Span::raw("  "), Style::new()),
    }
}

fn state_glyph(palette: Palette, request: &Request) -> Span<'static> {
    match request.state {
        RequestState::Open => Span::styled("● ", warning(palette)),
        RequestState::Answered => Span::styled("✓ ", Style::new().fg(palette.green)),
        RequestState::Closed => Span::styled("○ ", muted(palette)),
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
fn coordination(palette: Palette, goal: &GoalAttention) -> (String, Style) {
    match &goal.coordinator {
        Some(_) => (
            "Saved. Pickup has not been confirmed; waiting for the coordinator to incorporate it."
                .into(),
            muted(palette),
        ),
        None => (
            "Saved. No coordinator owns this goal; the answer stays available.".into(),
            warning(palette),
        ),
    }
}

/// The rationale shown under a question when there is room for all of it.
fn rationale(palette: Palette, request: &Request, max: usize) -> Vec<(String, Style)> {
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
        block(
            &format!("↳ Recommendation: {recommendation}"),
            accent(palette),
        );
    }
    if !request.tasks.is_empty() {
        let tasks = request
            .tasks
            .iter()
            .map(|task| format!("{} ({})", clean_line(&task.title), task.status.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        block(&format!("Waits on this: {tasks}"), muted(palette));
    }
    lines
}

impl Viewer {
    fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        self.hits.clear();
        let buffer = frame.buffer_mut();
        buffer.set_style(
            area,
            Style::new()
                .bg(self.palette.background)
                .fg(self.palette.strong),
        );
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            let mut rows = Rows::new(buffer, area);
            rows.text("Enlarge this view to see attention.", muted(self.palette));
            rows.text("Esc closes.", muted(self.palette));
            return;
        }
        let title = if self.tab == Tab::Attention {
            format!(" Attention · {} need you ", self.waiting())
        } else {
            " Goals ".into()
        };
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(muted(self.palette))
            .title(Line::from(Span::styled(
                title,
                accent(self.palette).add_modifier(Modifier::BOLD),
            )))
            .render(area, buffer);
        let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
        let footer = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
        let mut body = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
        if self.tab == Tab::Work {
            self.header(buffer, Rect::new(body.x, body.y, body.width, 1));
            body.y += 1;
            body.height -= 1;
        }
        if let Some(status) = self.status_line() {
            let mut rows = Rows::new(buffer, body);
            // Up to two lines: what failed must stay readable, the body must stay visible.
            let max = rows.width();
            for line in wrap(&status, max).iter().take(2) {
                rows.text(line, warning(self.palette));
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

    fn header(&self, buffer: &mut Buffer, area: Rect) {
        let label = fit(
            &format!("Goals · {}", self.scope_label),
            usize::from(area.width),
        );
        buffer.set_string(
            area.x,
            area.y,
            label,
            accent(self.palette).add_modifier(Modifier::BOLD),
        );
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
        let Some(first) = errors.first() else {
            return self
                .session_error
                .as_ref()
                .map(|error| format!("Session navigation unavailable: {}", clean_line(error)));
        };
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
            (Tab::Work, _) => "G inbox · ↑/↓ goal · PgUp/PgDn tasks · Esc back",
            (_, Focus::Text) => "Enter send · Esc back · ←/→ edit answer",
            (_, Focus::Context) => "↑/↓ scroll · Enter/Esc back",
            (_, Focus::Requests) => "↑/↓ inbox · 1–8 reply · R write · O open · Esc close",
            (_, Focus::Choices) => "↑/↓ choice · Enter reply · R write · O open · Esc back",
        }
    }

    fn footer(&mut self, buffer: &mut Buffer, area: Rect) {
        let mut buttons = Vec::new();
        if !(self.tab == Tab::Attention && self.focus == Focus::Requests) {
            buttons.push(("[ Back ]", Target::Back));
        }
        if self.tab == Tab::Attention {
            buttons.push(("[ G Goals ]", Target::Tab(Tab::Work)));
        }
        buttons.push(("[ Close ]", Target::Close));
        let mut x = area.x;
        for (label, target) in buttons {
            let cells = u16::try_from(width(label)).unwrap_or(u16::MAX);
            if x + cells > area.right() {
                return;
            }
            buffer.set_string(x, area.y, label, muted(self.palette));
            self.hits.push((Rect::new(x, area.y, cells, 1), target));
            x += cells + 2;
        }
        let hints = fit(self.hints(), usize::from(area.right().saturating_sub(x)));
        buffer.set_string(x, area.y, hints, muted(self.palette));
    }

    fn attention(&mut self, buffer: &mut Buffer, area: Rect) -> Option<Position> {
        let requests = self.requests();
        if requests.is_empty() {
            let mut rows = Rows::new(buffer, area);
            let message = if self.list.is_none() {
                "Attention could not be read. Nothing was changed."
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
            if !self.goals().is_empty() {
                rows.text("G  Inspect goals and unfinished work", muted(self.palette));
            }
            return None;
        }
        if self.focus == Focus::Context {
            self.context(buffer, area);
            return None;
        }
        let selected = self.selected.clone();
        if area.width >= WIDE {
            let list_width = (area.width / 3).clamp(22, LIST_WIDTH);
            let list = Rect::new(area.x, area.y, list_width, area.height);
            let detail = Rect::new(
                area.x + list_width + GAP,
                area.y,
                area.width - list_width - GAP,
                area.height,
            );
            let entries: Vec<InboxEntry> = requests
                .iter()
                .map(|(goal, request)| InboxEntry {
                    id: request.id.clone(),
                    session: self.conversation_name(goal),
                    project: clean_line(&goal.project_name),
                    subject: format!(
                        "{} · {}",
                        match request.state {
                            RequestState::Open => "Needs you",
                            RequestState::Answered => "Reply saved",
                            RequestState::Closed => "Closed",
                        },
                        clean_line(&request.title)
                    ),
                    glyph: state_glyph(self.palette, request),
                })
                .collect();
            drop(requests);
            self.request_list(buffer, list, &entries, selected.as_deref());
            Block::new()
                .borders(Borders::LEFT)
                .border_style(muted(self.palette))
                .render(Rect::new(detail.x - 1, area.y, 1, area.height), buffer);
            self.detail(buffer, detail)
        } else {
            drop(requests);
            self.detail(buffer, area)
        }
    }

    fn request_list(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        entries: &[InboxEntry],
        selected: Option<&str>,
    ) {
        self.hits.push((area, Target::Region(Region::Requests)));
        let mut rows = Rows::new(buffer, area);
        rows.text("≡ Inbox", accent(self.palette).add_modifier(Modifier::BOLD));
        rows.blank();
        let visible = (rows.left() / 4).max(1);
        let at = entries
            .iter()
            .position(|row| Some(row.id.as_str()) == selected)
            .unwrap_or(0);
        let first = at.saturating_sub(visible - 1);
        for entry in entries.iter().skip(first).take(visible) {
            if rows.left() < 3 {
                break;
            }
            let is_selected = Some(entry.id.as_str()) == selected;
            let row_area = Rect::new(area.x, rows.area.y + rows.next, area.width, 3);
            if is_selected {
                rows.buffer
                    .set_style(row_area, Style::new().bg(self.palette.selection_bg));
            }
            let name = fit(&entry.session, rows.width().saturating_sub(2));
            rows.text(
                &format!("{} {name}", if is_selected { "›" } else { " " }),
                if is_selected {
                    accent(self.palette).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                },
            );
            rows.text(&format!("  {}", entry.project), muted(self.palette));
            let subject = fit(&entry.subject, rows.width().saturating_sub(2));
            rows.line(&Line::from(vec![
                entry.glyph.clone(),
                Span::styled(subject, muted(self.palette)),
            ]));
            self.hits
                .push((row_area, Target::Request(entry.id.clone())));
            rows.blank();
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
        let session_name = self.conversation_name(goal);
        let question = wrap(&request.question, max);
        let notice = self
            .notice
            .as_ref()
            .filter(|notice| {
                notice.request.is_none() || notice.request.as_deref() == Some(&request.id)
            })
            .map(|notice| (wrap(&notice.text, max), tone(self.palette, notice.tone)));
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
                Item::Context => ("Read in full…".to_owned(), None),
            })
            .collect();
        let draft = self.drafts.get(&request.id).cloned().unwrap_or_default();
        let outcome = (!open).then(|| self.outcome(goal, request, &draft, max));
        let rationale = if typing {
            Vec::new()
        } else {
            rationale(self.palette, request, max)
        };

        let action_rows = 1 + match &outcome {
            Some(lines) => lines.len() + 2,
            None if typing => 2,
            None => item_labels.len(),
        };
        let unreadable: Vec<String> = self
            .retired
            .iter()
            .find(|retired| retired.request.id == request.id)
            .and_then(|retired| retired.unreadable.as_deref())
            .map(|error| {
                wrap(
                    &format!(
                        "Cannot read this goal now ({}); shown as last read.",
                        clean_line(error)
                    ),
                    max,
                )
            })
            .unwrap_or_default();
        let notice_rows = notice.as_ref().map_or(0, |(lines, _)| lines.len() + 1);
        let fixed = 4 + unreadable.len() + action_rows + notice_rows;
        let room = usize::from(area.height).saturating_sub(fixed);
        let question_rows = question.len().min(room.max(1));
        let show_rationale = !rationale.is_empty() && room >= question_rows + rationale.len() + 2;

        let mut rows = Rows::new(buffer, area);
        rows.text(
            &session_name,
            accent(self.palette).add_modifier(Modifier::BOLD),
        );
        rows.text(&heading, muted(self.palette));
        rows.section(&format!("? {}", clean_line(&request.title)), self.palette);
        for line in &unreadable {
            rows.text(line, warning(self.palette));
        }
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
        while rows.left() > action_rows + notice_rows + 1 {
            rows.blank();
        }
        rows.section("↳ Reply", self.palette);
        if let Some((lines, style)) = &notice {
            rows.blank();
            for line in lines {
                rows.text(line, *style);
            }
        }
        if let Some(lines) = outcome {
            rows.blank();
            for (line, style) in &lines {
                rows.text(line, *style);
            }
            if let Some(row) = rows.text(
                "› Read in full…",
                if self.focus == Focus::Choices {
                    bold()
                } else {
                    Style::new()
                },
            ) {
                self.hits.push((row, Target::Item(0)));
            }
            if let Some(row) = rows.text("O  Open conversation ↗", accent(self.palette)) {
                self.hits.push((row, Target::Open));
            }
            return None;
        }
        if typing {
            let prompt = "Answer: ";
            let field = max.saturating_sub(width(prompt)).max(1);
            let (shown, caret) = window(&draft, field);
            let text = if draft.text.is_empty() {
                Span::styled("Your answer", muted(self.palette))
            } else {
                Span::raw(shown)
            };
            let row = rows.line(&Line::from(vec![
                Span::styled(prompt, muted(self.palette)),
                text,
            ]));
            let send = rows.text("[ Send answer ]", accent(self.palette));
            if let Some(row) = rows.text("O  Open conversation ↗", accent(self.palette)) {
                self.hits.push((row, Target::Open));
            }
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
        let visible = rows.left().saturating_sub(1);
        let first = item_at.saturating_sub(visible.saturating_sub(1));
        for (index, (label, detail)) in item_labels.iter().enumerate().skip(first).take(visible) {
            let selected = focused && index == item_at;
            let (marker, style) = if selected {
                (Span::styled("› ", accent(self.palette)), bold())
            } else {
                (Span::raw("  "), Style::new())
            };
            let marker = match items[index] {
                Item::Choice(choice) => Span::styled(
                    format!("{} ", choice + 1),
                    accent(self.palette).add_modifier(Modifier::BOLD),
                ),
                Item::Other => Span::styled("R ", accent(self.palette)),
                Item::Context => marker,
            };
            let label = fit(label, max.saturating_sub(2));
            let mut spans = vec![marker, Span::styled(label.clone(), style)];
            if let Some(detail) = detail {
                let left = max.saturating_sub(2 + width(&label) + 3);
                if left > 8 {
                    spans.push(Span::styled(
                        format!(" · {}", fit(detail, left)),
                        muted(self.palette),
                    ));
                }
            }
            if let Some(row) = rows.line(&Line::from(spans)) {
                self.hits.push((row, Target::Item(index)));
            }
        }
        if let Some(row) = rows.text("O  Open conversation ↗", accent(self.palette)) {
            self.hits.push((row, Target::Open));
        }
        None
    }

    /// What became of a request that no longer waits, and any unsent draft.
    fn outcome(
        &self,
        goal: &GoalAttention,
        request: &Request,
        draft: &Draft,
        max: usize,
    ) -> Vec<(String, Style)> {
        let mut lines: Vec<(String, Style)> = Vec::new();
        let mut add = |text: &str, style: Style| {
            lines.extend(wrap(text, max).into_iter().map(|line| (line, style)));
        };
        if let Some(retired) = self
            .retired
            .iter()
            .find(|retired| retired.request.id == request.id && retired.unreadable.is_none())
        {
            add(&retired.outcome, warning(self.palette));
        } else {
            add(
                &format!("Answer saved: {}", answer_text(request)),
                muted(self.palette),
            );
            let (note, style) = coordination(self.palette, goal);
            add(&note, style);
        }
        if !draft.text.is_empty() {
            add(
                &format!("Your unsent draft: {}", clean_line(&draft.text)),
                muted(self.palette),
            );
        }
        lines
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
            muted(self.palette),
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
        // Choice rows clip to one line; here every label and detail is whole.
        if !request.choices.is_empty() {
            lines.push((String::new(), Style::new()));
            lines.push(("Choices".to_owned(), muted(self.palette)));
            let inner = max.saturating_sub(2).max(1);
            for choice in &request.choices {
                for (index, line) in wrap(&choice.label, inner).into_iter().enumerate() {
                    let lead = if index == 0 { "• " } else { "  " };
                    lines.push((format!("{lead}{line}"), bold()));
                }
                for line in choice.detail.iter().flat_map(|detail| wrap(detail, inner)) {
                    lines.push((format!("  {line}"), muted(self.palette)));
                }
            }
        }
        let rationale = rationale(self.palette, request, max);
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
            .map(|goal| (goal_glyph(self.palette, goal), clean_line(&goal.title)))
            .collect();
        let count = entries.len();
        let detail = if area.width >= WIDE {
            let list = Rect::new(area.x, area.y, GOALS_WIDTH, area.height);
            self.hits.push((list, Target::Region(Region::Goals)));
            let mut rows = Rows::new(buffer, list);
            rows.text("Goals", accent(self.palette));
            let visible = rows.left().max(1);
            let first = at.saturating_sub(visible - 1);
            for (index, (glyph, title)) in entries.iter().enumerate().skip(first).take(visible) {
                let (marker, style) = row_style(self.palette, index == at, true);
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
                Span::styled(counter, muted(self.palette)),
                entries[at].0.clone(),
                Span::styled(title, accent(self.palette).add_modifier(Modifier::BOLD)),
            ])) {
                self.hits.push((row, Target::Region(Region::Goals)));
            }
            rows.rest()
        };
        self.goal_detail(buffer, detail, at);
    }

    #[expect(
        clippy::too_many_lines,
        reason = "One row budget keeps the goal summary and task outline visible together."
    )]
    fn goal_detail(&mut self, buffer: &mut Buffer, area: Rect, at: usize) {
        let Some(goal) = self.goals().get(at) else {
            return;
        };
        let palette = self.palette;
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
            muted(palette),
        ));
        let (coordination, style) = match (&goal.coordinator, goal.status) {
            (_, Status::Held) => (
                "Paused: the goal is held until someone resumes it.".to_owned(),
                warning(palette),
            ),
            (_, status) if status.is_closed() => {
                (format!("Goal is {}.", status.as_str()), muted(palette))
            }
            (Some(owner), _) => (
                format!(
                    "Coordinator claim: {} (a claim does not show the session is running)",
                    clean_line(owner)
                ),
                muted(palette),
            ),
            (None, _) if goal.mandate.is_some() => (
                "Stopped: work was agreed but nobody coordinates it; resume the goal in Pi."
                    .to_owned(),
                warning(palette),
            ),
            (None, _) => (
                "Not started: no execution is agreed and nobody coordinates it.".to_owned(),
                muted(palette),
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
            head.push((parts.join(" · "), warning(palette)));
        }
        if let Some(mandate) = &goal.mandate {
            let first = mandate
                .decision
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("");
            head.push((
                fit(&format!("Agreed: {}", clean_line(first)), max),
                muted(palette),
            ));
        }
        let lines: Vec<(String, Style)> = match &self.overview {
            Some((id, Ok(overview))) if *id == goal.goal => {
                task_lines(palette, &goal.goal, overview, max)
            }
            Some((id, Err(error))) if *id == goal.goal => {
                wrap(&format!("Tasks unavailable: {error}"), max)
                    .into_iter()
                    .map(|line| (line, warning(palette)))
                    .collect()
            }
            _ => vec![("Reading tasks…".to_owned(), muted(palette))],
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

/// The goal fields a retired request is shown with; its requests and mandate
/// stay in the live list.
fn heading(goal: &GoalAttention) -> GoalAttention {
    GoalAttention {
        project: goal.project.clone(),
        project_name: goal.project_name.clone(),
        goal: goal.goal.clone(),
        title: goal.title.clone(),
        status: goal.status,
        coordinator: goal.coordinator.clone(),
        mandate: None,
        requests: Vec::new(),
    }
}

fn goal_glyph(palette: Palette, goal: &GoalAttention) -> Span<'static> {
    if goal
        .requests
        .iter()
        .any(|request| request.state == RequestState::Open)
    {
        Span::styled("? ", warning(palette))
    } else if goal.status == Status::Held
        || (goal.coordinator.is_none() && goal.mandate.is_some() && !goal.status.is_closed())
    {
        Span::styled("! ", warning(palette))
    } else if goal.coordinator.is_none() && !goal.status.is_closed() {
        Span::styled("○ ", muted(palette))
    } else {
        Span::styled("● ", accent(palette))
    }
}

/// A task's state as the goal popup in Pi names it, with holds first.
fn task_state(palette: Palette, task: &Task) -> (&'static str, Style) {
    let open = matches!(task.status, Status::Open | Status::Review);
    let request_hold = |wanted: RequestState| {
        task.holds
            .iter()
            .any(|hold| matches!(hold, Hold::Request { state, .. } if *state == wanted))
    };
    if open && request_hold(RequestState::Open) {
        return ("? Needs you", warning(palette));
    }
    // The human already answered; the hold lifts when the coordinator
    // incorporates the answer.
    if open && request_hold(RequestState::Answered) {
        return ("◇ Awaiting coordinator", muted(palette));
    }
    if open
        && task
            .holds
            .iter()
            .any(|hold| matches!(hold, Hold::HeldAncestor { .. }))
    {
        return ("! Paused", warning(palette));
    }
    match task.status {
        Status::Done => ("✓ Done", Style::new().fg(palette.green)),
        Status::Held => ("! Held", warning(palette)),
        Status::Dropped => ("− Dropped", muted(palette)),
        Status::Review => ("◇ Review", accent(palette)),
        Status::Open if task.owner.is_some() => ("● Claimed", accent(palette)),
        Status::Open if !task.blocked_by.is_empty() => ("◌ Waiting", muted(palette)),
        Status::Open => ("○ Open", muted(palette)),
    }
}

/// The goal's tasks as an outline under the root, oldest first.
fn task_lines(
    palette: Palette,
    root: &str,
    overview: &GoalOverview,
    max: usize,
) -> Vec<(String, Style)> {
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
        muted(palette),
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
        let (label, style) = task_state(palette, task);
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
            warning(palette),
        ));
    }
    if overview.hidden_closed > 0 {
        lines.push((
            format!("{} closed tasks hidden", overview.hidden_closed),
            muted(palette),
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
    use dots_tasks::attention::{Answer, Choice, Decision, DecisionKind, RequestTask, ScopeError};
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
        assert!(narrow.contains("Public export names"), "{narrow}");
        assert!(narrow.contains("which way"), "{narrow}");
        assert!(narrow.contains("Update consumers together"), "{narrow}");
        assert!(narrow.contains("Attention · 2 need you"), "{narrow}");
        let wide = screen(&mut view, 120, 40);
        assert!(wide.contains("≡ Inbox"), "{wide}");
        assert!(wide.contains("CLI output format"), "{wide}");
        assert!(
            wide.contains("Recommendation: Update consumers together."),
            "{wide}"
        );
        assert!(
            wide.contains("Waits on this: Adapt the API (open)"),
            "{wide}"
        );
        let (list_column, _) = locate(&wide, "Needs you · CLI output format");
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
    fn goals_opens_with_g_and_escape_closes_without_answering() {
        let mut view = viewer(two());
        assert_eq!(press(&mut view, KeyCode::Char('g')), Effect::Overview);
        assert_eq!(view.tab, Tab::Work);
        assert_eq!(press(&mut view, KeyCode::Char('g')), Effect::None);
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

    /// `two()` with "r-1" replaced by "r-3".
    fn replaced() -> AttentionList {
        let mut list = two();
        let mut replacement = request("r-3", "Public export names", &[("rename", "Rename")]);
        replacement.supersedes = Some("r-1".into());
        replacement.created_at = 3;
        list.goals[0].requests[0] = replacement;
        list
    }

    /// Queued Enters, as a user pressing Return again would send them.
    fn enters(view: &mut Viewer) -> Vec<Effect> {
        (0..6)
            .map(|_| press(view, KeyCode::Enter))
            .filter(|effect| *effect != Effect::None)
            .collect()
    }

    #[test]
    fn a_question_replaced_while_typing_keeps_the_draft_with_the_original() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Enter);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Enter);
        type_text(&mut view, "half typed");
        view.refresh(Ok(replaced()));
        assert_eq!(view.selected.as_deref(), Some("r-1"));
        assert_eq!(view.focus, Focus::Choices);
        assert_eq!(view.drafts["r-1"].text, "half typed");
        assert!(
            !view.drafts.contains_key("r-3"),
            "the replacement starts fresh"
        );
        let shown = screen(&mut view, 120, 30);
        assert!(shown.contains("question changed"), "{shown}");
        assert!(shown.contains("Public export names: which way"), "{shown}");
        assert!(shown.contains("Your unsent draft: half typed"), "{shown}");
        assert!(shown.contains("○ Closed · Public export names"), "{shown}");
        assert!(
            shown.contains("● Needs you · Public export names"),
            "{shown}"
        );
        assert_eq!(enters(&mut view), []);

        // A later refresh keeps the original; the replacement needs a deliberate choice.
        view.refresh(Ok(replaced()));
        assert_eq!(view.selected.as_deref(), Some("r-1"));
        press(&mut view, KeyCode::Esc);
        press(&mut view, KeyCode::Up);
        assert_eq!(view.selected.as_deref(), Some("r-3"));
        press(&mut view, KeyCode::Enter);
        match press(&mut view, KeyCode::Enter) {
            Effect::Answer {
                request, choice, ..
            } => {
                assert_eq!(request, "r-3");
                assert_eq!(choice.as_deref(), Some("rename"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn repeated_return_after_a_stale_answer_never_answers_the_replacement() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Enter);
        let Effect::Answer { request, .. } = press(&mut view, KeyCode::Enter) else {
            panic!("the first choice answers");
        };
        assert_eq!(request, "r-1");
        view.answered(
            "r-1",
            Err(dots_tasks::Error::RequestNotOpen {
                id: "r-1".into(),
                state: RequestState::Closed,
                closure: Some(ClosureKind::Superseded),
                replacement: Some("r-3".into()),
            }
            .into()),
        );
        assert_eq!(enters(&mut view), [], "before the reload");
        view.refresh(Ok(replaced()));
        assert_eq!(view.selected.as_deref(), Some("r-1"));
        assert_eq!(enters(&mut view), [], "after the reload");
        press(&mut view, KeyCode::Esc);
        let notice = view.notice.clone().unwrap();
        assert_eq!(notice.request.as_deref(), Some("r-1"));
        assert!(notice.text.contains("Nothing was sent"), "{}", notice.text);
        assert!(!view.drafts.contains_key("r-3"));
        let shown = screen(&mut view, 120, 30);
        assert!(shown.contains("Replaced by a newer question"), "{shown}");
        assert!(shown.contains("Attention · 2 need you"), "{shown}");
    }

    #[test]
    fn an_answer_saved_elsewhere_keeps_the_draft_on_its_own_request() {
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
                state: RequestState::Answered,
                closure: None,
                replacement: None,
            }
            .into()),
        );
        assert!(
            view.notice
                .clone()
                .unwrap()
                .text
                .contains("already answered")
        );
        let mut answered = two();
        answered.goals[0].requests[0].state = RequestState::Answered;
        view.refresh(Ok(answered));
        assert_eq!(view.selected.as_deref(), Some("r-1"));
        assert_eq!(view.drafts["r-1"].text, "mine");
        assert_eq!(enters(&mut view), []);
        press(&mut view, KeyCode::Esc);
        assert!(screen(&mut view, 120, 30).contains("Your unsent draft: mine"));

        // Once the coordinator takes up the answer the request leaves the
        // list; the draft stays readable with it.
        let mut withdrawn = two();
        withdrawn.goals[0].requests.remove(0);
        view.refresh(Ok(withdrawn));
        assert_eq!(view.selected.as_deref(), Some("r-1"));
        let shown = screen(&mut view, 120, 30);
        assert!(
            shown.contains("incorporation has not been confirmed"),
            "{shown}"
        );
        assert!(shown.contains("Your unsent draft: mine"), "{shown}");
    }

    /// "r-1" of `two()` as the store returns it once answered with its first choice.
    fn answered_first() -> Request {
        let mut request = two().goals.remove(0).requests.remove(0);
        request.state = RequestState::Answered;
        request.answer = Some(Answer {
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
        request
    }

    /// `two()` after "r-1" was answered.
    fn two_answered() -> AttentionList {
        let mut list = two();
        list.goals[0].requests[0] = answered_first();
        list
    }

    fn unreadable(goal: Option<&str>) -> ScopeError {
        ScopeError {
            project: project().as_str().into(),
            goal: goal.map(Into::into),
            error: "database disk image is malformed".into(),
        }
    }

    #[test]
    fn a_local_answer_still_reads_as_saved_after_the_reload() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Enter);
        let Effect::Answer { request, .. } = press(&mut view, KeyCode::Enter) else {
            panic!("the first choice answers");
        };
        // Session::apply: the committed answer, then the reload.
        view.answered(&request, Ok(answered_first()));
        view.refresh(Ok(two_answered()));
        let notice = view.notice.clone().unwrap();
        assert_eq!(notice.text, "Saved your answer to “Public export names”.");
        assert_eq!(notice.tone, Tone::Info);
        let shown = screen(&mut view, 120, 30);
        assert!(!shown.contains("elsewhere"), "{shown}");
        assert!(
            shown.contains("Answer saved: Update consumers together"),
            "{shown}"
        );

        // Positive control: an answer from another session while this one
        // only looked.
        let mut view = viewer(two());
        view.refresh(Ok(two_answered()));
        let notice = view.notice.clone().unwrap();
        assert!(
            notice.text.contains("answered elsewhere"),
            "{}",
            notice.text
        );
        assert_eq!(notice.tone, Tone::Warning);
    }

    #[test]
    fn an_unreadable_goal_keeps_its_requests_as_last_read_until_it_reads_again() {
        let mut view = viewer(two());
        press(&mut view, KeyCode::Enter);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Enter);
        type_text(&mut view, "mine");
        for _ in 0..2 {
            let mut failed = list(Vec::new());
            failed.errors.push(unreadable(None));
            view.refresh(Ok(failed));
            let (_, now) = view.current().unwrap();
            assert_eq!((now.id.as_str(), now.state), ("r-1", RequestState::Open));
            assert_eq!(view.focus, Focus::Text, "typing goes on");
            assert_eq!(view.waiting(), 2);
            assert_eq!(view.drafts["r-1"].text, "mine");
            let shown = screen(&mut view, 120, 30);
            assert!(shown.contains("Cannot read this goal now"), "{shown}");
            assert!(!shown.contains("No longer waits"), "{shown}");
            assert!(!shown.contains("nothing was sent"), "{shown}");
        }
        view.refresh(Ok(two()));
        assert!(view.retired.is_empty());
        let (_, now) = view.current().unwrap();
        assert_eq!((now.id.as_str(), now.state), ("r-1", RequestState::Open));
        assert_eq!(view.drafts["r-1"].text, "mine");
        let shown = screen(&mut view, 120, 30);
        assert!(!shown.contains("Cannot read"), "{shown}");
        assert!(shown.contains("Answer: mine"), "{shown}");
        match press(&mut view, KeyCode::Enter) {
            Effect::Answer { request, text, .. } => {
                assert_eq!(request, "r-1");
                assert_eq!(text.as_deref(), Some("mine"));
            }
            other => panic!("{other:?}"),
        }

        // Healthy data stays authoritative: answered elsewhere during the outage.
        let mut failed = list(Vec::new());
        failed.errors.push(unreadable(Some("t-goal")));
        view.refresh(Ok(failed));
        view.refresh(Ok(two_answered()));
        let (_, now) = view.current().unwrap();
        assert_eq!(now.state, RequestState::Answered);
        assert!(view.retired.is_empty());
    }

    #[test]
    fn a_request_missing_from_a_readable_goal_still_stops_waiting() {
        for error in [
            unreadable(Some("t-other")),
            ScopeError {
                project: "p-00000000000000000000000000000002".into(),
                goal: None,
                error: "unsupported schema".into(),
            },
        ] {
            let mut view = viewer(two());
            let mut partial = two();
            partial.goals[0].requests.remove(0);
            partial.errors.push(error);
            view.refresh(Ok(partial));
            let (_, now) = view.current().unwrap();
            assert_eq!((now.id.as_str(), now.state), ("r-1", RequestState::Closed));
            let shown = screen(&mut view, 120, 30);
            assert!(shown.contains("No longer waits for you"), "{shown}");
            assert!(!shown.contains("Cannot read this goal"), "{shown}");
            assert_eq!(view.waiting(), 1);
        }
    }

    #[test]
    fn narrow_choices_that_share_a_prefix_can_be_read_in_full() {
        let mut deploy = request(
            "r-1",
            "Publish target",
            &[
                (
                    "prod",
                    "Publish the artifact to the designated production account",
                ),
                (
                    "staging",
                    "Publish the artifact to the designated staging account",
                ),
            ],
        );
        deploy.choices[0].detail =
            Some("Uses the release signing key.\nVisible to customers.".into());
        let mut view = viewer(list(vec![goal(vec![deploy], Some("pi-coordinator"))]));
        press(&mut view, KeyCode::Enter);
        let rows = screen(&mut view, 40, 20);
        assert!(!rows.contains("production"), "clipped rows:\n{rows}");
        assert!(!rows.contains("staging"), "clipped rows:\n{rows}");
        press(&mut view, KeyCode::End);
        assert!(screen(&mut view, 40, 20).contains("› Read in full…"));
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.focus, Focus::Context);
        let mut seen = String::new();
        for _ in 0..12 {
            seen.push_str(&screen(&mut view, 40, 20));
            press(&mut view, KeyCode::Down);
        }
        for text in [
            "Publish target: which way should this",
            "• Publish the artifact to the",
            "production account",
            "Uses the release signing key.",
            "Visible to customers.",
            "staging account",
        ] {
            assert!(seen.contains(text), "{text:?} never shown:\n{seen}");
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.focus, Focus::Choices);
    }

    #[test]
    fn only_a_saved_answer_clears_its_draft() {
        let mut view = viewer(two());
        view.drafts.insert("r-1".into(), Draft::default());
        view.answered("r-1", Err(anyhow::anyhow!("disk full")));
        assert!(view.drafts.contains_key("r-1"));
        let notice = view.notice.clone().unwrap();
        assert!(
            notice.text.contains("Not saved: disk full"),
            "{}",
            notice.text
        );
        assert_eq!(notice.tone, Tone::Error);
        let saved = two().goals.remove(0).requests.remove(0);
        view.answered("r-1", Ok(saved));
        assert!(!view.drafts.contains_key("r-1"));
        assert_eq!(
            view.selected.as_deref(),
            Some("r-1"),
            "selection never jumps"
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
        assert!(shown.contains("No coordinator owns this goal"), "{shown}");
        assert!(shown.contains("Attention · 0 need you"), "{shown}");
        let mut view = viewer(list(vec![goal(vec![answered], Some("pi-x"))]));
        let shown = screen(&mut view, 80, 20);
        assert!(shown.contains("Pickup has not been confirmed"), "{shown}");
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
        let (column, row) = locate(&shown, "[ G Goals ]");
        assert_eq!(click(&mut view, column + 1, row), Effect::Overview);
        assert_eq!(view.tab, Tab::Work);
        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "[ Back ]");
        click(&mut view, column, row);
        assert_eq!(view.tab, Tab::Attention);

        let shown = screen(&mut view, 120, 30);
        let (column, row) = locate(&shown, "● Needs you");
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
    fn work_shows_every_task_and_who_each_hold_waits_on() {
        let mut view = viewer(list(vec![goal(Vec::new(), None)]));
        press(&mut view, KeyCode::Char('g'));
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
        let mut answered = task("t-cli", "t-goal", "Adapt the CLI", Status::Open);
        answered.holds.push(Hold::Request {
            request: "r-2".into(),
            task: "t-cli".into(),
            state: RequestState::Answered,
        });
        let mut claimed = task("t-docs", "t-goal", "Write docs", Status::Open);
        claimed.owner = Some("pi-worker".into());
        let done = task("t-spike", "t-goal", "Spike", Status::Done);
        let dropped = task("t-alias", "t-goal", "Aliases", Status::Dropped);
        view.set_overview(
            "t-goal".into(),
            Ok(GoalOverview {
                goal: Goal {
                    root,
                    brief: "briefs/goal.md".into(),
                    progress: GoalProgress::default(),
                    last_activity: 1,
                },
                tasks: vec![group, held, answered, claimed, done, dropped],
                hidden_closed: 0,
            }),
        );
        let shown = screen(&mut view, 120, 30);
        assert!(shown.contains("○ Open      API"), "{shown}");
        assert!(shown.contains("  ? Needs you Adapt the API"), "{shown}");
        assert!(
            shown.contains("◇ Awaiting coordinator Adapt the CLI"),
            "{shown}"
        );
        assert!(
            shown.contains("● Claimed   Write docs · pi-worker"),
            "{shown}"
        );
        assert!(shown.contains("✓ Done      Spike"), "{shown}");
        assert!(shown.contains("− Dropped   Aliases"), "{shown}");
        assert!(!shown.contains("hidden"), "{shown}");
    }

    #[test]
    fn only_agreed_work_without_a_coordinator_reads_as_stopped() {
        let mut draft = goal(Vec::new(), None);
        draft.goal = "t-draft".into();
        draft.title = "Someday".into();
        let mut stopped = goal(Vec::new(), None);
        stopped.mandate = Some(Decision {
            id: "d-1".into(),
            project: project(),
            goal: "t-goal".into(),
            kind: DecisionKind::Mandate,
            decision: "Refactor and update every consumer.".into(),
            human: None,
            request: None,
            at: 1,
            by: Some("pi-coordinator".into()),
        });
        let mut view = viewer(list(vec![draft, stopped]));
        press(&mut view, KeyCode::Char('g'));
        let shown = screen(&mut view, 120, 20);
        assert!(shown.contains("› ○ Someday"), "{shown}");
        assert!(shown.contains("! Refactor exports"), "{shown}");
        assert!(shown.contains("Not started"), "{shown}");
        assert!(!shown.contains("Stopped"), "{shown}");
        press(&mut view, KeyCode::Down);
        let shown = screen(&mut view, 120, 20);
        assert!(shown.contains("Stopped: work was agreed"), "{shown}");
        assert!(
            shown.contains("Agreed: Refactor and update every consumer."),
            "{shown}"
        );
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
    fn conversation_navigation_requires_the_current_exact_runtime() {
        let mut data = two();
        data.goals[0].coordinator = Some("pi-current".into());
        let mut view = viewer(data);
        view.conversations.insert(
            "pi-other".into(),
            Conversation::Reported {
                label: "other-session".into(),
                session: "other".into(),
                block: "other".into(),
            },
        );
        assert_eq!(view.open_conversation(), Effect::None);
        view.conversations.insert(
            "pi-current".into(),
            Conversation::Reported {
                label: "service-cleanup".into(),
                session: "target".into(),
                block: "agent".into(),
            },
        );
        assert!(screen(&mut view, 120, 24).contains("service-cleanup"));
        let Effect::Open {
            owner,
            session,
            block,
            ..
        } = view.open_conversation()
        else {
            panic!("exact conversation not opened");
        };
        assert_eq!(
            (owner.as_str(), session.as_str(), block.as_str()),
            ("pi-current", "target", "agent")
        );
        view.conversations
            .insert("pi-current".into(), Conversation::Ambiguous);
        assert_eq!(view.open_conversation(), Effect::None);
        view.session_error = Some("Rex is unavailable".into());
        assert_eq!(view.open_conversation(), Effect::None);
    }

    #[test]
    fn navigation_cannot_silently_discard_another_questions_draft() {
        let mut view = viewer(two());
        view.drafts.insert(
            "r-2".into(),
            Draft {
                text: "still thinking".into(),
                cursor: 14,
            },
        );
        assert_eq!(view.open_conversation(), Effect::None);
        assert_eq!(view.drafts["r-2"].text, "still thinking");
        assert!(view.notice.as_ref().unwrap().text.contains("unsent drafts"));
    }

    #[test]
    fn number_shortcuts_answer_only_visible_choices() {
        let mut view = viewer(two());
        screen(&mut view, 40, 16);
        let Effect::Answer {
            request, choice, ..
        } = press(&mut view, KeyCode::Char('1'))
        else {
            panic!("visible first choice not answered");
        };
        assert_eq!(request, "r-1");
        assert_eq!(choice.as_deref(), Some("update"));
        screen(&mut view, 24, 6);
        assert_eq!(press(&mut view, KeyCode::Char('1')), Effect::None);
    }

    #[test]
    fn mouse_release_cannot_answer_a_question_other_than_the_pressed_one() {
        let mut view = viewer(two());
        screen(&mut view, 120, 30);
        let rect = view
            .hits
            .iter()
            .find(|(_, target)| *target == Target::Item(0))
            .unwrap()
            .0;
        view.mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        });
        view.select_request("r-2".into());
        screen(&mut view, 120, 30);
        assert_eq!(
            view.mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE
            }),
            Effect::None
        );
    }

    #[test]
    fn incorporation_requires_canonical_closure_not_disappearance() {
        let mut view = viewer(two_answered());
        view.select_request("r-1".into());
        let mut absent = two_answered();
        absent.goals[0].requests.remove(0);
        view.refresh(Ok(absent));
        let shown = screen(&mut view, 120, 30);
        assert!(
            shown.contains("incorporation has not been confirmed"),
            "{shown}"
        );
        let mut closed = answered_first();
        closed.state = RequestState::Closed;
        closed.closure = Some(dots_tasks::attention::Closure {
            kind: ClosureKind::Incorporated,
            at: 3,
            by: Some("coordinator".into()),
            reason: None,
            decision: Some("decision".into()),
        });
        view.reconciled("r-1", Ok(closed));
        let shown = screen(&mut view, 120, 30);
        assert!(shown.contains("✓ Incorporated"), "{shown}");
        assert!(
            shown.contains("does not confirm that work resumed"),
            "{shown}"
        );
        view.reconciled("r-1", Err(anyhow::anyhow!("catalog unavailable")));
        let shown = screen(&mut view, 120, 30);
        assert!(
            shown.contains("Cannot confirm the question's outcome"),
            "{shown}"
        );
        let mut absent = two_answered();
        absent.goals[0].requests.remove(0);
        view.refresh(Ok(absent));
        assert_eq!(
            view.selected.as_deref(),
            Some("r-1"),
            "an outcome read failure cannot lose the original question"
        );
    }

    #[test]
    fn saved_feedback_stays_bound_to_its_question() {
        let mut view = viewer(two());
        view.answered("r-1", Ok(answered_first()));
        assert_eq!(
            view.notice.as_ref().unwrap().request.as_deref(),
            Some("r-1")
        );
        view.select_request("r-2".into());
        assert!(view.notice.is_none());
    }

    #[test]
    fn the_real_view_uses_picker_palette_surfaces_and_selected_rows() {
        use ratatui::style::Color;
        let mut view = viewer(two());
        view.palette.background = Color::Rgb(250, 248, 246);
        view.palette.selection_bg = Color::Rgb(220, 230, 240);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| view.draw(frame)).unwrap();
        let rect = view
            .hits
            .iter()
            .find(|(_, target)| *target == Target::Request("r-1".into()))
            .unwrap()
            .0;
        assert_eq!(
            terminal.backend().buffer()[(0, 0)].bg,
            view.palette.background
        );
        assert_eq!(
            terminal.backend().buffer()[(rect.x, rect.y)].bg,
            view.palette.selection_bg
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
