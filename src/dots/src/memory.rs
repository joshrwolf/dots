//! Host scope discovery and CLI presentation; storage mechanics belong to dots-memory.
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};
use dots_memory::Store;
use serde_json::{Value, json};

use crate::command::{MemoryCommand, MemoryScope};

pub(crate) fn run(command: MemoryCommand, directory: &Path, json_output: bool) -> Result<String> {
    // Explicit global operations work even if the current checkout has broken Git metadata.
    let needs_project = !matches!(
        &command,
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
    );
    let scopes = Scopes::resolve(directory, needs_project)?;
    let value = match command {
        MemoryCommand::Scope => json!({"global": scopes.global, "project": scopes.project}),
        MemoryCommand::Read {
            path,
            scope,
            start,
            lines,
        } => {
            json!({"scope": label(scope), "document": scopes.store(scope)?.read(&path, start, usize::from(lines))?})
        }
        MemoryCommand::Write {
            path,
            scope,
            expected,
            content_file,
        } => {
            let input = if content_file.is_absolute() {
                content_file
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
                .take((dots_memory::MAX_FILE_BYTES + 1) as u64)
                .read_to_string(&mut content)
                .context("read UTF-8 memory input")?;
            ensure!(
                content.len() <= dots_memory::MAX_FILE_BYTES,
                "memory input exceeds 256 KiB"
            );
            json!({"scope": label(scope), "published": scopes.store(scope)?.publish(&path, &expected, Some(&content))?})
        }
        MemoryCommand::Remove {
            path,
            scope,
            expected,
        } => {
            json!({"scope": label(scope), "published": scopes.store(scope)?.publish(&path, &expected, None)?})
        }
        MemoryCommand::Search {
            query,
            scope,
            include_sources,
            offset,
            limit,
        } => {
            let selected = scope.map_or_else(
                || {
                    let mut selected = vec![MemoryScope::Global];
                    if scopes.project.is_some() {
                        selected.push(MemoryScope::Project);
                    }
                    selected
                },
                |scope| vec![scope],
            );
            let results = selected.into_iter().map(|scope| -> Result<Value> {
                Ok(json!({"scope": label(scope), "result": scopes.store(scope)?.search(&query, include_sources, offset as usize, usize::from(limit))?}))
            }).collect::<Result<Vec<_>>>()?;
            json!({"searches": results})
        }
    };
    Ok(if json_output {
        crate::json::render(value)
    } else {
        render(&value)
    })
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
}
impl Scopes {
    fn resolve(directory: &Path, project: bool) -> Result<Self> {
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .context("HOME or absolute XDG_DATA_HOME is required")?
            .join("dots/memory");
        ensure!(root.is_absolute(), "memory root must be absolute");
        let checkout = if project { checkout(directory)? } else { None };
        let project = checkout
            .map(|checkout| -> Result<PathBuf> {
                let key = dots_memory::project_key(&checkout);
                ensure!(
                    !key.is_empty() && key != "_global",
                    "project memory key collides with the root or global scope"
                );
                Ok(root.join(key))
            })
            .transpose()?;
        Ok(Self {
            global: root.join("_global"),
            project,
        })
    }
    fn store(&self, scope: MemoryScope) -> Result<Store> {
        let path = match scope {
            MemoryScope::Global => &self.global,
            MemoryScope::Project => self
                .project
                .as_ref()
                .context("outside Git: project memory is unavailable; select --scope global")?,
        };
        Ok(Store::new(path.clone())?)
    }
}

fn checkout(directory: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .env("LC_ALL", "C")
        .current_dir(directory)
        .output()
        .context("resolve memory checkout with git")?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        if message
            .starts_with("fatal: not a git repository (or any of the parent directories): .git")
        {
            return Ok(None);
        }
        bail!("cannot resolve memory checkout: {message}");
    }
    let common = String::from_utf8(output.stdout).context("Git checkout path is not UTF-8")?;
    let checkout = Path::new(common.trim_end())
        .parent()
        .context("Git common directory has no parent")?
        .canonicalize()
        .context("canonicalize memory checkout")?;
    Ok(Some(
        checkout
            .to_str()
            .context("checkout path is not Unicode")?
            .to_owned(),
    ))
}

fn render(value: &Value) -> String {
    let mut out = String::new();
    if let Some(document) = value.get("document") {
        let _ = writeln!(
            out,
            "{} · {}",
            document["path"].as_str().unwrap_or_default(),
            document["revision"].as_str().unwrap_or_default()
        );
        out.push_str(document["content"].as_str().unwrap_or_default());
        if let Some(next) = document["next_line"].as_u64() {
            let _ = writeln!(out, "More: repeat read with --start {next}");
        }
    } else if let Some(searches) = value["searches"].as_array() {
        for search in searches {
            let _ = writeln!(
                out,
                "{} · literal",
                search["scope"].as_str().unwrap_or_default()
            );
            if let Some(hits) = search.pointer("/result/hits").and_then(Value::as_array) {
                if hits.is_empty() {
                    out.push_str("No matches.\n");
                }
                for hit in hits {
                    let _ = writeln!(
                        out,
                        "{}:{}: {}{}",
                        hit["path"].as_str().unwrap_or_default(),
                        hit["line"],
                        hit["excerpt"].as_str().unwrap_or_default(),
                        if hit["truncated"] == true {
                            " [truncated]"
                        } else {
                            ""
                        }
                    );
                }
            }
            if let Some(next) = search
                .pointer("/result/next_offset")
                .and_then(Value::as_u64)
            {
                let _ = writeln!(
                    out,
                    "More: repeat search for this scope with --offset {next}"
                );
            }
        }
    } else {
        out = crate::json::render(value.clone());
    }
    out
}
