//! Host scope discovery and CLI presentation; filesystem mechanics belong to dots-documents.
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, ensure};
use dots_documents::{Document, Search, Store};
use serde_json::json;

use crate::command::{MemoryCommand, MemoryScope};

pub(crate) fn run(
    command: MemoryCommand,
    directory: &Path,
    project: Option<&str>,
    json_output: bool,
) -> Result<String> {
    let needs_project = needs_project(&command);
    ensure!(
        project.is_none() || needs_project,
        "--project cannot be combined with --scope global"
    );
    let scopes = Scopes::resolve(directory, needs_project, project)?;
    if matches!(
        &command,
        MemoryCommand::Write { .. } | MemoryCommand::Remove { .. }
    ) {
        crate::project::open()?;
    }
    match command {
        MemoryCommand::Scope => Ok(crate::json::render(
            json!({"global": scopes.global, "project": scopes.project, "diagnostic": scopes.diagnostic}),
        )),
        MemoryCommand::Read {
            path,
            scope,
            start,
            lines,
        } => {
            let document = scopes
                .store(scope)?
                .read(&path, start, usize::from(lines))?;
            Ok(if json_output {
                crate::json::render(json!({"scope": label(scope), "document": document}))
            } else {
                render_document(&document)
            })
        }
        MemoryCommand::Write {
            path,
            scope,
            expected,
            content_file,
        } => {
            let content = read_input(directory, &content_file)?;
            let published = scopes
                .store(scope)?
                .publish(&path, &expected, Some(&content))?;
            Ok(crate::json::render(
                json!({"scope": label(scope), "published": published}),
            ))
        }
        MemoryCommand::Remove {
            path,
            scope,
            expected,
        } => {
            let published = scopes.store(scope)?.publish(&path, &expected, None)?;
            Ok(crate::json::render(
                json!({"scope": label(scope), "published": published}),
            ))
        }
        MemoryCommand::Search {
            query,
            scope,
            offset,
            limit,
        } => {
            let selected = selected_scopes(&scopes, scope);
            let results = selected
                .into_iter()
                .map(|scope| -> Result<(&'static str, Search)> {
                    Ok((
                        label(scope),
                        scopes
                            .store(scope)?
                            .search(&query, offset as usize, usize::from(limit))?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(if json_output {
                let searches = results
                    .iter()
                    .map(|(scope, result)| json!({"scope": scope, "result": result}))
                    .collect::<Vec<_>>();
                crate::json::render(json!({"searches": searches, "diagnostic": scopes.diagnostic}))
            } else {
                let rendered = render_searches(&results);
                scopes
                    .diagnostic
                    .as_ref()
                    .map_or(rendered.clone(), |diagnostic| {
                        format!("{diagnostic}\n{rendered}")
                    })
            })
        }
    }
}

fn needs_project(command: &MemoryCommand) -> bool {
    // Explicit global operations work even if the current checkout has broken Git metadata.
    !matches!(
        command,
        MemoryCommand::Read {
            scope: MemoryScope::Global,
            ..
        } | MemoryCommand::Write {
            scope: MemoryScope::Global,
            ..
        } | MemoryCommand::Remove {
            scope: MemoryScope::Global,
            ..
        } | MemoryCommand::Search {
            scope: Some(MemoryScope::Global),
            ..
        }
    )
}

fn read_input(directory: &Path, content_file: &Path) -> Result<String> {
    let input = if content_file.is_absolute() {
        content_file.to_path_buf()
    } else {
        directory.join(content_file)
    };
    ensure!(
        std::fs::metadata(&input)
            .context("inspect memory input")?
            .is_file(),
        "memory input must be a regular file"
    );
    let mut content = String::new();
    std::fs::File::open(&input)
        .context("open memory input file")?
        .take((dots_documents::MAX_FILE_BYTES + 1) as u64)
        .read_to_string(&mut content)
        .context("read UTF-8 memory input")?;
    ensure!(
        content.len() <= dots_documents::MAX_FILE_BYTES,
        "memory input exceeds 256 KiB"
    );
    Ok(content)
}

fn selected_scopes(scopes: &Scopes, scope: Option<MemoryScope>) -> Vec<MemoryScope> {
    scope.map_or_else(
        || {
            let mut selected = vec![MemoryScope::Global];
            if scopes.project.is_some() {
                selected.push(MemoryScope::Project);
            }
            selected
        },
        |scope| vec![scope],
    )
}

fn label(scope: MemoryScope) -> &'static str {
    match scope {
        MemoryScope::Global => "global",
        MemoryScope::Project => "project",
    }
}

struct Scopes {
    global: PathBuf,
    project: Option<PathBuf>,
    lock_path: PathBuf,
    diagnostic: Option<String>,
}

impl Scopes {
    fn resolve(directory: &Path, needs_project: bool, explicit: Option<&str>) -> Result<Self> {
        let root = crate::project::data_root()?;
        let workspaces = root.join("workspaces");
        let mut project = None;
        let mut diagnostic = None;
        if needs_project && crate::project::catalog_path()?.try_exists()? {
            let mut store = crate::project::open()?;
            match crate::project::resolve_optional(&mut store, directory, explicit)? {
                crate::project::Resolution::Bound(id) => {
                    project = Some(workspaces.join(id.as_str()).join("knowledge"));
                }
                crate::project::Resolution::Unavailable(message) => {
                    diagnostic = Some(message.into());
                }
            }
        } else if needs_project {
            ensure!(
                explicit.is_none(),
                "workspace is absent; use dots project init"
            );
            diagnostic =
                Some("Workspace is absent; use dots project init for project memory.".into());
        }
        Ok(Self {
            global: workspaces.join("global/knowledge"),
            project,
            lock_path: root.join("mutation.lock"),
            diagnostic,
        })
    }

    fn store(&self, scope: MemoryScope) -> Result<Store> {
        let path = match scope {
            MemoryScope::Global => &self.global,
            MemoryScope::Project => self.project.as_ref().with_context(|| {
                self.diagnostic
                    .as_deref()
                    .unwrap_or("project memory is unavailable")
                    .to_owned()
            })?,
        };
        Ok(Store::try_new(path.clone(), self.lock_path.clone())?)
    }
}

fn render_document(document: &Document) -> String {
    let mut out = format!("{} · {}\n", document.path.display(), document.revision);
    out.push_str(&document.content);
    if let Some(next) = document.next_line {
        let _ = writeln!(out, "More: repeat read with --start {next}");
    }
    out
}

fn render_searches(searches: &[(&str, Search)]) -> String {
    let mut out = String::new();
    for (scope, result) in searches {
        let _ = writeln!(out, "{scope} · literal");
        if result.hits.is_empty() {
            out.push_str("No matches.\n");
        }
        for hit in &result.hits {
            let _ = writeln!(
                out,
                "{}:{}: {}{}",
                hit.path.display(),
                hit.line,
                hit.excerpt,
                if hit.truncated { " [truncated]" } else { "" }
            );
        }
        if let Some(next) = result.next_offset {
            let _ = writeln!(
                out,
                "More: repeat search for this scope with --offset {next}"
            );
        }
    }
    out
}
