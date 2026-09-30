//! Exact GitHub check-run output, including checks backed by external providers.
use crate::logview::{Navigation, Query, page};
use crate::provider::checked;
use crate::{Error, LogPage, LogSource, ObservationSource, Provider, Result, SnapshotRef};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    io::{Seek as _, Write as _},
    str::FromStr,
};
use unicode_segmentation::UnicodeSegmentation as _;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "String")]
pub struct OutputRef {
    repository: crate::Repository,
    pr: Option<u64>,
    head: String,
    id: u64,
}
impl OutputRef {
    pub(crate) fn create(url: &str, head: &str, id: u64) -> Option<Self> {
        let (repository, resource) = crate::target::Resource::parse(url).ok()?;
        let pr = if let crate::target::Resource::Pr(pr) = resource {
            Some(pr)
        } else {
            None
        };
        Self::new(repository, pr, head, id).ok()
    }
    pub(crate) fn new(
        repository: crate::Repository,
        pr: Option<u64>,
        head: &str,
        id: u64,
    ) -> Result<Self> {
        if id == 0 {
            return Err(crate::target::invalid());
        }
        Ok(Self {
            repository,
            pr,
            head: crate::target::sha(head)?,
            id,
        })
    }
    fn url(&self) -> String {
        format!("{}/check-runs/{}", self.repository.url(), self.id)
    }
    fn endpoint(&self) -> String {
        self.repository.endpoint(&format!("check-runs/{}", self.id))
    }
    pub fn command(&self) -> String {
        format!("dots github ci output {self}")
    }
    fn fallback(&self) -> String {
        format!(
            "gh api --hostname {} {}",
            self.repository.host,
            self.endpoint()
        )
    }
}
impl FromStr for OutputRef {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let parts: Vec<_> = value.split(':').collect();
        match parts.as_slice() {
            ["check", repo, pr, head, id] => Self::new(
                repo.parse()?,
                crate::reference::optional_id(pr)?,
                head,
                crate::target::id(id)?,
            ),
            _ => Err(crate::target::invalid()),
        }
    }
}
impl fmt::Display for OutputRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "check:{}:{}:{}:{}",
            self.repository,
            self.pr.map_or_else(|| "-".into(), |id| id.to_string()),
            self.head,
            self.id
        )
    }
}
impl From<OutputRef> for String {
    fn from(reference: OutputRef) -> Self {
        reference.to_string()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Wire {
    id: u64,
    name: String,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    output: Body,
}
#[derive(Debug, Deserialize, Serialize)]
struct Body {
    title: Option<String>,
    summary: Option<String>,
    text: Option<String>,
    #[serde(default)]
    annotations_count: usize,
}
#[derive(Debug, Deserialize, Serialize)]
struct Saved {
    reference: String,
    wire: Wire,
    observed_at: u64,
    current_pr_head: Option<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Annotation {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(rename = "annotation_level")]
    pub level: String,
    pub message: String,
    pub title: Option<String>,
    #[serde(default)]
    pub shortened: bool,
}
#[derive(Debug, Serialize)]
pub struct CheckOutput {
    pub reference: OutputRef,
    pub name: String,
    pub check_head: String,
    pub observed_head: String,
    pub observed_pr_head: Option<String>,
    pub current_pr_head: Option<String>,
    pub status: String,
    pub conclusion: Option<String>,
    pub provider_url: Option<String>,
    pub observed_at: u64,
    pub source: ObservationSource,
    pub snapshot: Option<SnapshotRef>,
    pub output: Option<LogPage>,
    pub annotations_count: usize,
    pub annotations: Option<Vec<Annotation>>,
    pub annotations_command: Option<String>,
    pub next_annotations_command: Option<String>,
    pub full_output_command: String,
    pub warnings: Vec<String>,
}

impl CheckOutput {
    /// Navigate this observation, preserving its snapshot when available.
    pub fn command(&self) -> String {
        self.snapshot.as_ref().map_or_else(
            || self.reference.command(),
            |snapshot| format!("{} --snapshot {snapshot}", self.reference.command()),
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OutputOptions<'a> {
    pub snapshot: Option<&'a SnapshotRef>,
    pub start: Option<usize>,
    pub lines: usize,
    pub search: Option<&'a str>,
    pub annotations: bool,
    pub annotation_page: usize,
}
impl Default for OutputOptions<'_> {
    fn default() -> Self {
        Self {
            snapshot: None,
            start: None,
            lines: 40,
            search: None,
            annotations: false,
            annotation_page: 1,
        }
    }
}

pub fn check_output(
    provider: &impl Provider,
    reference: &OutputRef,
    options: OutputOptions<'_>,
) -> Result<CheckOutput> {
    output_observed(provider, reference, options, None)
}
pub fn check_output_target(
    provider: &impl Provider,
    value: &str,
    repository: Option<&crate::Repository>,
    options: OutputOptions<'_>,
) -> Result<CheckOutput> {
    validate(options)?;
    if value.starts_with("check:") {
        let reference: OutputRef = value.parse()?;
        crate::target::repository(provider, repository, Some(reference.repository.clone()))?;
        return check_output(provider, &reference, options);
    }
    let (repository, id) = if value.starts_with("https://") {
        let (repo, crate::target::Resource::Check(id)) = crate::target::Resource::parse(value)?
        else {
            return Err(crate::target::invalid());
        };
        (
            crate::target::repository(provider, repository, Some(repo))?,
            id,
        )
    } else {
        (
            crate::target::repository(provider, repository, None)?,
            crate::target::id(value)?,
        )
    };
    if let Some(snapshot) = options.snapshot {
        let saved: Saved = serde_json::from_slice(&provider.read_snapshot(snapshot)?.bytes)?;
        let reference: OutputRef = saved.reference.parse()?;
        if reference.repository != repository || reference.id != id {
            return Err(Error::InvalidResponse(
                "saved output does not match the requested check".into(),
            ));
        }
        return check_output(provider, &reference, options);
    }
    let endpoint = repository.endpoint(&format!("check-runs/{id}"));
    let wire: Wire = serde_json::from_slice(&checked(
        provider,
        &["api", "--hostname", &repository.host, &endpoint],
    )?)?;
    if wire.id != id {
        return Err(Error::InvalidResponse(
            "GitHub returned a different check ID".into(),
        ));
    }
    let reference = OutputRef::new(repository, None, &wire.head_sha, id)?;
    output_known(provider, &reference, options, None, Some(wire))
}

pub(crate) fn output_observed(
    provider: &impl Provider,
    reference: &OutputRef,
    options: OutputOptions<'_>,
    head: Option<&str>,
) -> Result<CheckOutput> {
    output_known(provider, reference, options, head, None)
}
fn output_known(
    provider: &impl Provider,
    reference: &OutputRef,
    options: OutputOptions<'_>,
    head: Option<&str>,
    wire: Option<Wire>,
) -> Result<CheckOutput> {
    validate(options)?;
    let (mut saved, snapshot, source) = observation(
        provider,
        reference,
        options.snapshot,
        head,
        options.annotations,
        wire,
    )?;
    let nav = Navigation {
        failed_steps: Vec::new(),
        supports_tests: false,
        command: snapshot.as_ref().map_or_else(
            || reference.command(),
            |snapshot| format!("{} --snapshot {snapshot}", reference.command()),
        ),
        full_command: reference.fallback(),
    };
    let mut output = None;
    let mut annotations = None;
    let mut next_annotations_command = None;
    if options.annotations {
        let endpoint = format!(
            "{}/annotations?per_page=10&page={}",
            reference.endpoint(),
            options.annotation_page
        );
        match checked(
            provider,
            &["api", "--hostname", &reference.repository.host, &endpoint],
        ) {
            Ok(bytes) => {
                let mut values: Vec<Annotation> = serde_json::from_slice(&bytes)?;
                if values.len() > 10 {
                    return Err(Error::InvalidResponse(
                        "annotation page exceeded requested limit".into(),
                    ));
                }
                for value in &mut values {
                    bound_annotation(value);
                }
                if values.len() == 10
                    && options.annotation_page * 10 < saved.wire.output.annotations_count
                {
                    next_annotations_command = Some(format!(
                        "{} --annotations --annotation-page {}",
                        reference.command(),
                        options.annotation_page + 1
                    ));
                }
                annotations = Some(values);
            }
            Err(error) => saved
                .warnings
                .push(format!("Annotations unavailable: {}", error.diagnostic())),
        }
    } else {
        let body = &saved.wire.output;
        if body.title.as_deref().unwrap_or_default().is_empty()
            && body.summary.as_deref().unwrap_or_default().is_empty()
            && body.text.as_deref().unwrap_or_default().is_empty()
        {
            saved.warnings.push("GitHub has no stored title, summary or text for this check; use annotations or the provider link.".into());
        } else {
            output = Some(text_page(body, &nav, options, saved.observed_at)?);
        }
    }
    Ok(CheckOutput {
        reference: reference.clone(),
        name: saved.wire.name,
        check_head: saved.wire.head_sha,
        observed_head: reference.head.clone(),
        observed_pr_head: reference.pr.map(|_| reference.head.clone()),
        current_pr_head: saved.current_pr_head,
        status: saved.wire.status,
        conclusion: saved.wire.conclusion,
        provider_url: saved.wire.details_url,
        observed_at: saved.observed_at,
        source,
        snapshot,
        output,
        annotations_count: saved.wire.output.annotations_count,
        annotations,
        annotations_command: (saved.wire.output.annotations_count > 0)
            .then(|| format!("{} --annotations", reference.command())),
        next_annotations_command,
        full_output_command: if options.annotations {
            format!(
                "gh api --hostname {} '{}/annotations?per_page=10&page={}'",
                reference.repository.host,
                reference.endpoint(),
                options.annotation_page
            )
        } else {
            reference.fallback()
        },
        warnings: saved.warnings,
    })
}

fn text_page(
    body: &Body,
    nav: &Navigation,
    options: OutputOptions<'_>,
    observed_at: u64,
) -> Result<LogPage> {
    let text = format!(
        "Title:\n{}\nSummary:\n{}\nText:\n{}",
        body.title.as_deref().unwrap_or_default(),
        body.summary.as_deref().unwrap_or_default(),
        body.text.as_deref().unwrap_or_default()
    );
    let mut file = crate::cache::spool()?;
    file.write_all(text.as_bytes())?;
    file.rewind()?;
    let query = options.search.map_or(Query::Window, Query::Search);
    let start = options.start;
    page(
        &mut file,
        nav,
        start,
        options.lines,
        if options.snapshot.is_some() {
            LogSource::Cache
        } else {
            LogSource::Network
        },
        observed_at,
        query,
    )
}

fn validate(options: OutputOptions<'_>) -> Result<()> {
    if options.lines == 0
        || options.lines > 500
        || options.start == Some(0)
        || options.annotation_page == 0
        || options.annotation_page > 100_000
        || (options.annotations
            && (options.snapshot.is_some() || options.search.is_some() || options.start.is_some()))
        || (!options.annotations && options.annotation_page != 1)
        || options
            .search
            .is_some_and(|s| s.is_empty() || s.len() > 1024 || s.contains(['\n', '\r', '\0']))
    {
        return Err(Error::InvalidResponse(
            "invalid output window/search or annotation options; use output --help".into(),
        ));
    }
    Ok(())
}

fn observation(
    provider: &impl Provider,
    reference: &OutputRef,
    snapshot: Option<&SnapshotRef>,
    head: Option<&str>,
    annotations: bool,
    wire: Option<Wire>,
) -> Result<(Saved, Option<SnapshotRef>, ObservationSource)> {
    if let Some(snapshot) = snapshot {
        return Ok((
            saved_output(provider, reference, snapshot)?,
            Some(snapshot.clone()),
            ObservationSource::Snapshot,
        ));
    }
    let wire = if let Some(wire) = wire {
        wire
    } else {
        let bytes = checked(
            provider,
            &[
                "api",
                "--hostname",
                &reference.repository.host,
                &reference.endpoint(),
            ],
        )?;
        serde_json::from_slice::<Wire>(&bytes)?
    };
    if wire.id != reference.id || wire.head_sha.is_empty() {
        return Err(Error::InvalidResponse(
            "check-run identity did not match reference".into(),
        ));
    }
    let mut warnings = Vec::new();
    if wire.head_sha != reference.head {
        warnings.push("Check head differs from the observed PR head (a merge commit is possible); do not assume the revisions are identical.".into());
    }
    let current = if reference.pr.is_none() {
        None
    } else if let Some(head) = head {
        Some(head.into())
    } else if let Some(pr) = reference.pr {
        let url = format!("{}/pull/{pr}", reference.repository.url());
        match checked(provider, &["pr", "view", &url, "--json", "headRefOid"]) {
            Ok(bytes) => {
                #[derive(Deserialize)]
                struct Head {
                    #[serde(rename = "headRefOid")]
                    oid: String,
                }
                Some(serde_json::from_slice::<Head>(&bytes)?.oid)
            }
            Err(error) => {
                warnings.push(format!(
                    "Current PR head unavailable: {}",
                    error.diagnostic()
                ));
                None
            }
        }
    } else {
        None
    };
    if current
        .as_deref()
        .is_some_and(|head| head != reference.head)
    {
        warnings.push("PR head changed; this output belongs to the original check run.".into());
    }
    let mut saved = Saved {
        reference: reference.to_string(),
        wire,
        observed_at: crate::cache::now(),
        current_pr_head: current,
        warnings,
    };
    let snapshot = if annotations {
        None
    } else {
        let bytes = serde_json::to_vec(&saved)?;
        SnapshotRef::create(&reference.url(), &bytes)
            .filter(|snapshot| provider.save_snapshot(snapshot, &bytes, saved.observed_at))
    };
    if !annotations && snapshot.is_none() {
        saved.warnings.push(
            "Could not save this observation; follow-up output commands will query GitHub again."
                .into(),
        );
    }
    Ok((saved, snapshot, ObservationSource::Network))
}
fn saved_output(
    provider: &impl Provider,
    reference: &OutputRef,
    snapshot: &SnapshotRef,
) -> Result<Saved> {
    let invalid = || {
        Error::InvalidResponse(format!(
            "Saved check output missing, expired or invalid; refresh with {}",
            reference.command()
        ))
    };
    let cached = provider.read_snapshot(snapshot).map_err(|error| {
        Error::InvalidResponse(format!(
            "{}; refresh with {}",
            error.diagnostic(),
            reference.command()
        ))
    })?;
    if SnapshotRef::create(&reference.url(), &cached.bytes)
        .is_none_or(|actual| actual.to_string() != snapshot.to_string())
    {
        return Err(invalid());
    }
    let saved: Saved = serde_json::from_slice(&cached.bytes).map_err(|_| invalid())?;
    if saved.reference != reference.to_string()
        || saved.wire.id != reference.id
        || crate::cache::now()
            .checked_sub(saved.observed_at)
            .is_none_or(|age| age >= 300)
    {
        return Err(invalid());
    }
    Ok(saved)
}

fn bound_annotation(value: &mut Annotation) {
    for (text, limit) in [
        (&mut value.path, 512),
        (&mut value.message, 2048),
        (&mut value.level, 64),
    ] {
        value.shortened |= truncate(text, limit);
    }
    if let Some(title) = &mut value.title {
        value.shortened |= truncate(title, 256);
    }
}
fn truncate(text: &mut String, limit: usize) -> bool {
    if text.len() <= limit {
        return false;
    }
    let mut end = 0;
    for grapheme in text.graphemes(true) {
        if end + grapheme.len() > limit {
            break;
        }
        end += grapheme.len();
    }
    text.truncate(end);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Memory(RefCell<Vec<u8>>);
    impl Provider for Memory {
        fn gh(&self, _: &[&str]) -> Result<std::process::Output> {
            unreachable!("snapshot navigation must not query GitHub")
        }
        fn local_head(&self) -> Result<Option<String>> {
            unreachable!("snapshot navigation must not inspect the checkout")
        }
        fn saved_snapshot(&self, _: &SnapshotRef) -> Option<crate::CachedLog> {
            Some(crate::CachedLog {
                bytes: self.0.borrow().clone(),
                fetched_at: crate::cache::now(),
            })
        }
    }
    fn reference() -> OutputRef {
        OutputRef::create(
            "https://github.com/example/project/pull/7",
            &"a".repeat(40),
            110_000_000_001,
        )
        .unwrap()
    }
    fn save(provider: &Memory, time: u64) -> SnapshotRef {
        let reference = reference();
        let saved = Saved {
            reference: reference.to_string(),
            observed_at: time,
            current_pr_head: Some(reference.head.clone()),
            warnings: vec![],
            wire: Wire {
                id: reference.id,
                name: "build".into(),
                head_sha: reference.head.clone(),
                status: "completed".into(),
                conclusion: Some("failure".into()),
                details_url: None,
                output: Body {
                    title: Some("Failure".into()),
                    summary: None,
                    text: Some("error: undefined symbol\nnext line".into()),
                    annotations_count: 0,
                },
            },
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        let snapshot = SnapshotRef::create(&reference.url(), &bytes).unwrap();
        *provider.0.borrow_mut() = bytes;
        snapshot
    }
    #[test]
    fn external_default_prefers_compiler_diagnostics_and_explicit_start_wins() {
        let nav = Navigation {
            failed_steps: Vec::new(),
            command: "dots github ci output REF".into(),
            full_command: "gh api example".into(),
            supports_tests: false,
        };
        let body = Body {
            title: Some("Build failed".into()),
            summary: Some("Error: build failed; follow provider".into()),
            text: Some(format!(
                "{}\n# example/module\nsrc/café.go:57:22: undefined: log.Value\n{}",
                "updating module\n".repeat(100),
                "cleanup\n".repeat(100)
            )),
            annotations_count: 0,
        };
        let result = text_page(&body, &nav, OutputOptions::default(), 1).unwrap();
        assert!(
            result
                .lines
                .iter()
                .any(|line| line.contains("undefined: log.Value"))
        );
        assert!(result.navigation_note.contains("Heuristic"));
        let result = text_page(
            &body,
            &nav,
            OutputOptions {
                start: Some(1),
                ..Default::default()
            },
            1,
        )
        .unwrap();
        assert_eq!(result.start_line, 1);
        assert_eq!(result.lines.first().unwrap(), "Title:");
        let body = Body {
            text: Some("ordinary output\n".repeat(100)),
            summary: None,
            ..body
        };
        assert_eq!(
            text_page(&body, &nav, OutputOptions::default(), 1)
                .unwrap()
                .start_line,
            1
        );
    }

    #[test]
    fn saved_output_search_and_context_use_zero_queries() {
        let provider = Memory::default();
        let time = crate::cache::now().saturating_sub(10);
        let snapshot = save(&provider, time);
        let result = check_output(
            &provider,
            &reference(),
            OutputOptions {
                snapshot: Some(&snapshot),
                search: Some("undefined"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.observed_at, time);
        assert!(matches!(result.source, ObservationSource::Snapshot));
        let log = result.output.unwrap();
        let hit = log.match_index.first().unwrap();
        assert!(hit.command.contains(&format!("--snapshot {snapshot}")));
        let context = check_output(
            &provider,
            &reference(),
            OutputOptions {
                snapshot: Some(&snapshot),
                start: Some(hit.line),
                lines: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            context.output.unwrap().lines,
            vec!["error: undefined symbol"]
        );
    }
    #[test]
    fn expired_corrupt_and_wrong_check_observations_never_refetch() {
        let provider = Memory::default();
        let snapshot = save(&provider, crate::cache::now().saturating_sub(300));
        let read = |reference: &OutputRef, snapshot: &SnapshotRef| {
            check_output(
                &provider,
                reference,
                OutputOptions {
                    snapshot: Some(snapshot),
                    ..Default::default()
                },
            )
        };
        assert!(
            read(&reference(), &snapshot)
                .unwrap_err()
                .to_string()
                .contains("refresh with dots github ci output")
        );
        let snapshot = save(&provider, crate::cache::now());
        let mut other = reference();
        other.id += 1;
        assert!(read(&other, &snapshot).is_err());
        provider.0.borrow_mut().push(b'x');
        assert!(read(&reference(), &snapshot).is_err());
    }
}
