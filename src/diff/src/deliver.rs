//! Delivering a review to the Pi session it is for. Each interactive Pi
//! opens an inbox socket and registers itself in a per-user directory; the
//! viewer chooses one by a fixed order and never guesses between several.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde::Deserialize;

use crate::agents::{Agent, Worker, worktree_of};

/// Who a review goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "built once per resolution and moved, never stored in bulk"
)]
pub(crate) enum Resolution {
    Agent {
        agent: Agent,
        /// The worker the review is about, whose parent forwards it.
        worker: Option<Worker>,
        why: Why,
    },
    /// Several sessions fit; the user picks.
    Ambiguous(Vec<Agent>),
    None,
    /// The workers registry, needed to find the session, could not be read.
    Unavailable(String),
    /// The session the user picked has ended; they pick again.
    PickGone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// The user picked it for this worktree.
    Picked,
    /// The pane the viewer was opened beside.
    Bound,
    /// The only interactive Pi working in the worktree.
    Working,
    /// The parent of the worker that worked in the worktree.
    Parent,
}

/// What resolution knows besides the registries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Ask<'a> {
    pub(crate) worktree: &'a Path,
    /// Every worktree of the repository, to tell which one a directory is in.
    pub(crate) worktrees: &'a [PathBuf],
    /// The pane the viewer was opened beside, while it shows the worktree it
    /// was opened on.
    pub(crate) bound: Option<&'a str>,
    /// The session id picked for this worktree.
    pub(crate) picked: Option<&'a str>,
}

/// Chooses the session for a worktree: the one the user picked for it, the
/// bound pane's Pi, the one interactive Pi working there, then the parent of
/// the workers that worked there. Several at one step is ambiguous. A pick
/// holds until the user changes it; once its session ends, nothing else is
/// chosen in its place.
pub(crate) fn resolve(
    agents: &[Agent],
    workers: std::result::Result<&[Worker], &str>,
    ask: Ask<'_>,
) -> Resolution {
    let found = |agent: &Agent, why| Resolution::Agent {
        agent: agent.clone(),
        worker: None,
        why,
    };
    if let Some(picked) = ask.picked {
        return agents
            .iter()
            .find(|a| a.session_id == picked)
            .map_or(Resolution::PickGone, |a| found(a, Why::Picked));
    }
    if let Some(a) = ask
        .bound
        .and_then(|b| agents.iter().find(|a| a.rex_block.as_deref() == Some(b)))
    {
        return found(a, Why::Bound);
    }
    let here = |dir: &Path| worktree_of(dir, ask.worktrees) == Some(ask.worktree);
    let working: Vec<&Agent> = agents.iter().filter(|a| here(&a.cwd)).collect();
    match working.as_slice() {
        [one] => return found(one, Why::Working),
        [] => {}
        many => return Resolution::Ambiguous(many.iter().map(|a| (*a).clone()).collect()),
    }
    let workers = match workers {
        Ok(w) => w,
        Err(e) => return Resolution::Unavailable(e.to_owned()),
    };
    // A worker's owner may be another worker; follow owners up to a session
    // with an inbox.
    let parent = |w: &Worker| {
        let mut owner = w.owner_session.clone();
        for _ in 0..16 {
            let file = owner?;
            if let Some(a) = agents
                .iter()
                .find(|a| a.session_file.as_deref() == Some(&file))
            {
                return Some(a);
            }
            owner = workers
                .iter()
                .find(|x| x.session_file.as_deref() == Some(&file))
                .and_then(|x| x.owner_session.clone());
        }
        None
    };
    let mut owned: Vec<(&Worker, &Agent)> = workers
        .iter()
        .filter(|w| here(&w.cwd))
        .filter_map(|w| parent(w).map(|a| (w, a)))
        .collect();
    owned.sort_by_key(|(w, _)| std::cmp::Reverse(w.updated_at));
    let mut parents: Vec<&Agent> = owned.iter().map(|(_, a)| *a).collect();
    parents.dedup_by(|a, b| a.session_id == b.session_id);
    parents.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    parents.dedup_by(|a, b| a.session_id == b.session_id);
    match (parents.as_slice(), owned.first()) {
        ([one], Some((worker, _))) => Resolution::Agent {
            agent: (*one).clone(),
            worker: Some((*worker).clone()),
            why: Why::Parent,
        },
        ([], _) | (_, None) => Resolution::None,
        (many, _) => Resolution::Ambiguous(many.iter().map(|a| (*a).clone()).collect()),
    }
}

