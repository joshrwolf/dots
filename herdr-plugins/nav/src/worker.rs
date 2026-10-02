//! Responsive service control in front of one serialized navigation worker.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail, ensure};
use herdrkit::api::Direction;
use herdrkit::service::{Handler, Service};
use herdrkit::{Invocation, InvocationKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine::Engine;
use crate::history::Travel;
use crate::{NavigationDecision, chord, decide};

const QUEUE: usize = 32;
const RESULTS: usize = 96;
const START_DEADLINE: Duration = Duration::from_secs(5);
const ACTION_DEADLINE: Duration = Duration::from_secs(20);
const HOOKS: &[&str] = &[
    "workspace.focused",
    "pane.focused",
    "pane.created",
    "pane.closed",
    "pane.moved",
    "tab.closed",
    "workspace.closed",
];

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Observe,
    Navigate { operation: Operation },
    Result { ticket: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Travel { direction: Travel },
    PreviousWorkspace,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Observed,
    Submitted {
        ticket: String,
    },
    Pending,
    Complete {
        result: std::result::Result<(), String>,
    },
}

#[derive(Debug)]
enum Work {
    Observe,
    Navigate {
        ticket: String,
        operation: Operation,
        deadline: Instant,
    },
}

#[derive(Debug)]
struct Update {
    completed: Option<(String, std::result::Result<(), String>)>,
    status: Value,
    error: Option<String>,
}

#[derive(Debug)]
struct Control {
    sender: mpsc::SyncSender<Work>,
    receiver: mpsc::Receiver<Update>,
    observing: Arc<AtomicBool>,
    results: VecDeque<(String, Option<std::result::Result<(), String>>)>,
    epoch: String,
    next: u64,
    status: Value,
    last_error: Option<String>,
}

impl Control {
    fn start(mut engine: Engine) -> Result<Self> {
        let status = engine.status();
        let epoch = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        );
        let (sender, commands) = mpsc::sync_channel(QUEUE);
        let (updates, receiver) = mpsc::sync_channel(RESULTS);
        let observing = Arc::new(AtomicBool::new(false));
        let pending_observation = Arc::clone(&observing);
        thread::Builder::new()
            .name("nav-jumplist".into())
            .spawn(move || {
                loop {
                    let (completed, result) = match commands.recv_timeout(Duration::from_secs(1)) {
                        Ok(Work::Navigate {
                            ticket,
                            operation,
                            deadline,
                        }) => {
                            let result = if Instant::now() >= deadline {
                                Err(anyhow::anyhow!(
                                    "navigation request expired before execution"
                                ))
                            } else {
                                match operation {
                                    Operation::Travel { direction } => engine.travel(direction),
                                    Operation::PreviousWorkspace => engine.previous_workspace(),
                                }
                            };
                            (Some(ticket), result)
                        }
                        Ok(Work::Observe) => {
                            pending_observation.store(false, Ordering::Release);
                            (None, engine.refresh().map(|_| ()))
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            (None, engine.refresh().map(|_| ()))
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let result = result.map_err(|error| format!("{error:#}"));
                    if let Err(error) = &result {
                        eprintln!("nav jumplist: {error}");
                    }
                    let update = Update {
                        error: result.as_ref().err().cloned(),
                        completed: completed.map(|ticket| (ticket, result)),
                        status: engine.status(),
                    };
                    if updates.send(update).is_err() {
                        break;
                    }
                }
            })
            .context("starting navigation worker")?;
        Ok(Self {
            sender,
            receiver,
            observing,
            results: VecDeque::new(),
            epoch,
            next: 0,
            status,
            last_error: None,
        })
    }

    fn drain(&mut self) {
        while let Ok(update) = self.receiver.try_recv() {
            if let Some((ticket, result)) = update.completed
                && let Some((_, slot)) = self.results.iter_mut().find(|(id, _)| *id == ticket)
            {
                *slot = Some(result);
            }
            self.status = update.status;
            self.last_error = update.error;
        }
    }

    fn handle(&mut self, request: Request) -> Result<Reply> {
        self.drain();
        match request {
            Request::Observe => {
                if !self.observing.swap(true, Ordering::AcqRel) {
                    match self.sender.try_send(Work::Observe) {
                        Ok(()) => {}
                        Err(mpsc::TrySendError::Full(_)) => {
                            // Queued traversals reconcile independently; the idle
                            // read also repairs any coalesced/missed wake.
                            self.observing.store(false, Ordering::Release);
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => {
                            bail!("navigation worker stopped")
                        }
                    }
                }
                Ok(Reply::Observed)
            }
            Request::Navigate { operation } => {
                ensure!(
                    self.results
                        .iter()
                        .filter(|(_, result)| result.is_none())
                        .count()
                        < QUEUE,
                    "navigation queue is full"
                );
                if self.results.len() >= RESULTS
                    && let Some(index) =
                        self.results.iter().position(|(_, result)| result.is_some())
                {
                    self.results.remove(index);
                }
                self.next = self
                    .next
                    .checked_add(1)
                    .context("navigation ticket counter exhausted")?;
                let ticket = format!("{}-{}", self.epoch, self.next);
                self.sender
                    .try_send(Work::Navigate {
                        ticket: ticket.clone(),
                        operation,
                        deadline: Instant::now() + START_DEADLINE,
                    })
                    .context("submitting navigation request")?;
                self.results.push_back((ticket.clone(), None));
                Ok(Reply::Submitted { ticket })
            }
            Request::Result { ticket } => {
                let (_, result) = self.results.iter().find(|(id, _)| *id == ticket).context(
                    "navigation result expired or worker restarted; do not replay the request",
                )?;
                Ok(match result {
                    None => Reply::Pending,
                    Some(result) => Reply::Complete {
                        result: result.clone(),
                    },
                })
            }
        }
    }
}

impl Handler for Control {
    fn request(&mut self, payload: Value) -> std::result::Result<Value, String> {
        serde_json::from_value(payload)
            .context("invalid navigation control request")
            .and_then(|request| self.handle(request))
            .and_then(|reply| serde_json::to_value(reply).context("encoding navigation reply"))
            .map_err(|error| format!("{error:#}"))
    }

    fn tick(&mut self) -> std::result::Result<(), String> {
        self.drain();
        Ok(())
    }

    fn status(&self) -> Value {
        json!({"navigation": self.status, "last_error": self.last_error,
            "queued": self.results.iter().filter(|(_, result)| result.is_none()).count()})
    }
}

/// Dispatch only Herdr-supplied invocation identities.
pub fn run(invocation: &Invocation) -> Result<()> {
    let operation = match invocation.kind() {
        InvocationKind::Action { id } if id == "back" => Some(Operation::Travel {
            direction: Travel::Back,
        }),
        InvocationKind::Action { id } if id == "forward" => Some(Operation::Travel {
            direction: Travel::Forward,
        }),
        InvocationKind::Action { id } if id == "previous-workspace" => {
            Some(Operation::PreviousWorkspace)
        }
        InvocationKind::Action { id } if id == "status" => None,
        InvocationKind::Action { id } => {
            return directional(
                invocation,
                id.parse()
                    .with_context(|| format!("invalid navigation action {id:?}"))?,
            );
        }
        InvocationKind::Startup => None,
        InvocationKind::Event { name, .. } if HOOKS.contains(&name.as_str()) => None,
        other => bail!("unsupported nav invocation {other:?}"),
    };
    let service = Service::new(invocation, "jumplist")?;
    if service.is_worker() {
        let engine = Engine::open(
            invocation.client().clone(),
            &service.state_dir().join("history"),
        )?;
        return service
            .run(Control::start(engine)?)
            .context("running jumplist service");
    }
    if let Some(operation) = operation {
        return execute(&service, operation);
    }
    if matches!(invocation.kind(), InvocationKind::Action { .. }) {
        service.ensure()?;
        println!("{}", serde_json::to_string_pretty(&service.status()?)?);
    } else {
        service.wake(serde_json::to_value(Request::Observe)?)?;
    }
    Ok(())
}

fn execute(service: &Service, operation: Operation) -> Result<()> {
    let reply = serde_json::from_value(
        service.wake(serde_json::to_value(Request::Navigate { operation })?)?,
    )?;
    let Reply::Submitted { ticket } = reply else {
        bail!("navigation service did not return a submission ticket");
    };
    let deadline = Instant::now() + ACTION_DEADLINE;
    loop {
        let reply =
            serde_json::from_value(service.wake(serde_json::to_value(Request::Result {
                ticket: ticket.clone(),
            })?)?)?;
        match reply {
            Reply::Complete { result } => return result.map_err(anyhow::Error::msg),
            Reply::Pending => {}
            _ => bail!("navigation service returned an invalid completion reply"),
        }
        ensure!(
            Instant::now() < deadline,
            "navigation completion timed out; request was not retried"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn directional(invocation: &Invocation, direction: Direction) -> Result<()> {
    let client = invocation.client();
    let pane = invocation.require_pane_id()?;
    let info = match client.pane_process_info(pane) {
        Ok(info) => Some(info),
        Err(error) => {
            eprintln!("nav: could not read what is running in {pane}: {error:#}");
            None
        }
    };
    match decide(direction, info.as_ref()) {
        NavigationDecision::Forward => client
            .pane_send_keys(pane, &[chord(direction)])
            .with_context(|| format!("sending {} to {pane}", chord(direction))),
        NavigationDecision::MoveFocus => client
            .pane_focus_direction(pane, direction)
            .with_context(|| format!("moving focus {} from {pane}", chord(direction))),
    }
}

#[cfg(test)]
mod tests;
