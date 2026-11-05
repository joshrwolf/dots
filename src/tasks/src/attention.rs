//! Attention requests and decisions: durable human questions and the shared
//! record of what was decided, owned by a goal rather than a session.
//!
//! A request's content never changes; changing the question supersedes it
//! with a new identity. States move `open -> answered -> closed` by
//! compare-and-set inside one immediate transaction, so a competing answer,
//! withdrawal or supersession loses cleanly. An open or answered request
//! holds the tasks it names and their descendants until the coordinator
//! incorporates the answer, which records a decision and closes the request
//! together. Answers can come from any session; every other change belongs
//! to the goal's coordinator. Provenance is cooperative bookkeeping that
//! keeps the human's own words apart from model interpretation; it is not a
//! security boundary against an agent with shell access to the catalog.
use rusqlite::{OptionalExtension as _, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::store::{self, Actor, Error, EventKind, Owner, Project, RequestState, Status, Store};

#[cfg(test)]
mod tests;

const MAX_CHOICES: usize = 8;
const MAX_TASKS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    /// A product or scope choice only the user can make.
    Decision,
    /// Permission for an action outside the recorded mandate.
    Authorization,
    /// Something the user must do, such as authenticate; never multiple choice.
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An explicit human selection or entry in a user interface.
    Ui,
    /// A real user chat message, mapped to this record by an agent.
    Chat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    /// The accepted execution agreement; the newest one is current.
    Mandate,
    Decision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClosureKind {
    Incorporated,
    Withdrawn,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
}

/// The immutable content of a request, as supplied by its coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestContent {
    pub kind: RequestKind,
    pub title: String,
    pub question: String,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub recommendation: Option<String>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// The goal root or its descendants that wait for this request.
    #[serde(default)]
    pub tasks: Vec<String>,
}

/// A human response to an open request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerContent {
    #[serde(default)]
    pub choice: Option<String>,
    /// The human's own words; for chat, the smallest relevant instruction.
    #[serde(default)]
    pub text: Option<String>,
    pub source: Source,
    /// Where the input came from, such as a session and entry.
    #[serde(default)]
    pub provenance: Option<String>,
    /// An agent's reading of chat input; absent for explicit UI answers.
    #[serde(default)]
    pub interpretation: Option<String>,
}

/// What the coordinator decided on incorporating an answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incorporation {
    pub kind: DecisionKind,
    pub decision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanInput {
    pub text: String,
    pub source: Source,
    #[serde(default)]
    pub provenance: Option<String>,
}

/// A decision established outside any request, such as an agreement in chat.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionContent {
    pub kind: DecisionKind,
    /// The coordinator's account of what was decided.
    pub decision: String,
    /// The human input the decision rests on; required, never model-authored.
    pub human: HumanInput,
}

#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    #[serde(flatten)]
    pub content: AnswerContent,
    pub at: i64,
    pub by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Closure {
    pub kind: ClosureKind,
    pub at: i64,
    pub by: Option<String>,
    /// Why a request was withdrawn.
    pub reason: Option<String>,
    /// The decision an incorporated request produced.
    pub decision: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestTask {
    pub id: String,
    pub title: String,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub id: String,
    pub project: Project,
    pub goal: String,
    pub key: String,
    pub kind: RequestKind,
    pub title: String,
    pub question: String,
    pub context: Option<String>,
    pub recommendation: Option<String>,
    pub choices: Vec<Choice>,
    pub tasks: Vec<RequestTask>,
    pub state: RequestState,
    pub created_at: i64,
    pub created_by: Option<String>,
    pub supersedes: Option<String>,
    pub superseded_by: Option<String>,
    pub answer: Option<Answer>,
    pub closure: Option<Closure>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Decision {
    pub id: String,
    pub project: Project,
    pub goal: String,
    pub kind: DecisionKind,
    pub decision: String,
    /// Human input recorded with a standalone decision; an incorporated
    /// decision's human input is its request's answer.
    pub human: Option<HumanInput>,
    pub request: Option<String>,
    pub at: i64,
    pub by: Option<String>,
}

/// A claim on work a new request now holds; its holder should be steered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeldClaim {
    pub task: String,
    pub owner: String,
}

#[derive(Debug, Serialize)]
pub struct Raised {
    pub request: Request,
    /// False when an identical request with this key already existed.
    pub created: bool,
    pub held_claims: Vec<HeldClaim>,
}

#[derive(Debug, Serialize)]
pub struct Incorporated {
    pub request: Request,
    pub decision: Decision,
}

/// One goal, exactly identified across projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GoalRef {
    pub project: Project,
    pub goal: String,
}

