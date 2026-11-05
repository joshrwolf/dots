//! CLI boundary for immutable retained outputs and their discovery.
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Result;
use dots_tasks::output_discovery::Association;
use dots_tasks::outputs::{OutputContent, PublishMode, PublishOutput};
use dots_tasks::{Actor, Owner};
use serde_json::json;
use time::OffsetDateTime;

use crate::command::OutputCommand;

pub(crate) fn run(
    command: OutputCommand,
    selected: Option<&str>,
    owner: Option<&str>,
    directory: &Path,
    json_output: bool,
) -> Result<String> {
    let mut store = crate::project::open()?;
    let project = crate::project::resolve(&mut store, directory, selected)?;
    let session = crate::agent::owner();
    let actor = Actor {
        name: owner
            .or(session.as_deref())
            .map(Owner::try_new)
            .transpose()?,
        now: OffsetDateTime::now_utc().unix_timestamp(),
    };
    let value = match command {
        OutputCommand::Publish {
            slug,
            filename,
            media_type,
            content_file,
            create,
            description,
            provenance,
            task,
        } => {
            let input = PublishOutput {
                slug: &slug,
                filename: &filename,
                media_type: &media_type,
                source: &content_file,
                mode: if create {
                    PublishMode::Create
                } else {
                    PublishMode::Extend
                },
                description: &description,
                provenance: &provenance,
                task: task.as_deref(),
            };
            json!({"publication":store.publish_output(&project,&input,actor)?})
        }
        OutputCommand::List { task, offset } => {
            serde_json::to_value(store.list_outputs(&project, task.as_deref(), offset)?)?
        }
        OutputCommand::Revisions { slug, offset } => {
            json!({"output":store.output_manifest(&project,&slug,offset)?})
        }
        OutputCommand::Show {
            slug,
            digest,
            start,
            lines,
        } => {
            let read = store.read_output(&project, &slug, &digest, start, usize::from(lines))?;
            if !json_output {
                let mut text = format!("{} {}\n{}\n", slug, digest, read.path.display());
                match &read.content {
                    OutputContent::Available { document } => {
                        text.push_str(&document.content);
                        if let Some(next) = document.next_line {
                            writeln!(text, "\nMore content: --start {next}")?;
                        }
                    }
                    other => writeln!(text, "{}", serde_json::to_string(other)?)?,
                }
                return Ok(text);
            }
            json!({"output":read})
        }
        OutputCommand::Path { slug, digest } => {
            let read = store.read_output(&project, &slug, &digest, 1, 1)?;
            let mut value = serde_json::to_value(read)?;
            if let Some(content) = value
                .get_mut("content")
                .and_then(serde_json::Value::as_object_mut)
            {
                content.remove("document");
            }
            json!({"output":value})
        }
        OutputCommand::Attach { slug, task } => {
            json!({"changed":store.associate_output(&project,&task,&slug,Association::Attach,actor)?})
        }
        OutputCommand::Detach { slug, task } => {
            json!({"changed":store.associate_output(&project,&task,&slug,Association::Detach,actor)?})
        }
        OutputCommand::Integrity => json!({"integrity":store.output_integrity(&project)?}),
        OutputCommand::Purge { digest } => {
            json!({"purge":store.purge_output_digest(&project,&digest,actor)?})
        }
    };
    let mut value = value;
    value
        .as_object_mut()
        .expect("output responses are objects")
        .insert("project".into(), serde_json::to_value(project)?);
    Ok(crate::json::render(value))
}
