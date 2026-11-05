//! `dots attention`: content files, scope selection, and presentation over
//! the store's requests and decisions.
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use anyhow::{Context as _, Result, ensure};
use dots_tasks::attention::{
    AnswerContent, Decision, DecisionContent, DecisionKind, GoalRef, Incorporated, Incorporation,
    Raised, Request, RequestContent, Scope, ScopeError, StateFilter,
};
use dots_tasks::{Actor, Owner, Store};
use serde::de::DeserializeOwned;
use serde_json::json;
use time::OffsetDateTime;

use crate::command::{AttentionCommand, AttentionScope, DecisionKinds, RequestStates, ScopeArgs};

/// Content files are small JSON documents; anything larger is a mistake.
const MAX_CONTENT_BYTES: u64 = 64 * 1024;

pub(crate) struct Invocation<'request> {
    pub command: AttentionCommand,
    pub project: Option<String>,
    pub owner: Option<&'request str>,
    pub directory: &'request Path,
    pub json: bool,
}

pub(crate) fn run(invocation: Invocation<'_>) -> Result<String> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let runtime = crate::agent::owner();
    let actor = Actor {
        name: invocation
            .owner
            .or(runtime.as_deref())
            .map(Owner::try_new)
            .transpose()?,
        now,
    };
    let json = invocation.json;
    let mut store = crate::project::open()?;
    let value = match invocation.command {
        AttentionCommand::List { scope, state } => {
            ensure!(
                invocation.project.is_none(),
                "list reads the --scope it is given; --project does not apply"
            );
            let list = store.attention(&scope.scope()?, filter(state))?;
            if !json {
                return Ok(list_text(&list.goals, &list.errors));
            }
            json!({"scope": scope.name(), "state": state_name(state), "now": now,
                   "goals": list.goals, "errors": list.errors})
        }
        AttentionCommand::Decisions { scope, kind } => {
            ensure!(
                invocation.project.is_none(),
                "decisions reads the --scope it is given; --project does not apply"
            );
            let list = store.decisions(&scope.scope()?, kind.map(decision_kind))?;
            if !json {
                let mut out = String::new();
                for goal in &list.goals {
                    let _ = writeln!(out, "{} {}  {}", goal.project_name, goal.goal, goal.title);
                    for decision in &goal.decisions {
                        out.push_str(&decision_text(decision));
                    }
                }
                out.push_str(&errors_text(&list.errors));
                return Ok(if out.is_empty() {
                    "No decisions in scope.\n".into()
                } else {
                    out
                });
            }
            json!({"scope": scope.name(), "now": now, "goals": list.goals, "errors": list.errors})
        }
        command => {
            let project = crate::project::resolve(
                &mut store,
                invocation.directory,
                invocation.project.as_deref(),
            )?;
            let outcome = mutate(&mut store, &project, command, actor)?;
            if !json {
                return Ok(outcome.text());
            }
            let mut value = outcome.json()?;
            if let serde_json::Value::Object(fields) = &mut value {
                fields.insert("project".into(), serde_json::to_value(&project)?);
                fields.insert("now".into(), now.into());
            }
            value
        }
    };
    Ok(crate::json::render(value))
}

/// The result of a command on one request or decision.
enum Outcome {
    Raised {
        raised: Raised,
        superseded: Option<String>,
    },
    Request(Request),
    Incorporated(Incorporated),
    Decision(Decision),
}

impl Outcome {
    fn json(&self) -> Result<serde_json::Value> {
        Ok(match self {
            Self::Raised { raised, superseded } => {
                let mut value = serde_json::to_value(raised)?;
                if let (Some(superseded), serde_json::Value::Object(fields)) =
                    (superseded, &mut value)
                {
                    fields.insert("superseded".into(), superseded.clone().into());
                }
                value
            }
            Self::Request(request) => json!({ "request": request }),
            Self::Incorporated(incorporated) => serde_json::to_value(incorporated)?,
            Self::Decision(decision) => json!({ "decision": decision }),
        })
    }