impl GoalRef {
    /// Parse `PROJECT:GOAL`.
    pub fn parse(value: &str) -> Result<Self, Error> {
        let (project, goal) = value
            .split_once(':')
            .ok_or(Error::Invalid("goal reference: expected PROJECT:GOAL"))?;
        if goal.is_empty() || goal.len() > 64 || goal.chars().any(char::is_control) {
            return Err(Error::Invalid("goal reference: expected PROJECT:GOAL"));
        }
        Ok(Self {
            project: Project::try_new(project)?,
            goal: goal.to_owned(),
        })
    }
}

/// Which goals a read covers. There is no implicit fallback between them.
#[derive(Debug, Clone)]
pub enum Scope {
    /// Every registered project.
    Global,
    /// Exactly these goals; an empty set reads nothing.
    Goals(Vec<GoalRef>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    /// Open or answered: what still needs a human or the coordinator.
    Outstanding,
    Open,
    Answered,
    Closed,
    All,
}

impl StateFilter {
    fn matches(self, state: RequestState) -> bool {
        match self {
            Self::Outstanding => state != RequestState::Closed,
            Self::Open => state == RequestState::Open,
            Self::Answered => state == RequestState::Answered,
            Self::Closed => state == RequestState::Closed,
            Self::All => true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct GoalAttention {
    pub project: Project,
    pub project_name: String,
    pub goal: String,
    pub title: String,
    pub status: Status,
    /// Current holder of the root claim. A claim is not evidence that its
    /// session is still running; `null` means nobody coordinates the goal.
    pub coordinator: Option<String>,
    /// The newest mandate, if one was recorded.
    pub mandate: Option<Decision>,
    pub requests: Vec<Request>,
}

#[derive(Debug, Serialize)]
pub struct GoalDecisions {
    pub project: Project,
    pub project_name: String,
    pub goal: String,
    pub title: String,
    pub decisions: Vec<Decision>,
}

/// A project or goal in scope that could not be read.
#[derive(Debug, Serialize)]
pub struct ScopeError {
    pub project: String,
    pub goal: Option<String>,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct AttentionList {
    pub goals: Vec<GoalAttention>,
    pub errors: Vec<ScopeError>,
}

#[derive(Debug, Serialize)]
pub struct DecisionList {
    pub goals: Vec<GoalDecisions>,
    pub errors: Vec<ScopeError>,
}

impl Store {
    /// Raise a request for a goal's coordinator. The same key with identical
    /// content returns the existing request in whatever state it reached.
    pub fn raise_request(
        &mut self,
        project: &Project,
        goal: &str,
        key: &str,
        content: &RequestContent,
        actor: Actor<'_>,
    ) -> Result<Raised, Error> {
        validate_key(key)?;
        let content = normalized(content)?;
        let tx = self.write()?;
        require_goal(&tx, project, goal)?;
        store::authorize(&tx, goal, actor.name)?;
        validate_tasks(&tx, project, goal, &content.tasks)?;
        let current: Option<String> = tx
            .query_row(
                "SELECT id FROM requests WHERE goal=?1 AND key=?2 AND closure IS NOT 'superseded'",
                params![goal, key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = current {
            let existing = load(&tx, project, &id)?;
            if stored_content(&existing) != content {
                let reason = if existing.state == RequestState::Open {
                    format!(
                        "already uses key {key} with different content; supersede it to change the question"
                    )
                } else {
                    format!(
                        "uses key {key} and is {}; choose a new key for a different question",
                        existing.state.as_str()
                    )
                };
                return Err(Error::RequestConflict { id, reason });
            }
            return Ok(Raised {
                request: existing,
                created: false,
                held_claims: Vec::new(),
            });
        }
        let id = insert(&tx, project, goal, key, &content, None, actor)?;
        store::record(&tx, goal, actor, EventKind::RequestRaised, Some(&id))?;
        let held_claims = held_claims(&tx, goal, &content.tasks)?;
        let request = load(&tx, project, &id)?;
        tx.commit()?;
        Ok(Raised {
            request,
            created: true,
            held_claims,
        })
    }

    /// Replace an open request's content with a new identity under the same
    /// key. An answered request must be incorporated instead.
    pub fn supersede_request(
        &mut self,
        project: &Project,
        id: &str,
        content: &RequestContent,
        actor: Actor<'_>,
    ) -> Result<Raised, Error> {
        let content = normalized(content)?;
        let tx = self.write()?;
        let old = load(&tx, project, id)?;
        store::authorize(&tx, &old.goal, actor.name)?;
        require_open(&old)?;
        validate_tasks(&tx, project, &old.goal, &content.tasks)?;
        if stored_content(&old) == content {
            return Err(Error::Invalid(
                "replacement content is identical; nothing to supersede",
            ));
        }
        close(&tx, id, ClosureKind::Superseded, actor, None)?;
        let replacement = insert(&tx, project, &old.goal, &old.key, &content, Some(id), actor)?;
        store::record(
            &tx,
            &old.goal,
            actor,
            EventKind::RequestSuperseded,
            Some(&format!("{id} -> {replacement}")),
        )?;
        let held_claims = held_claims(&tx, &old.goal, &content.tasks)?;
        let request = load(&tx, project, &replacement)?;
        tx.commit()?;
        Ok(Raised {
            request,
            created: true,
            held_claims,
        })
    }

    /// Close an open request without an answer.
    pub fn withdraw_request(
        &mut self,
        project: &Project,
        id: &str,
        reason: &str,
        actor: Actor<'_>,
    ) -> Result<Request, Error> {
        multiline(reason, 1024, "withdrawal reason: 1..1024 bytes")?;
        let tx = self.write()?;
        let request = load(&tx, project, id)?;
        store::authorize(&tx, &request.goal, actor.name)?;
        require_open(&request)?;
        close(&tx, id, ClosureKind::Withdrawn, actor, Some(reason))?;
        store::record(
            &tx,
            &request.goal,
            actor,
            EventKind::RequestWithdrawn,
            Some(id),
        )?;
        let request = load(&tx, project, id)?;
        tx.commit()?;
        Ok(request)
    }

    /// Record a human response to an open request, from any session.
    pub fn answer_request(
        &mut self,
        project: &Project,
        id: &str,
        answer: &AnswerContent,
        actor: Actor<'_>,
    ) -> Result<Request, Error> {
        validate_answer(answer)?;
        let tx = self.write()?;
        let request = load(&tx, project, id)?;
        require_open(&request)?;
        if let Some(choice) = &answer.choice
            && request.choices.iter().all(|known| &known.id != choice)
        {
            return Err(Error::RequestConflict {
                id: id.into(),
                reason: format!(
                    "has no choice {choice}; valid choices are {}",
                    choice_list(&request.choices)
                ),
            });
        }
        let changed = tx.execute(
            "UPDATE requests SET state='answered', answer_choice=?2, answer_text=?3, answer_source=?4,
               answer_provenance=?5, answer_interpretation=?6, answered_at=?7, answered_by=?8
             WHERE id=?1 AND state='open'",
            params![
                id,
                answer.choice,
                answer.text,
                source_str(answer.source),
                answer.provenance,
                answer.interpretation,
                actor.now,
                actor.name.map(Owner::as_str)
            ],
        )?;
        if changed != 1 {
            return Err(Error::Invalid("request changed state during answer"));
        }
        store::record(
            &tx,
            &request.goal,
            actor,
            EventKind::RequestAnswered,
            Some(id),
        )?;
        let request = load(&tx, project, id)?;
        tx.commit()?;
        Ok(request)
    }

    /// Record the coordinator's decision on an answered request and close it,
    /// both or neither.
    pub fn incorporate_request(
        &mut self,
        project: &Project,
        id: &str,
        incorporation: &Incorporation,
        actor: Actor<'_>,
    ) -> Result<Incorporated, Error> {
        validate_decision(&incorporation.decision)?;
        let tx = self.write()?;
        let request = load(&tx, project, id)?;
        store::authorize(&tx, &request.goal, actor.name)?;
        match request.state {
            RequestState::Answered => {}
            RequestState::Open => {
                return Err(Error::RequestConflict {
                    id: id.into(),
                    reason: "has no answer yet; wait for one, or withdraw or supersede it".into(),
                });
            }
            RequestState::Closed => return Err(not_open(&request)),
        }
        let decision = insert_decision(
            &tx,
            project,
            &request.goal,
            incorporation.kind,
            &incorporation.decision,
            None,
            Some(id),
            actor,
        )?;
        close(&tx, id, ClosureKind::Incorporated, actor, None)?;
        store::record(
            &tx,
            &request.goal,
            actor,
            EventKind::RequestIncorporated,
            Some(&format!("{id} -> {decision}")),
        )?;
        let incorporated = Incorporated {
            request: load(&tx, project, id)?,
            decision: load_decision(&tx, project, &decision)?,
        };
        tx.commit()?;
        Ok(incorporated)
    }

    /// Record a decision or mandate established outside any request.
    pub fn record_decision(
        &mut self,
        project: &Project,
        goal: &str,
        content: &DecisionContent,
        actor: Actor<'_>,
    ) -> Result<Decision, Error> {
        validate_decision(&content.decision)?;
        multiline(&content.human.text, 4096, "human text: 1..4096 bytes")?;
        validate_provenance(content.human.provenance.as_deref())?;
        let tx = self.write()?;
        require_goal(&tx, project, goal)?;
        store::authorize(&tx, goal, actor.name)?;
        let id = insert_decision(
            &tx,
            project,
            goal,
            content.kind,
            &content.decision,
            Some(&content.human),
            None,
            actor,
        )?;
        store::record(&tx, goal, actor, EventKind::DecisionRecorded, Some(&id))?;
        let decision = load_decision(&tx, project, &id)?;
        tx.commit()?;
        Ok(decision)
    }

    pub fn request(&mut self, project: &Project, id: &str) -> Result<Request, Error> {
        let tx = self.connection.transaction()?;
        load(&tx, project, id)
    }

    /// Requests in scope. Global reads list goals with matching requests and
    /// unfinished goals with a mandate; exact reads list every named goal.
    /// Unreadable projects and goals are reported, never skipped silently.
    pub fn attention(
        &mut self,
        scope: &Scope,
        filter: StateFilter,
    ) -> Result<AttentionList, Error> {
        let mut list = AttentionList {
            goals: Vec::new(),
            errors: Vec::new(),
        };
        for (project, goals) in self.scope_goals(scope, &mut list.errors)? {
            let tx = self.connection.transaction()?;
            for goal in goals {
                match goal_attention(&tx, &project, goal.as_deref(), filter) {
                    Ok(found) => list.goals.extend(found),
                    Err(error) => list
                        .errors
                        .push(scope_error(project.0.as_str(), goal, &error)),
                }
            }
        }
        Ok(list)
    }

    /// Decisions in scope, oldest first. Global reads list goals with any.
    pub fn decisions(
        &mut self,
        scope: &Scope,
        kind: Option<DecisionKind>,
    ) -> Result<DecisionList, Error> {
        let mut list = DecisionList {
            goals: Vec::new(),
            errors: Vec::new(),
        };
        for (project, goals) in self.scope_goals(scope, &mut list.errors)? {
            let tx = self.connection.transaction()?;
            for goal in goals {
                match goal_decisions(&tx, &project, goal.as_deref(), kind) {
                    Ok(found) => list.goals.extend(found),
                    Err(error) => list
                        .errors
                        .push(scope_error(project.0.as_str(), goal, &error)),
                }
            }
        }
        Ok(list)
    }

    /// Registered projects in scope, each with exact goals or `None` for all.
    /// An unregistered project in an exact scope becomes an error entry.
    fn scope_goals(
        &mut self,
        scope: &Scope,
        errors: &mut Vec<ScopeError>,
    ) -> Result<Vec<ScopedGoals>, Error> {
        match scope {
            Scope::Global => Ok(self
                .registered_projects()?
                .into_iter()
                .map(|project| ((project.id, project.name), vec![None]))
                .collect()),
            Scope::Goals(goals) => {
                let mut selected = Vec::new();
                for goal in goals {
                    match self.registered_project(&goal.project) {
                        Ok(project) => selected
                            .push(((project.id, project.name), vec![Some(goal.goal.clone())])),
                        Err(error @ Error::UnregisteredProject(_)) => errors.push(scope_error(
                            goal.project.as_str(),
                            Some(goal.goal.clone()),
                            &error,
                        )),
                        Err(error) => return Err(error),
                    }
                }
                Ok(selected)
            }
        }
    }
}

type ProjectName = (Project, String);

/// A project in scope with its exact goals, or `None` for all of them.
type ScopedGoals = (ProjectName, Vec<Option<String>>);

fn scope_error(project: &str, goal: Option<String>, error: &Error) -> ScopeError {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    ScopeError {
        project: project.to_owned(),
        goal,
        error: text,
    }
}

/// Goals of `project` in scope: exactly `goal`, or for a global read those
/// with matching requests and unfinished goals with a mandate.
fn goal_attention(
    tx: &Transaction<'_>,
    (project, name): &ProjectName,
    goal: Option<&str>,
    filter: StateFilter,
) -> Result<Vec<GoalAttention>, Error> {
    let roots = if let Some(goal) = goal {
        vec![goal.to_owned()]
    } else {
        let mut statement = tx.prepare(
                "SELECT tasks.id FROM tasks JOIN goals ON goals.root=tasks.id WHERE tasks.project=?1
                   AND (EXISTS(SELECT 1 FROM requests WHERE requests.goal=tasks.id)
                     OR (tasks.status NOT IN ('done','dropped')
                         AND EXISTS(SELECT 1 FROM decisions WHERE decisions.goal=tasks.id AND decisions.kind='mandate')))
                 ORDER BY tasks.created_at, tasks.rowid",
            )?;
        statement
            .query_map([project.as_str()], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    let mut found = Vec::new();
    for root in roots {
        let task = require_goal(tx, project, &root)?;
        let requests = goal_requests(tx, project, &root)?
            .into_iter()
            .filter(|request| filter.matches(request.state))
            .collect::<Vec<_>>();
        let mandate = goal_decision_list(tx, project, &root, Some(DecisionKind::Mandate))?.pop();
        let discoverable = !requests.is_empty() || (mandate.is_some() && !task.status.is_closed());
        if goal.is_some() || discoverable {
            found.push(GoalAttention {
                project: project.clone(),
                project_name: name.clone(),
                goal: root,
                title: task.title,
                status: task.status,
                coordinator: task.owner,
                mandate,
                requests,
            });
        }
    }
    Ok(found)
}

fn goal_decisions(
    tx: &Transaction<'_>,
    (project, name): &ProjectName,
    goal: Option<&str>,
    kind: Option<DecisionKind>,
) -> Result<Vec<GoalDecisions>, Error> {
    let roots = if let Some(goal) = goal {
        vec![goal.to_owned()]
    } else {
        let mut statement = tx.prepare(
            "SELECT tasks.id FROM tasks JOIN goals ON goals.root=tasks.id WHERE tasks.project=?1
                   AND EXISTS(SELECT 1 FROM decisions WHERE decisions.goal=tasks.id)
                 ORDER BY tasks.created_at, tasks.rowid",
        )?;
        statement
            .query_map([project.as_str()], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    let mut found = Vec::new();
    for root in roots {
        let task = require_goal(tx, project, &root)?;
        let decisions = goal_decision_list(tx, project, &root, kind)?;
        if goal.is_some() || !decisions.is_empty() {
            found.push(GoalDecisions {
                project: project.clone(),
                project_name: name.clone(),
                goal: root,
                title: task.title,
                decisions,
            });
        }
    }
    Ok(found)
}

fn goal_requests(
    tx: &Transaction<'_>,
    project: &Project,
    goal: &str,
) -> Result<Vec<Request>, Error> {
    let ids = {
        let mut statement =
            tx.prepare("SELECT id FROM requests WHERE goal=?1 ORDER BY created_at, rowid")?;
        statement
            .query_map([goal], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    ids.iter().map(|id| load(tx, project, id)).collect()
}

fn goal_decision_list(
    tx: &Transaction<'_>,
    project: &Project,
    goal: &str,
    kind: Option<DecisionKind>,
) -> Result<Vec<Decision>, Error> {
    let ids = {
        let mut statement = tx.prepare(
            "SELECT id FROM decisions WHERE goal=?1 AND (?2 IS NULL OR kind=?2) ORDER BY at, rowid",
        )?;
        statement
            .query_map(params![goal, kind.map(decision_kind_str)], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?
    };
    ids.iter()
        .map(|id| load_decision(tx, project, id))
        .collect()
}

/// The goal root `goal` in `project`, or an error naming what it is not.
fn require_goal(tx: &Transaction<'_>, project: &Project, goal: &str) -> Result<store::Task, Error> {
    let task = store::load(tx, project, goal)?;
    if !task.is_goal_root() {
        return Err(Error::Invalid("task has no goal association"));
    }
    Ok(task)
}

/// Refuse a transition that needs an open request, naming what became of it.
fn require_open(request: &Request) -> Result<(), Error> {
    if request.state == RequestState::Open {
        Ok(())
    } else {
        Err(not_open(request))
    }
}

fn not_open(request: &Request) -> Error {
    Error::RequestNotOpen {
        id: request.id.clone(),
        state: request.state,
        closure: request.closure.as_ref().map(|closure| closure.kind),
        replacement: request.superseded_by.clone(),
    }
}

/// Why a request no longer accepts open-only transitions, and what to do.
pub(crate) fn not_open_reason(
    state: RequestState,
    closure: Option<ClosureKind>,
    replacement: Option<&str>,
) -> String {
    match (state, closure, replacement) {
        (_, _, Some(replacement)) => {
            format!("was superseded by {replacement}; respond to the replacement instead")
        }
        (RequestState::Answered, _, None) => {
            "is already answered; its coordinator incorporates the answer".into()
        }
        (_, Some(closure), None) => format!("is closed ({})", closure_str(closure)),
        (state, None, None) => format!("is {}", state.as_str()),
    }
}

fn choice_list(choices: &[Choice]) -> String {
    if choices.is_empty() {
        "none; answer with text".into()
    } else {
        choices
            .iter()
            .map(|choice| choice.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn insert(
    tx: &Transaction<'_>,
    project: &Project,
    goal: &str,
    key: &str,
    content: &RequestContent,
    supersedes: Option<&str>,
    actor: Actor<'_>,
) -> Result<String, Error> {
    let id = store::fresh_id(tx, "r", "requests")?;
    tx.execute(
        "INSERT INTO requests(id,project,goal,key,kind,title,question,context,recommendation,state,created_at,created_by,supersedes)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'open',?10,?11,?12)",
        params![
            id,
            project.as_str(),
            goal,
            key,
            request_kind_str(content.kind),
            content.title,
            content.question,
            content.context,
            content.recommendation,
            actor.now,
            actor.name.map(Owner::as_str),
            supersedes
        ],
    )?;
    for (position, choice) in content.choices.iter().enumerate() {
        tx.execute(
            "INSERT INTO request_choices(request,position,id,label,detail) VALUES(?1,?2,?3,?4,?5)",
            params![
                id,
                i64::try_from(position).expect("bounded choice count"),
                choice.id,
                choice.label,
                choice.detail
            ],
        )?;
    }
    for task in &content.tasks {
        tx.execute(
            "INSERT INTO request_tasks(request,task) VALUES(?1,?2)",
            params![id, task],
        )?;
    }
    Ok(id)
}

fn close(
    tx: &Transaction<'_>,
    id: &str,
    kind: ClosureKind,
    actor: Actor<'_>,
    reason: Option<&str>,
) -> Result<(), Error> {
    let from = if kind == ClosureKind::Incorporated {
        "answered"
    } else {
        "open"
    };
    let changed = tx.execute(
        "UPDATE requests SET state='closed', closure=?2, closed_at=?3, closed_by=?4, close_reason=?5
         WHERE id=?1 AND state=?6",
        params![
            id,
            closure_str(kind),
            actor.now,
            actor.name.map(Owner::as_str),
            reason,
            from
        ],
    )?;
    if changed != 1 {
        return Err(Error::Invalid("request changed state during close"));
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "one insert for both standalone and incorporated decisions"
)]
fn insert_decision(
    tx: &Transaction<'_>,
    project: &Project,
    goal: &str,
    kind: DecisionKind,
    decision: &str,
    human: Option<&HumanInput>,
    request: Option<&str>,
    actor: Actor<'_>,
) -> Result<String, Error> {
    let id = store::fresh_id(tx, "d", "decisions")?;
    tx.execute(
        "INSERT INTO decisions(id,project,goal,kind,decision,human_text,human_source,human_provenance,request,at,actor)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id,
            project.as_str(),
            goal,
            decision_kind_str(kind),
            decision,
            human.map(|human| human.text.as_str()),
            human.map(|human| source_str(human.source)),
            human.and_then(|human| human.provenance.as_deref()),
            request,
            actor.now,
            actor.name.map(Owner::as_str)
        ],
    )?;
    Ok(id)
}

/// Claims on the named tasks and their descendants, other than goal roots.
fn held_claims(
    tx: &Transaction<'_>,
    goal: &str,
    tasks: &[String],
) -> Result<Vec<HeldClaim>, Error> {
    let mut claims = Vec::new();
    let mut statement = tx.prepare(
        "WITH RECURSIVE subtree(id) AS (SELECT ?1 UNION SELECT tasks.id FROM tasks JOIN subtree ON tasks.parent=subtree.id)
         SELECT id, owner FROM tasks WHERE id IN subtree AND id<>?2 AND owner IS NOT NULL ORDER BY created_at, rowid",
    )?;
    for task in tasks {
        for row in statement.query_map(params![task, goal], |row| {
            Ok(HeldClaim {
                task: row.get(0)?,
                owner: row.get(1)?,
            })
        })? {
            let claim = row?;
            if !claims.contains(&claim) {
                claims.push(claim);
            }
        }
    }
    Ok(claims)
}

#[expect(
    clippy::too_many_lines,
    reason = "one consistent read of a request row, its choices, tasks, answer and closure"
)]
fn load(tx: &Transaction<'_>, project: &Project, id: &str) -> Result<Request, Error> {
    type Row = (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        i64,
        Option<String>,
        Option<String>,
    );
    let row: Row = tx
        .query_row(
            "SELECT goal,key,kind,title,question,context,recommendation,state,created_at,created_by,supersedes
             FROM requests WHERE id=?1 AND project=?2",
            params![id, project.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| Error::RequestNotFound(id.to_owned()))?;
    let (
        goal,
        key,
        kind,
        title,
        question,
        context,
        recommendation,
        state,
        created_at,
        created_by,
        supersedes,
    ) = row;
    let corrupt = |field| Error::Corrupt {
        id: id.to_owned(),
        field,
    };
    let kind = parse_request_kind(&kind).ok_or_else(|| corrupt("kind"))?;
    let state = RequestState::parse(&state).ok_or_else(|| corrupt("state"))?;
    let choices = {
        let mut statement = tx.prepare(
            "SELECT id,label,detail FROM request_choices WHERE request=?1 ORDER BY position",
        )?;
        statement
            .query_map([id], |row| {
                Ok(Choice {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    detail: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let tasks = {
        let mut statement = tx.prepare(
            "SELECT tasks.id,tasks.title,tasks.status FROM request_tasks JOIN tasks ON tasks.id=request_tasks.task
             WHERE request_tasks.request=?1 ORDER BY tasks.id",
        )?;
        let rows = statement.query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (task, title, status) = row?;
            let status = Status::parse(&status).ok_or(Error::Corrupt {
                id: task.clone(),
                field: "status",
            })?;
            Ok(RequestTask {
                id: task,
                title,
                status,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?
    };
    let answer = load_answer(tx, id)?;
    let closure = load_closure(tx, id)?;
    if (state == RequestState::Open) != answer.is_none() && state != RequestState::Closed
        || (state == RequestState::Closed) != closure.is_some()
    {
        return Err(corrupt("state"));
    }
    let superseded_by = tx
        .query_row("SELECT id FROM requests WHERE supersedes=?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(Request {
        id: id.to_owned(),
        project: project.clone(),
        goal,
        key,
        kind,
        title,
        question,
        context,
        recommendation,
        choices,
        tasks,
        state,
        created_at,
        created_by,
        supersedes,
        superseded_by,
        answer,
        closure,
    })
}

fn load_answer(tx: &Transaction<'_>, id: &str) -> Result<Option<Answer>, Error> {
    type Row = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let row: Row = tx.query_row(
        "SELECT answer_choice,answer_text,answer_source,answer_provenance,answer_interpretation,answered_at,answered_by
         FROM requests WHERE id=?1",
        [id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        },
    )?;
    let (choice, text, source, provenance, interpretation, at, by) = row;
    let (source, at) = match (source, at) {
        (None, None) => return Ok(None),
        (Some(source), Some(at)) => (source, at),
        _ => {
            return Err(Error::Corrupt {
                id: id.to_owned(),
                field: "answer",
            });
        }
    };
    let source = parse_source(&source).ok_or_else(|| Error::Corrupt {
        id: id.to_owned(),
        field: "answer source",
    })?;
    Ok(Some(Answer {
        content: AnswerContent {
            choice,
            text,
            source,
            provenance,
            interpretation,
        },
        at,
        by,
    }))
}

fn load_closure(tx: &Transaction<'_>, id: &str) -> Result<Option<Closure>, Error> {
    let row: (Option<String>, Option<i64>, Option<String>, Option<String>) = tx.query_row(
        "SELECT closure,closed_at,closed_by,close_reason FROM requests WHERE id=?1",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let (kind, at, by, reason) = match row {
        (None, None, _, _) => return Ok(None),
        (Some(kind), Some(at), by, reason) => (kind, at, by, reason),
        _ => {
            return Err(Error::Corrupt {
                id: id.to_owned(),
                field: "closure",
            });
        }
    };
    let kind = parse_closure(&kind).ok_or_else(|| Error::Corrupt {
        id: id.to_owned(),
        field: "closure",
    })?;
    let decision = tx
        .query_row("SELECT id FROM decisions WHERE request=?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    if (kind == ClosureKind::Incorporated) != decision.is_some() {
        return Err(Error::Corrupt {
            id: id.to_owned(),
            field: "decision",
        });
    }
    Ok(Some(Closure {
        kind,
        at,
        by,
        reason,
        decision,
    }))
}

fn load_decision(tx: &Transaction<'_>, project: &Project, id: &str) -> Result<Decision, Error> {
    type Row = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<String>,
    );
    let row: Row = tx.query_row(
        "SELECT goal,kind,decision,human_text,human_source,human_provenance,request,at,actor
         FROM decisions WHERE id=?1 AND project=?2",
        params![id, project.as_str()],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        },
    )?;
    let (goal, kind, decision, human_text, human_source, human_provenance, request, at, by) = row;
    let corrupt = |field| Error::Corrupt {
        id: id.to_owned(),
        field,
    };
    let kind = parse_decision_kind(&kind).ok_or_else(|| corrupt("kind"))?;
    let human = match (human_text, human_source) {
        (None, None) => None,
        (Some(text), Some(source)) => Some(HumanInput {
            text,
            source: parse_source(&source).ok_or_else(|| corrupt("human source"))?,
            provenance: human_provenance,
        }),
        _ => return Err(corrupt("human input")),
    };
    if human.is_some() == request.is_some() {
        return Err(corrupt("human input"));
    }
    Ok(Decision {
        id: id.to_owned(),
        project: project.clone(),
        goal,
        kind,
        decision,
        human,
        request,
        at,
        by,
    })
}

/// The comparable content a stored request was raised with.
fn stored_content(request: &Request) -> RequestContent {
    RequestContent {
        kind: request.kind,
        title: request.title.clone(),
        question: request.question.clone(),
        context: request.context.clone(),
        recommendation: request.recommendation.clone(),
        choices: request.choices.clone(),
        tasks: request.tasks.iter().map(|task| task.id.clone()).collect(),
    }
}

/// Validated content in its stored form: trimmed title, sorted unique tasks.
fn normalized(content: &RequestContent) -> Result<RequestContent, Error> {
    let mut content = content.clone();
    content.title = content.title.trim().to_owned();
    single_line(&content.title, 120, "request title: one line, 1..120 bytes")?;
    multiline(&content.question, 4096, "request question: 1..4096 bytes")?;
    if let Some(context) = &content.context {
        multiline(context, 8192, "request context: 1..8192 bytes when given")?;
    }
    if let Some(recommendation) = &content.recommendation {
        multiline(
            recommendation,
            1024,
            "request recommendation: 1..1024 bytes when given",
        )?;
    }
    if content.kind == RequestKind::Action && !content.choices.is_empty() {
        return Err(Error::Invalid(
            "request choices: an action request has no choices",
        ));
    }
    if content.choices.len() > MAX_CHOICES {
        return Err(Error::Invalid("request choices: at most 8"));
    }
    for (index, choice) in content.choices.iter().enumerate() {
        if choice.id.is_empty()
            || choice.id.len() > 32
            || !choice
                .id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(Error::Invalid(
                "request choice id: 1..32 lowercase letters, digits or hyphens",
            ));
        }
        if content.choices[..index]
            .iter()
            .any(|earlier| earlier.id == choice.id)
        {
            return Err(Error::Invalid("request choice ids must be unique"));
        }
        single_line(
            &choice.label,
            80,
            "request choice label: one line, 1..80 bytes",
        )?;
        if let Some(detail) = &choice.detail {
            multiline(
                detail,
                512,
                "request choice detail: 1..512 bytes when given",
            )?;
        }
    }
    content.tasks.sort();
    content.tasks.dedup();
    if content.tasks.len() > MAX_TASKS {
        return Err(Error::Invalid("request tasks: at most 32"));
    }
    Ok(content)
}

fn validate_tasks(
    tx: &Transaction<'_>,
    project: &Project,
    goal: &str,
    tasks: &[String],
) -> Result<(), Error> {
    for task in tasks {
        if store::load(tx, project, task)?.goal.as_deref() != Some(goal) {
            return Err(Error::Invalid(
                "request tasks: each must be the goal root or one of its descendants",
            ));
        }
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<(), Error> {
    let mut bytes = key.bytes();
    let valid_first = bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    if !valid_first
        || key.len() > 64
        || !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(Error::Invalid(
            "request key: 1..64 of a-z, 0-9, '.', '_' or '-', starting with a letter or digit",
        ));
    }
    Ok(())
}

fn validate_answer(answer: &AnswerContent) -> Result<(), Error> {
    if answer.choice.is_none() && answer.text.is_none() {
        return Err(Error::Invalid("answer: give a choice, text, or both"));
    }
    if let Some(text) = &answer.text {
        multiline(text, 4096, "answer text: 1..4096 bytes when given")?;
    }
    validate_provenance(answer.provenance.as_deref())?;
    match (answer.source, &answer.interpretation) {
        (Source::Ui, Some(_)) => Err(Error::Invalid(
            "answer interpretation: an explicit UI answer carries no interpretation",
        )),
        (Source::Chat, None) => Err(Error::Invalid(
            "answer interpretation: a chat answer records how the agent read the message",
        )),
        (Source::Chat, Some(_)) if answer.text.is_none() => Err(Error::Invalid(
            "answer text: a chat answer quotes the relevant user instruction",
        )),
        (Source::Chat, Some(interpretation)) => {
            multiline(interpretation, 4096, "answer interpretation: 1..4096 bytes")
        }
        (Source::Ui, None) => Ok(()),
    }
}

fn validate_decision(decision: &str) -> Result<(), Error> {
    multiline(decision, 8192, "decision: 1..8192 bytes")
}

fn validate_provenance(provenance: Option<&str>) -> Result<(), Error> {
    provenance.map_or(Ok(()), |provenance| {
        single_line(
            provenance,
            512,
            "provenance: one line, 1..512 bytes when given",
        )
    })
}

fn single_line(value: &str, max: usize, message: &'static str) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::Invalid(message));
    }
    Ok(())
}

fn multiline(value: &str, max: usize, message: &'static str) -> Result<(), Error> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        return Err(Error::Invalid(message));
    }
    Ok(())
}

fn request_kind_str(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::Decision => "decision",
        RequestKind::Authorization => "authorization",
        RequestKind::Action => "action",
    }
}

fn parse_request_kind(value: &str) -> Option<RequestKind> {
    [
        RequestKind::Decision,
        RequestKind::Authorization,
        RequestKind::Action,
    ]
    .into_iter()
    .find(|kind| request_kind_str(*kind) == value)
}

fn source_str(source: Source) -> &'static str {
    match source {
        Source::Ui => "ui",
        Source::Chat => "chat",
    }
}

fn parse_source(value: &str) -> Option<Source> {
    [Source::Ui, Source::Chat]
        .into_iter()
        .find(|source| source_str(*source) == value)
}

fn decision_kind_str(kind: DecisionKind) -> &'static str {
    match kind {
        DecisionKind::Mandate => "mandate",
        DecisionKind::Decision => "decision",
    }
}

fn parse_decision_kind(value: &str) -> Option<DecisionKind> {
    [DecisionKind::Mandate, DecisionKind::Decision]
        .into_iter()
        .find(|kind| decision_kind_str(*kind) == value)
}

fn closure_str(kind: ClosureKind) -> &'static str {
    match kind {
        ClosureKind::Incorporated => "incorporated",
        ClosureKind::Withdrawn => "withdrawn",
        ClosureKind::Superseded => "superseded",
    }
}

fn parse_closure(value: &str) -> Option<ClosureKind> {
    [
        ClosureKind::Incorporated,
        ClosureKind::Withdrawn,
        ClosureKind::Superseded,
    ]
    .into_iter()
    .find(|kind| closure_str(*kind) == value)
}
