//! Which agent wrote each of the shown worktree's files, and showing only
//! one agent's files. Experimental: attribution reads the agents' session
//! files and leaves anything they do not plainly name unknown.

use std::path::PathBuf;

use super::App;
use crate::attribution::{Attribution, Source};
use crate::repo::FileChange;

/// Whose files the view shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AuthorFilter {
    Author(String),
    /// Files no agent is known to have written.
    Unknown,
}

impl AuthorFilter {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Author(name) => name,
            Self::Unknown => "unknown",
        }
    }
}

impl App {
    /// The session files of the agents working in the shown worktree and
    /// of the sessions that own those workers.
    pub(crate) fn attribution_sources(&self) -> Vec<Source> {
        let (Some(owners), Some(snapshot)) = (self.owners(self.current), &self.agents) else {
            return Vec::new();
        };
        let mut sources: Vec<Source> = Vec::new();
        let mut add = |name: String, file: Option<&String>| {
            if let Some(file) = file
                && !sources.iter().any(|s| s.file.as_os_str() == file.as_str())
            {
                sources.push(Source {
                    name,
                    file: PathBuf::from(file),
                });
            }
        };
        for w in &owners.workers {
            add(w.label.clone(), w.session_file.as_ref());
        }
        for a in &owners.sessions {
            add(format!("pi {}", a.label()), a.session_file.as_ref());
        }
        for w in &owners.workers {
            let Some(owner) = &w.owner_session else {
                continue;
            };
            let name = if let Some(a) = snapshot
                .agents
                .iter()
                .find(|a| a.session_file.as_ref() == Some(owner))
            {
                format!("pi {}", a.label())
            } else if let Some(p) = snapshot
                .workers
                .iter()
                .find(|p| p.session_file.as_ref() == Some(owner))
            {
                p.label.clone()
            } else {
                format!("parent of {}", w.label)
            };
            add(name, Some(owner));
        }
        sources
    }

    /// Takes the shown worktree's attribution, showing the files the
    /// author filter now leaves in.
    pub(crate) fn set_attribution(&mut self, attribution: Option<Attribution>) {
        if attribution == self.attribution {
            return;
        }
        self.attribution = attribution;
        self.apply_author_filter(false);
    }

    /// The agents that wrote `path` in the shown worktree: empty when
    /// none is known to, `None` for any path when no agent's sessions were
    /// read.
    pub(crate) fn authors_of(&self, path: &str) -> Option<Vec<&str>> {
        Some(self.attributed()?.authors_of(path).collect())
    }

    /// The shown worktree's attribution, once some agent's sessions were
    /// read for it.
    fn attributed(&self) -> Option<&Attribution> {
        self.attribution
            .as_ref()
            .filter(|a| self.session.shows(&a.worktree) && !a.authors.is_empty())
    }

    /// Whether files can be attributed in the shown worktree.
    pub(crate) fn can_attribute(&self) -> bool {
        self.attributed().is_some()
    }

    /// What a file header adds after the path: a mode change, and who
    /// wrote the file, as `by fix, pi lead`.
    pub(crate) fn header_note(&self, change: &FileChange) -> Option<String> {
        let authors = self.authors_of(&change.path).map(|a| {
            if a.is_empty() {
                "by unknown".to_owned()
            } else {
                format!("by {}", a.join(", "))
            }
        });
        let parts: Vec<String> = change.mode_note().into_iter().chain(authors).collect();
        (!parts.is_empty()).then(|| parts.join("  "))
    }

    /// Shows the next author's files, then the files of no known author,
    /// then every file again. Returns the filter now in effect.
    pub(crate) fn next_author_filter(&mut self) -> Option<&AuthorFilter> {
        let mut choices: Vec<AuthorFilter> = Vec::new();
        if let Some(a) = &self.attribution {
            for (i, name) in a.authors.iter().enumerate() {
                let wrote = a.files.values().any(|v| v.contains(&i));
                let choice = AuthorFilter::Author(name.clone());
                if wrote && !choices.contains(&choice) {
                    choices.push(choice);
                }
            }
            choices.push(AuthorFilter::Unknown);
        }
        let next = match &self.author_filter {
            None => choices.first(),
            Some(f) => choices
                .iter()
                .position(|c| c == f)
                .and_then(|i| choices.get(i + 1)),
        };
        self.author_filter = next.cloned();
        self.apply_author_filter(false);
        self.author_filter.as_ref()
    }

    /// Hides the files the author filter leaves out and shows the rest,
    /// keeping the view on the file at its top or the nearest one shown.
    /// `rebuilt`: the stream's files were replaced, so the sidebar's tree
    /// is built again even when no file's visibility changed.
    pub(super) fn apply_author_filter(&mut self, rebuilt: bool) {
        let hidden: Vec<bool> = self.stream.as_ref().map_or_else(Vec::new, |s| {
            s.files.iter().map(|f| self.hides(&f.change)).collect()
        });
        let Some(stream) = &mut self.stream else {
            return;
        };
        let mut changed = false;
        for (f, hide) in stream.files.iter_mut().zip(hidden) {
            changed |= f.hidden != hide;
            f.hidden = hide;
        }
        if changed {
            stream.relayout();
            if stream.files.get(self.top.file).is_some_and(|f| f.hidden)
                && let Some(file) = stream.shown_near(self.top.file)
            {
                self.top = super::Anchor { file, line: 0 };
                self.pending_anchor = None;
            }
            if let super::Mode::Visual(v) = &self.mode
                && stream.files.get(v.file).is_none_or(|f| f.hidden)
            {
                self.mode = super::Mode::default();
            }
        }
        if changed || rebuilt {
            self.sidebar.set_files(&stream.files);
            self.sync_files_cursor();
        }
    }