    fn text(&self) -> String {
        match self {
            Self::Raised { raised, superseded } => {
                let mut out = request_text(&raised.request);
                if let Some(superseded) = superseded {
                    let _ = writeln!(out, "Superseded {superseded}.");
                }
                if !raised.created {
                    out.push_str("This key already had this request; nothing new was raised.\n");
                }
                for claim in &raised.held_claims {
                    let _ = writeln!(
                        out,
                        "Warning: {} is claimed by {} and is now held; steer that work",
                        claim.task, claim.owner
                    );
                }
                out
            }
            Self::Request(request) => request_text(request),
            Self::Incorporated(incorporated) => {
                let mut out = request_text(&incorporated.request);
                out.push_str(&decision_text(&incorporated.decision));
                out
            }
            Self::Decision(decision) => decision_text(decision),
        }
    }
}

fn mutate(
    store: &mut Store,
    project: &dots_tasks::Project,
    command: AttentionCommand,
    actor: Actor<'_>,
) -> Result<Outcome> {
    Ok(match command {
        AttentionCommand::Raise {
            goal,
            key,
            content_file,
        } => {
            let content: RequestContent = content(&content_file)?;
            Outcome::Raised {
                raised: store.raise_request(project, &goal, &key, &content, actor)?,
                superseded: None,
            }
        }
        AttentionCommand::Supersede {
            request,
            content_file,
        } => {
            let content: RequestContent = content(&content_file)?;
            Outcome::Raised {
                raised: store.supersede_request(project, &request, &content, actor)?,
                superseded: Some(request),
            }
        }
        AttentionCommand::Withdraw { request, reason } => {
            Outcome::Request(store.withdraw_request(project, &request, &reason, actor)?)
        }
        AttentionCommand::Answer {
            request,
            content_file,
        } => {
            let answer: AnswerContent = content(&content_file)?;
            Outcome::Request(store.answer_request(project, &request, &answer, actor)?)
        }
        AttentionCommand::Incorporate {
            request,
            content_file,
        } => {
            let incorporation: Incorporation = content(&content_file)?;
            Outcome::Incorporated(store.incorporate_request(
                project,
                &request,
                &incorporation,
                actor,
            )?)
        }
        AttentionCommand::Decide { goal, content_file } => {
            let decision: DecisionContent = content(&content_file)?;
            Outcome::Decision(store.record_decision(project, &goal, &decision, actor)?)
        }
        AttentionCommand::Show { request } => Outcome::Request(store.request(project, &request)?),
        AttentionCommand::List { .. } | AttentionCommand::Decisions { .. } => {
            anyhow::bail!("scoped reads are handled before project resolution")
        }
    })
}

impl ScopeArgs {
    /// The store scope; `--goal` belongs only to `--scope goals`.
    pub(crate) fn scope(&self) -> Result<Scope> {
        match self.scope {
            AttentionScope::Global => {
                ensure!(
                    self.goals.is_empty(),
                    "--goal selects exact goals; use it with --scope goals, not --scope global"
                );
                Ok(Scope::Global)
            }
            AttentionScope::Goals => Ok(Scope::Goals(
                self.goals
                    .iter()
                    .map(|goal| GoalRef::parse(goal))
                    .collect::<Result<_, _>>()?,
            )),
        }
    }

    fn name(&self) -> &'static str {
        match self.scope {
            AttentionScope::Global => "global",
            AttentionScope::Goals => "goals",
        }
    }
}

/// A typed JSON content file, or stdin for `-`.
fn content<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    if path == Path::new("-") {
        std::io::stdin()
            .take(MAX_CONTENT_BYTES + 1)
            .read_to_end(&mut bytes)
            .context("read content from stdin")?;
    } else {
        std::fs::File::open(path)
            .and_then(|file| file.take(MAX_CONTENT_BYTES + 1).read_to_end(&mut bytes))
            .with_context(|| format!("read content file {}", path.display()))?;
    }
    ensure!(
        u64::try_from(bytes.len()).is_ok_and(|length| length <= MAX_CONTENT_BYTES),
        "content file {} exceeds 64 KiB",
        path.display()
    );
    serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "content file {} does not match this command's JSON shape; see its --help",
            path.display()
        )
    })
}

fn filter(state: RequestStates) -> StateFilter {
    match state {
        RequestStates::Outstanding => StateFilter::Outstanding,
        RequestStates::Open => StateFilter::Open,
        RequestStates::Answered => StateFilter::Answered,
        RequestStates::Closed => StateFilter::Closed,
        RequestStates::All => StateFilter::All,
    }
}