/// How the session took the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Ack {
    /// It started a turn with it.
    Started,
    /// It is busy; the message follows the current run.
    Queued,
}

#[derive(Debug, Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    state: Option<Ack>,
    #[serde(default)]
    error: Option<String>,
}

/// Sends `text` to `agent` as a user message and waits for its ack.
pub(crate) fn send(agent: &Agent, text: &str) -> Result<Ack> {
    let mut stream = UnixStream::connect(&agent.socket)
        .with_context(|| format!("{} is not listening", agent.label()))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let request = serde_json::json!({"type": "deliver", "text": text});
    stream.write_all(format!("{request}\n").as_bytes())?;
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .context("waiting for the session's answer")?;
    let reply: Reply = serde_json::from_str(&line)
        .with_context(|| format!("unexpected answer {:?}", line.trim()))?;
    match (reply.ok, reply.state) {
        (true, Some(state)) => Ok(state),
        _ => bail!(
            "{}",
            reply
                .error
                .unwrap_or_else(|| "the session refused the message".to_owned())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, cwd: &str, block: Option<&str>) -> Agent {
        Agent {
            pid: 1,
            session_id: id.into(),
            session_file: Some(format!("/s/{id}.jsonl")),
            session_name: None,
            rex_block: block.map(Into::into),
            cwd: cwd.into(),
            socket: format!("/i/{id}.sock").into(),
        }
    }

    fn worker(id: &str, cwd: &str, owner: &str, at: i64) -> Worker {
        Worker {
            id: id.into(),
            label: format!("w-{id}"),
            cwd: cwd.into(),
            status: crate::agents::Status::Done,
            model: None,
            owner_session: Some(owner.into()),
            session_file: Some(format!("/s/{id}.jsonl")),
            updated_at: at,
        }
    }

    fn chosen(r: &Resolution) -> Option<(&str, Option<&str>, Why)> {
        match r {
            Resolution::Agent { agent, worker, why } => Some((
                agent.session_id.as_str(),
                worker.as_ref().map(|w| w.id.as_str()),
                *why,
            )),
            _ => None,
        }
    }

    #[test]
    fn resolves_in_order_picked_bound_working_then_parent() {
        let worktrees: Vec<PathBuf> = vec!["/r".into(), "/r/.wt/feat".into(), "/w/other".into()];
        let ask = |bound, picked| Ask {
            worktree: Path::new("/r/.wt/feat"),
            worktrees: &worktrees,
            bound,
            picked,
        };
        let agents = vec![
            agent("main", "/r/src", Some("block:a")),
            agent("feat", "/r/.wt/feat/src", Some("block:b")),
            agent("parent", "/home", None),
        ];
        let workers = vec![worker("w1", "/r/.wt/feat", "/s/parent.jsonl", 5)];
        let resolve = |bound, picked| resolve(&agents, Ok(&workers), ask(bound, picked));
        // An explicit pick wins over the pane the viewer was opened from.
        assert_eq!(
            chosen(&resolve(Some("block:a"), Some("parent"))),
            Some(("parent", None, Why::Picked))
        );
        assert_eq!(
            chosen(&resolve(Some("block:a"), None)),
            Some(("main", None, Why::Bound))
        );
        // A picked session that ended is never replaced by a guess.
        assert_eq!(
            resolve(Some("block:a"), Some("ended")),
            Resolution::PickGone
        );
        // The main worktree's Pi is not in the nested worktree.
        assert_eq!(
            chosen(&resolve(None, None)),
            Some(("feat", None, Why::Working))
        );
        let idle: Vec<Agent> = agents
            .iter()
            .filter(|a| a.session_id != "feat")
            .cloned()
            .collect();
        assert_eq!(
            chosen(&super::resolve(&idle, Ok(&workers), ask(None, None))),
            Some(("parent", Some("w1"), Why::Parent))
        );
    }

    #[test]
    fn never_guesses_between_sessions() {
        let worktrees: Vec<PathBuf> = vec!["/r".into()];
        let ask = Ask {
            worktree: Path::new("/r"),
            worktrees: &worktrees,
            bound: None,
            picked: None,
        };
        let two = vec![agent("a", "/r", None), agent("b", "/r/x", None)];
        assert!(matches!(resolve(&two, Ok(&[]), ask), Resolution::Ambiguous(v) if v.len() == 2));
        let parents = vec![agent("p", "/h", None), agent("q", "/h", None)];
        let workers = vec![
            worker("w1", "/r", "/s/p.jsonl", 1),
            worker("w2", "/r", "/s/q.jsonl", 2),
        ];
        assert!(matches!(
            resolve(&parents, Ok(&workers), ask),
            Resolution::Ambiguous(_)
        ));
        assert_eq!(resolve(&[], Ok(&workers), ask), Resolution::None);
        assert_eq!(
            resolve(&[agent("x", "/elsewhere", None)], Ok(&[]), ask),
            Resolution::None
        );
    }

    #[test]
    fn nested_workers_reach_the_session_that_owns_their_parent() {
        let worktrees: Vec<PathBuf> = vec!["/r".into()];
        let ask = Ask {
            worktree: Path::new("/r"),
            worktrees: &worktrees,
            bound: None,
            picked: None,
        };
        let agents = vec![agent("root", "/h", None)];
        let workers = vec![
            worker("lead", "/h", "/s/root.jsonl", 1),
            worker("child", "/r", "/s/lead.jsonl", 3),
            worker("older", "/r", "/s/root.jsonl", 2),
        ];
        assert_eq!(
            chosen(&resolve(&agents, Ok(&workers), ask)),
            Some(("root", Some("child"), Why::Parent))
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dots-diff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dots")).expect("dir");
        dir
    }

    #[test]
    fn an_unreadable_registry_is_reported_where_resolution_needs_it() {
        let dir = scratch("registry-bad");
        let conn = rusqlite::Connection::open(dir.join("dots/workers.db")).expect("open");
        conn.execute_batch("create table workers (id text)")
            .expect("schema");
        drop(conn);
        let workers = crate::agents::workers_in(&dir, &|_| true).map_err(|e| format!("{e:#}"));
        assert!(workers.is_err(), "a changed table is an error");
        let worktrees: Vec<PathBuf> = vec!["/r".into()];
        let ask = Ask {
            worktree: Path::new("/r"),
            worktrees: &worktrees,
            bound: None,
            picked: None,
        };
        let agents = vec![agent("p", "/h", None)];
        assert!(matches!(
            resolve(&agents, workers.as_deref().map_err(String::as_str), ask),
            Resolution::Unavailable(_)
        ));
        // Steps before the workers' do not need them.
        let here = vec![agent("a", "/r", None)];
        assert_eq!(
            chosen(&resolve(
                &here,
                workers.as_deref().map_err(String::as_str),
                ask
            )),
            Some(("a", None, Why::Working))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delivers_and_reads_the_ack() {
        let dir = std::env::temp_dir().join(format!("dots-diff-inbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let socket = dir.join("p.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
        let server = std::thread::spawn(move || {
            for reply in [
                r#"{"ok":true,"state":"queued"}"#,
                r#"{"ok":false,"error":"busy"}"#,
            ] {
                let (stream, _) = listener.accept().expect("accept");
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).expect("read");
                let request: serde_json::Value = serde_json::from_str(&line).expect("json");
                assert_eq!(request["text"], "hello\nworld");
                (&stream)
                    .write_all(format!("{reply}\n").as_bytes())
                    .expect("write");
            }
        });
        let mut a = agent("p", "/r", None);
        a.socket = socket;
        assert_eq!(send(&a, "hello\nworld").expect("ack"), Ack::Queued);
        assert_eq!(
            send(&a, "hello\nworld").expect_err("refused").to_string(),
            "busy"
        );
        server.join().expect("server");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