    /// Whether the author filter leaves `change` out.
    fn hides(&self, change: &FileChange) -> bool {
        let Some(filter) = &self.author_filter else {
            return false;
        };
        let authors = self.authors_of(&change.path).unwrap_or_default();
        match filter {
            AuthorFilter::Author(name) => !authors.contains(&name.as_str()),
            AuthorFilter::Unknown => !authors.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::agents::{Agent, Snapshot, Status, Worker};
    use crate::app::fixture::app_with;
    use crate::app::sidebar::ListItem;

    fn worker(id: &str, cwd: &str, owner: &str) -> Worker {
        Worker {
            id: id.into(),
            label: id.into(),
            status: Status::Done,
            cwd: cwd.into(),
            model: None,
            owner_session: Some(owner.into()),
            session_file: Some(format!("/s/{id}.jsonl")),
            updated_at: 1,
        }
    }

    #[test]
    fn sources_are_the_worktrees_agents_and_their_parents() {
        let mut app = app_with(&[]);
        app.set_agents(Snapshot {
            agents: vec![Agent {
                pid: 1,
                session_id: "lead".into(),
                session_file: Some("/s/lead.jsonl".into()),
                session_name: Some("lead".into()),
                rex_block: None,
                cwd: "/home".into(),
                socket: "/i/lead.sock".into(),
            }],
            workers: vec![
                worker("fix", "/r", "/s/lead.jsonl"),
                worker("sub", "/r/src", "/s/fix.jsonl"),
                worker("orphan", "/r", "/s/ended.jsonl"),
                worker("other", "/elsewhere", "/s/lead.jsonl"),
            ],
            problems: Vec::new(),
            registry: None,
        });
        let names: Vec<(String, String)> = app
            .attribution_sources()
            .into_iter()
            .map(|s| (s.name, s.file.to_string_lossy().into_owned()))
            .collect();
        let pair = |n: &str, f: &str| (n.to_owned(), f.to_owned());
        assert_eq!(
            names,
            [
                pair("fix", "/s/fix.jsonl"),
                pair("sub", "/s/sub.jsonl"),
                pair("orphan", "/s/orphan.jsonl"),
                pair("pi lead", "/s/lead.jsonl"),
                pair("parent of orphan", "/s/ended.jsonl"),
            ]
        );
    }

    #[test]
    fn the_filter_cycles_through_authors_then_unknown() {
        let mut app = app_with(&[("a.rs", "x\n", "y\n"), ("b.rs", "x\n", "z\n")]);
        app.session
            .install(std::path::Path::new("/r"), Err("fixture".into()));
        let note = |app: &App, p: &str| app.header_note(&crate::app::fixture::change(p, "", ""));
        assert_eq!(note(&app, "a.rs"), None, "no sessions read");
        let attribution = Attribution {
            worktree: "/r".into(),
            authors: vec!["fix".into(), "idle".into(), "lead".into()],
            files: HashMap::from([("a.rs".into(), vec![0, 2])]),
            problems: Vec::new(),
        };
        app.set_attribution(Some(attribution.clone()));
        assert_eq!(note(&app, "a.rs").as_deref(), Some("by fix, lead"));
        assert_eq!(note(&app, "b.rs").as_deref(), Some("by unknown"));
        let shown = |app: &App| -> Vec<String> {
            let stream = app.stream.as_ref().expect("loaded");
            stream.shown().map(|(_, f)| f.change.path.clone()).collect()
        };
        let listed = |app: &App| -> Vec<usize> {
            app.sidebar
                .rows()
                .iter()
                .map(ListItem::first_file)
                .collect()
        };
        let mut seen = Vec::new();
        while let Some(f) = app.next_author_filter() {
            seen.push(f.label().to_owned());
            if seen.len() == 1 {
                assert_eq!(shown(&app), ["a.rs"]);
                assert_eq!(listed(&app), [0]);
            }
        }
        assert_eq!(seen, ["fix", "lead", "unknown"], "idle wrote nothing");
        assert_eq!(shown(&app), ["a.rs", "b.rs"], "all again");
        app.next_author_filter();
        app.next_author_filter();
        app.next_author_filter();
        assert_eq!(shown(&app), ["b.rs"]);
        assert_eq!(listed(&app), [1]);
        assert_eq!(
            app.stream.as_ref().map(|s| s.files.len()),
            Some(2),
            "every file is kept"
        );
        app.set_attribution(Some(Attribution {
            files: HashMap::from([("b.rs".into(), vec![0])]),
            ..attribution
        }));
        assert_eq!(shown(&app), ["a.rs"], "attribution changes only what shows");
    }
}