fn state_name(state: RequestStates) -> &'static str {
    match state {
        RequestStates::Outstanding => "outstanding",
        RequestStates::Open => "open",
        RequestStates::Answered => "answered",
        RequestStates::Closed => "closed",
        RequestStates::All => "all",
    }
}

fn decision_kind(kind: DecisionKinds) -> DecisionKind {
    match kind {
        DecisionKinds::Mandate => DecisionKind::Mandate,
        DecisionKinds::Decision => DecisionKind::Decision,
    }
}

fn list_text(goals: &[dots_tasks::attention::GoalAttention], errors: &[ScopeError]) -> String {
    let mut out = String::new();
    for goal in goals {
        let coordinator = goal.coordinator.as_deref().map_or_else(
            || "no coordinator".to_owned(),
            |owner| format!("root claimed by {owner}"),
        );
        let _ = writeln!(
            out,
            "{} {}  {}  [{}, {coordinator}]",
            goal.project_name,
            goal.goal,
            goal.title,
            goal.status.as_str()
        );
        if let Some(mandate) = &goal.mandate {
            let _ = writeln!(
                out,
                "  Mandate {}: {}",
                mandate.id,
                first_line(&mandate.decision)
            );
        }
        for request in &goal.requests {
            out.push_str(&request_line(request));
        }
    }
    out.push_str(&errors_text(errors));
    if out.is_empty() {
        "Nothing in scope needs attention.\n".into()
    } else {
        out
    }
}

fn errors_text(errors: &[ScopeError]) -> String {
    let mut out = String::new();
    for error in errors {
        let _ = writeln!(
            out,
            "Unreadable: {}{}: {}",
            error.project,
            error
                .goal
                .as_deref()
                .map(|goal| format!(" {goal}"))
                .unwrap_or_default(),
            error.error
        );
    }
    out
}

fn request_line(request: &Request) -> String {
    format!(
        "  {}  {:<8} {}  {}\n",
        request.id,
        request.state.as_str(),
        request.kind.as_str(),
        request.title
    )
}

fn request_text(request: &Request) -> String {
    let mut out = request_line(request).trim_start().to_owned();
    let _ = writeln!(out, "Goal: {}  Key: {}", request.goal, request.key);
    let _ = writeln!(out, "{}", request.question);
    if let Some(context) = &request.context {
        let _ = writeln!(out, "Context: {context}");
    }
    if let Some(recommendation) = &request.recommendation {
        let _ = writeln!(out, "Recommendation: {recommendation}");
    }
    for choice in &request.choices {
        let _ = writeln!(
            out,
            "Choice {}: {}{}",
            choice.id,
            choice.label,
            choice
                .detail
                .as_deref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default()
        );
    }
    for task in &request.tasks {
        let _ = writeln!(
            out,
            "Holds: {} {} [{}]",
            task.id,
            task.title,
            task.status.as_str()
        );
    }
    if let Some(answer) = &request.answer {
        let content = &answer.content;
        let _ = writeln!(
            out,
            "Answer ({}{}): {}{}",
            content.source.as_str(),
            answer
                .by
                .as_deref()
                .map(|by| format!(", by {by}"))
                .unwrap_or_default(),
            content
                .choice
                .as_deref()
                .map(|choice| format!("choice {choice} "))
                .unwrap_or_default(),
            content.text.as_deref().unwrap_or_default()
        );
        if let Some(interpretation) = &content.interpretation {
            let _ = writeln!(out, "Interpretation: {interpretation}");
        }
    }
    if let Some(replacement) = &request.superseded_by {
        let _ = writeln!(out, "Superseded by {replacement}");
    } else if let Some(closure) = &request.closure {
        let _ = writeln!(
            out,
            "Closed: {}{}{}",
            closure.kind.as_str(),
            closure
                .decision
                .as_deref()
                .map(|decision| format!(" as {decision}"))
                .unwrap_or_default(),
            closure
                .reason
                .as_deref()
                .map(|reason| format!(": {reason}"))
                .unwrap_or_default()
        );
    }
    out
}

fn decision_text(decision: &Decision) -> String {
    let mut out = format!(
        "  {} {}: {}\n",
        decision.id,
        decision.kind.as_str(),
        first_line(&decision.decision)
    );
    if let Some(human) = &decision.human {
        let _ = writeln!(out, "    User: {}", first_line(&human.text));
    }
    if let Some(request) = &decision.request {
        let _ = writeln!(out, "    From request {request}");
    }
    out
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}
