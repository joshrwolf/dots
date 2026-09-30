use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{ExitStatus, Output};

use serde_json::json;

use crate::{CheckRef, LogOptions, Provider, Result, inspect, job_logs};

const URL: &str = "https://github.com/example/project/pull/7";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const JOB_URL: &str = "https://github.com/example/project/actions/runs/20/job/30";

struct Call {
    args: Vec<String>,
    code: i32,
    body: String,
    error: String,
}

impl Call {
    fn ok(args: &[&str], body: impl Into<String>) -> Self {
        Self {
            args: args.iter().map(ToString::to_string).collect(),
            code: 0,
            body: body.into(),
            error: String::new(),
        }
    }
    fn error(args: &[&str], error: &str) -> Self {
        Self {
            code: 1,
            error: error.into(),
            ..Self::ok(args, "")
        }
    }
}

struct Fake(RefCell<VecDeque<Call>>);

impl Fake {
    fn new(calls: Vec<Call>) -> Self {
        Self(RefCell::new(calls.into()))
    }
    fn done(&self) {
        assert!(self.0.borrow().is_empty(), "unconsumed provider calls");
    }
}

impl Provider for Fake {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "scripted test provider asserts exact query boundaries"
    )]
    fn gh(&self, args: &[&str]) -> Result<Output> {
        let call = self.0.borrow_mut().pop_front().expect("unexpected query");
        assert_eq!(args, call.args);
        Ok(Output {
            status: ExitStatus::from_raw(call.code << 8),
            stdout: call.body.into_bytes(),
            stderr: call.error.into_bytes(),
        })
    }
    fn local_head(&self) -> Result<Option<String>> {
        Ok(Some(HEAD.into()))
    }
}

fn pr() -> Call {
    Call::ok(
        &["pr", "view", "--json", "url,number,headRefOid,state"],
        json!({"id":"PR_example","url":URL,"number":7,"headRefOid":HEAD,"state":"OPEN"})
            .to_string(),
    )
}

fn head(value: &str) -> Call {
    Call::ok(
        &["pr", "view", URL, "--json", "headRefOid"],
        json!({"headRefOid":value}).to_string(),
    )
}

fn checks(body: &serde_json::Value, _required: bool, _code: i32) -> Call {
    graph(&body.as_array().unwrap().iter().map(|row| {
        let state = match row["bucket"].as_str().unwrap() {
            "pass" => "SUCCESS", "fail" => "FAILURE", "pending" => "PENDING", "cancel" => "CANCELLED", _ => "NEUTRAL",
        };
        json!({"__typename":"CheckRun", "name":row["name"], "status":"COMPLETED", "conclusion":state,
            "detailsUrl":row["link"], "startedAt":"2026-01-01T00:00:00Z", "isRequired":true,
            "checkSuite":{"workflowRun":{"event":"push","workflow":{"name":row["workflow"]}}}})
    }).collect::<Vec<_>>(), HEAD, None)
}

fn graph(nodes: &[serde_json::Value], head: &str, next: Option<&str>) -> Call {
    let query = format!("query={}", crate::checks::QUERY);
    Call::ok(&["api","graphql","--hostname","github.com","--paginate","--slurp","-f", &query,"-f","owner=example","-f","repo=project","-F","number=7"],
        json!([{"data":{"repository":{"pullRequest":{"url":URL,"number":7,"state":"OPEN","headRefOid":head,"commits":{"nodes":[{"commit":{"oid":head,"statusCheckRollup":{"contexts":{
            "nodes":nodes,"pageInfo":{"hasNextPage":next.is_some(),"endCursor":next}
        }}}}]}}}}}]).to_string())
}

fn check(bucket: &str, link: &str) -> serde_json::Value {
    json!({"name":"tests", "workflow":"CI", "bucket":bucket,"link":link})
}

fn reference() -> CheckRef {
    CheckRef::from_check(URL, HEAD, JOB_URL).unwrap()
}

fn job(id: u64, status: &str) -> Call {
    Call::ok(
        &[
            "api",
            "--hostname",
            "github.com",
            "repos/example/project/actions/jobs/30",
        ],
        json!({
            "id":id,"run_id":20,"run_attempt":2,"head_sha":HEAD,"name":"tests","status":status,
            "conclusion":"failure","steps":[{"number":1,"name":"Test","status":"completed","conclusion":"failure"}]
        }).to_string(),
    )
}

fn log(text: &str) -> Call {
    Call::ok(
        &[
            "api",
            "--hostname",
            "github.com",
            "repos/example/project/actions/jobs/30/logs",
            "--allow-escape-sequences",
        ],
        text,
    )
}

#[test]
fn failing_inspection_returns_exact_job_evidence_and_a_followup() {
    let failure = check("fail", JOB_URL);
    let provider = Fake::new(vec![
        pr(),
        checks(&json!([failure]), false, 1),
        job(30, "completed"),
        log("compile\n##[error] expected 2, got 1\n"),
    ]);
    let result = inspect(&provider, None, true).unwrap();
    let failed = result.checks.first().unwrap();
    assert_eq!(failed.required, Some(true));
    assert_eq!(
        failed.next_command.as_deref(),
        Some(
            failed
                .evidence
                .as_ref()
                .unwrap()
                .reference
                .command()
                .as_str()
        )
    );
    let evidence = failed.evidence.as_ref().unwrap();
    assert_eq!(evidence.run_attempt, 2);
    assert!(
        evidence
            .log
            .as_ref()
            .unwrap()
            .lines
            .iter()
            .any(|line| line.contains("expected 2"))
    );
    provider.done();
}

#[test]
fn unavailable_requiredness_is_not_optional() {
    let mut response = checks(&json!([check("pending", JOB_URL)]), false, 0);
    let mut body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    *body.pointer_mut("/0/data/repository/pullRequest/commits/nodes/0/commit/statusCheckRollup/contexts/nodes/0/isRequired").unwrap() = serde_json::Value::Null;
    body.get_mut(0).unwrap().as_object_mut().unwrap().insert(
        "errors".into(),
        json!([{"message":"HTTP 403", "path":["data", "isRequired"]}]),
    );
    response.body = body.to_string();
    response.code = 1;
    let provider = Fake::new(vec![pr(), response]);
    let result = inspect(&provider, None, true).unwrap();
    assert_eq!(result.checks.first().unwrap().required, None);
    assert!(result.warnings.first().unwrap().contains("403"));
    assert!(result.checks.first().unwrap().evidence.is_none());
    provider.done();
}

#[test]
fn external_checks_and_ambiguous_run_links_never_fetch_unrelated_logs() {
    let provider = Fake::new(vec![
        pr(),
        checks(
            &json!([
                check("fail", "https://checks.example.test/123"),
                check("fail", "https://github.com/example/project/actions/runs/20")
            ]),
            false,
            1,
        ),
    ]);
    let result = inspect(&provider, None, true).unwrap();
    assert!(
        result
            .checks
            .iter()
            .all(|check| check.reference.is_none() && check.evidence_note.is_some())
    );
    provider.done();
}

#[test]
fn changed_pr_does_not_redirect_old_job_and_missing_logs_are_explicit() {
    let provider = Fake::new(vec![
        job(30, "completed"),
        head("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        Call::error(
            &[
                "api",
                "--hostname",
                "github.com",
                "repos/example/project/actions/jobs/30/logs",
                "--allow-escape-sequences",
            ],
            "HTTP 404 logs expired",
        ),
    ]);
    let result = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: None,
            lines: 100,
            search: None,
            test: None,
        },
    )
    .unwrap();
    assert_eq!(result.run_attempt, 2);
    assert_eq!(result.observed_pr_head.as_deref(), Some(HEAD));
    assert!(result.log.is_none());
    assert!(result.warnings.iter().any(|w| w.contains("changed")));
    assert!(result.warnings.iter().any(|w| w.contains("expired")));
    provider.done();
}

#[test]
fn wrong_job_identity_stops_before_log_fetch() {
    let provider = Fake::new(vec![job(31, "completed")]);
    assert!(
        job_logs(
            &provider,
            &reference().to_string(),
            None,
            LogOptions {
                snapshot: None,
                start: None,
                lines: 100,
                search: None,
                test: None
            }
        )
        .is_err()
    );
    provider.done();
}

#[test]
fn running_job_has_no_complete_log_evidence() {
    let provider = Fake::new(vec![job(30, "in_progress"), head(HEAD)]);
    let result = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: None,
            lines: 100,
            search: None,
            test: None,
        },
    )
    .unwrap();
    assert!(result.log.is_none());
    assert!(result.warnings.first().unwrap().contains("not complete"));
    provider.done();
}

#[test]
fn revision_change_during_inspection_rejects_mixed_results() {
    let provider = Fake::new(vec![
        pr(),
        graph(&[], "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", None),
    ]);
    assert!(
        inspect(&provider, None, false)
            .unwrap_err()
            .to_string()
            .contains("changed during")
    );
    provider.done();
}

#[test]
fn absent_is_distinct_from_authentication_error() {
    let args = &["pr", "view", "--json", "url,number,headRefOid,state"];
    let absent = Fake::new(vec![Call::error(
        args,
        "no pull requests found for branch main",
    )]);
    assert!(inspect(&absent, None, true).unwrap().subject.is_none());
    absent.done();
    let denied = Fake::new(vec![Call::error(args, "authentication failed")]);
    assert!(inspect(&denied, None, true).is_err());
    denied.done();
}

#[test]
fn references_reject_shell_syntax_and_cross_repository_links() {
    assert_eq!(
        reference().to_string().parse::<CheckRef>().unwrap(),
        reference()
    );
    for value in [
        reference().to_string().replace("example", "$(id)"),
        reference().to_string().replace("project", "../project"),
        reference().to_string().replace(HEAD, "HEAD"),
    ] {
        assert!(value.parse::<CheckRef>().is_err());
    }
    assert!(
        CheckRef::from_check(
            URL,
            HEAD,
            "https://github.com/example/other/actions/runs/20/job/30"
        )
        .is_none()
    );
}

#[test]
fn excerpts_preserve_unicode_and_report_exact_line_continuation() {
    let text = (1..=200)
        .map(|line| {
            format!(
                "{line}: cafe\u{301} 👨‍👩‍👧 {}",
                if line == 90 {
                    "##[error] failure"
                } else {
                    "output"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let provider = Fake::new(vec![job(30, "completed"), head(HEAD), log(&text)]);
    let result = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: Some(80),
            lines: 20,
            search: None,
            test: None,
        },
    )
    .unwrap();
    let page = result.log.unwrap();
    assert_eq!(page.start_line, 80);
    assert_eq!(page.omitted_before, 79);
    assert_eq!(page.omitted_after, 101);
    assert!(page.lines.first().unwrap().contains("cafe\u{301} 👨‍👩‍👧"));
    assert!(page.next_command.unwrap().contains("--start 100"));
    provider.done();
}

#[test]
fn oversized_lines_are_bounded_and_have_a_full_log_escape_hatch() {
    let text = "👨‍👩‍👧".repeat(10_000);
    let provider = Fake::new(vec![job(30, "completed"), head(HEAD), log(&text)]);
    let result = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: Some(1),
            lines: 100,
            search: None,
            test: None,
        },
    )
    .unwrap();
    let page = result.log.unwrap();
    assert_eq!(page.shortened_lines, 1);
    assert!(page.lines.first().unwrap().len() < 2200);
    assert!(page.full_log_command.contains("--job 30 --log"));
    provider.done();
}

#[test]
fn older_gh_flag_rejection_retries_before_any_log_download() {
    let mut fallback = log("\u{1b}[31m##[error] failed\u{1b}[0m");
    fallback.args.pop();
    let mut rejected = log("");
    rejected.code = 1;
    rejected.error = "unknown flag: --allow-escape-sequences".into();
    let provider = Fake::new(vec![job(30, "completed"), head(HEAD), rejected, fallback]);
    let result = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: None,
            lines: 100,
            search: None,
            test: None,
        },
    )
    .unwrap();
    assert!(
        result
            .log
            .unwrap()
            .lines
            .first()
            .unwrap()
            .contains("failed")
    );
    provider.done();
}

#[test]
fn large_prs_keep_complete_counts_and_page_failure_first_details() {
    let mut rows = (0..300)
        .map(|n| {
            let mut row = check("pass", "https://checks.example.test/pass");
            *row.get_mut("name").unwrap() = json!(format!("pass-{n:03}"));
            row
        })
        .collect::<Vec<_>>();
    let mut cancelled = check("cancel", "https://checks.example.test/cancelled");
    *cancelled.get_mut("name").unwrap() = json!("aaa-cancelled");
    rows.push(cancelled);
    rows.push(check("fail", JOB_URL));
    let mut pending = check("pending", "https://checks.example.test/pending");
    *pending.get_mut("name").unwrap() = json!("pending");
    rows.push(pending);
    let provider = Fake::new(vec![pr(), checks(&json!(rows), false, 1)]);
    let result = inspect(&provider, None, false).unwrap();
    assert_eq!(result.total_checks, 303);
    assert_eq!(result.summary.passed, 300);
    assert_eq!(result.summary.failed, 1);
    assert_eq!(result.summary.pending, 1);
    assert_eq!(result.checks.len(), 3);
    assert_eq!(result.hidden_checks, 300);
    assert_eq!(result.omitted_checks, 0);
    assert_eq!(
        result.checks.first().unwrap().bucket,
        crate::status::CheckBucket::Fail
    );
    assert!(result.next_command.is_none());
    provider.done();

    let provider = Fake::new(vec![pr(), checks(&json!(rows), false, 1)]);
    let result = crate::inspect_with_options(
        &provider,
        None,
        crate::InspectOptions {
            include_evidence: false,
            offset: 300,
            show_all: true,
            name: None,
            limit: 20,
        },
    )
    .unwrap();
    assert_eq!(result.checks.len(), 3);
    assert_eq!(result.total_checks, 303);
    assert!(result.next_command.is_none());
    provider.done();
}

#[test]
fn partial_evidence_diagnostics_preserve_timeout_and_output_limit_causes() {
    let error = crate::Error::Command(github_client::Error::CommandTimedOut {
        program: "gh api example".into(),
        timeout: std::time::Duration::from_secs(30),
    });
    assert!(error.diagnostic().contains("30s"));
    let error = crate::Error::Command(github_client::Error::CommandOutputTooLarge {
        program: "gh api example".into(),
        stream: "stdout",
        limit: 2048,
    });
    assert!(error.diagnostic().contains("2048"));
}

fn combine_pages(first: &mut Call, second: &Call) {
    let mut pages: Vec<serde_json::Value> = serde_json::from_str(&first.body).unwrap();
    pages.extend(serde_json::from_str::<Vec<serde_json::Value>>(&second.body).unwrap());
    first.body = serde_json::to_string(&pages).unwrap();
}

#[test]
fn explicit_url_uses_one_paginated_query_and_rejects_partial_or_mixed_pages() {
    let mut pages = graph(&[], HEAD, Some("next"));
    combine_pages(
        &mut pages,
        &checks(&json!([check("fail", JOB_URL)]), false, 0),
    );
    let provider = Fake::new(vec![pages]);
    let result = inspect(&provider, Some(URL), false).unwrap();
    assert_eq!(result.summary.failed, 1);
    provider.done();

    let provider = Fake::new(vec![graph(&[], HEAD, Some("next"))]);
    assert!(
        inspect(&provider, Some(URL), false)
            .unwrap_err()
            .to_string()
            .contains("before all checks")
    );
    provider.done();

    let mut pages = graph(&[], HEAD, Some("next"));
    combine_pages(
        &mut pages,
        &graph(&[], "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", None),
    );
    let provider = Fake::new(vec![pages]);
    assert!(
        inspect(&provider, Some(URL), false)
            .unwrap_err()
            .to_string()
            .contains("changed during")
    );
    provider.done();
}

#[test]
fn rate_limits_fail_without_retry_or_partial_success() {
    let mut call = graph(&[], HEAD, None);
    call.code = 1;
    call.body = "[{\"errors\":[{\"message\":\"API rate limit exceeded; retry later\"}]}]".into();
    let provider = Fake::new(vec![call]);
    assert!(
        inspect(&provider, Some(URL), false)
            .unwrap_err()
            .to_string()
            .contains("rate limit")
    );
    provider.done();
}

#[test]
fn null_bubbling_preserves_graphql_error_and_rejects_missing_status() {
    let mut response = checks(&json!([check("pending", JOB_URL)]), false, 0);
    let mut body: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    *body.pointer_mut("/0/data/repository/pullRequest/commits/nodes/0/commit/statusCheckRollup/contexts/nodes/0").unwrap() = serde_json::Value::Null;
    body.get_mut(0).unwrap().as_object_mut().unwrap().insert(
        "errors".into(),
        json!([{"message":"requiredness denied", "path":["nodes", 0, "isRequired"]}]),
    );
    response.body = body.to_string();
    response.code = 1;
    let provider = Fake::new(vec![response]);
    assert!(
        inspect(&provider, Some(URL), false)
            .unwrap_err()
            .to_string()
            .contains("requiredness denied")
    );
    provider.done();
}

#[test]
fn copied_pr_tabs_are_normalized_and_mismatched_identity_is_rejected() {
    let provider = Fake::new(vec![graph(&[], HEAD, None)]);
    inspect(
        &provider,
        Some(&format!("{URL}/checks?view=details#top")),
        false,
    )
    .unwrap();
    provider.done();
    let mut response = graph(&[], HEAD, None);
    response.body = response.body.replace("/pull/7", "/pull/8");
    let provider = Fake::new(vec![response]);
    assert!(
        inspect(&provider, Some(URL), false)
            .unwrap_err()
            .to_string()
            .contains("different pull-request identity")
    );
    provider.done();
}

#[test]
fn byte_budget_stops_at_first_omitted_line_without_placeholder_rows() {
    let text = std::iter::repeat_n("x".repeat(1000), 100)
        .collect::<Vec<_>>()
        .join("\n");
    let provider = Fake::new(vec![job(30, "completed"), head(HEAD), log(&text)]);
    let page = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: Some(1),
            lines: 100,
            search: None,
            test: None,
        },
    )
    .unwrap()
    .log
    .unwrap();
    assert_eq!(page.lines.len(), 16);
    assert_eq!(page.shortened_lines, 0);
    assert!(page.byte_limit_reached);
    assert!(page.next_command.unwrap().contains("--start 17"));
    assert_eq!(page.omitted_after, 84);
    provider.done();
}

#[test]
fn first_test_failure_beats_late_wrapper_error() {
    let text = format!(
        "setup\nassertion: got 4, want 5\n--- FAIL: TestExample\n{}\n##[error] process exited",
        "cleanup\n".repeat(100)
    );
    let provider = Fake::new(vec![job(30, "completed"), head(HEAD), log(&text)]);
    let page = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: None,
            lines: 10,
            search: None,
            test: None,
        },
    )
    .unwrap()
    .log
    .unwrap();
    assert!(page.lines.iter().any(|line| line.contains("got 4, want 5")));
    provider.done();
}

#[test]
fn literal_search_finds_early_evidence_and_quotes_next_match_safely() {
    let needle = "cafe\u{301} ' $(oops)";
    let text = format!(
        "prefix\n{needle}\n{}{}",
        "cleanup\n".repeat(100),
        format!("{needle}\n").repeat(11)
    );
    let provider = Fake::new(vec![job(30, "completed"), head(HEAD), log(&text)]);
    let page = job_logs(
        &provider,
        &reference().to_string(),
        None,
        LogOptions {
            snapshot: None,
            start: None,
            lines: 5,
            search: Some(needle),
            test: None,
        },
    )
    .unwrap()
    .log
    .unwrap();
    assert_eq!(page.matches, 12);
    assert_eq!(page.match_index.len(), 10);
    assert!(
        page.next_match_command
            .as_ref()
            .unwrap()
            .contains("--start 112")
    );
    assert!(page.lines.is_empty());
    assert!(page.match_index.iter().any(|hit| hit.preview == needle));
    assert!(page.next_match_command.unwrap().contains("'\"'\"'"));
    assert_eq!(page.shortened_lines, 0);
    provider.done();
    let provider = Fake::new(vec![]);
    assert!(
        job_logs(
            &provider,
            &reference().to_string(),
            None,
            LogOptions {
                snapshot: None,
                start: None,
                lines: 5,
                search: Some(""),
                test: None
            }
        )
        .is_err()
    );
    provider.done();
}

#[test]
fn search_absence_and_exhausted_search_do_not_fall_back_to_tail() {
    for start in [None, Some(100)] {
        let provider = Fake::new(vec![job(30, "completed"), head(HEAD), log("needle\ntail")]);
        let needle = if start.is_some() { "needle" } else { "absent" };
        let page = job_logs(
            &provider,
            &reference().to_string(),
            None,
            LogOptions {
                snapshot: None,
                start,
                lines: 5,
                search: Some(needle),
                test: None,
            },
        )
        .unwrap()
        .log
        .unwrap();
        assert!(page.lines.is_empty());
        assert!(page.next_match_command.is_none());
        provider.done();
    }
}

#[test]
fn dependency_failure_does_not_consume_substantive_evidence_budget() {
    let rows = (0..3)
        .map(|index| {
            let mut row = check("fail", JOB_URL);
            *row.get_mut("name").unwrap() = json!(format!("test-{index}"));
            row
        })
        .collect::<Vec<_>>();
    let provider = Fake::new(vec![
        pr(),
        checks(&json!(rows), false, 1),
        job(30, "completed"),
        log("##[error] dependency failed"),
        job(30, "completed"),
        log("--- FAIL: TestExample\nassertion"),
        job(30, "completed"),
        log("--- FAIL: TestOther\nassertion"),
    ]);
    let result = inspect(&provider, None, true).unwrap();
    assert_eq!(
        result
            .checks
            .iter()
            .filter(|check| check.evidence.is_some())
            .count(),
        3
    );
    assert!(
        result
            .checks
            .first()
            .unwrap()
            .evidence
            .as_ref()
            .unwrap()
            .dependency_failure
    );
    provider.done();
}

#[test]
fn aggregate_steps_skip_download_and_leave_budget_for_root_failures() {
    let mut aggregate = job(30, "completed");
    let mut body: serde_json::Value = serde_json::from_str(&aggregate.body).unwrap();
    *body.pointer_mut("/steps/0/name").unwrap() = json!("Check required job results");
    aggregate.body = body.to_string();
    let provider = Fake::new(vec![
        pr(),
        checks(&json!([check("fail", JOB_URL)]), false, 1),
        aggregate,
    ]);
    let result = inspect(&provider, None, true).unwrap();
    let evidence = result.checks.first().unwrap().evidence.as_ref().unwrap();
    assert!(evidence.dependency_failure);
    assert!(evidence.log.is_none());
    assert!(evidence.full_log_command.contains("--job 30"));
    provider.done();
}

#[test]
fn three_skipped_aggregates_leave_two_substantive_slots() {
    let rows: Vec<_> = (0..5)
        .map(|index| {
            let mut row = check("fail", JOB_URL);
            *row.get_mut("name").unwrap() = json!(format!("check-{index}"));
            row
        })
        .collect();
    let mut calls = vec![pr(), checks(&json!(rows), false, 1)];
    for _ in 0..3 {
        let mut aggregate = job(30, "completed");
        let mut body: serde_json::Value = serde_json::from_str(&aggregate.body).unwrap();
        *body.pointer_mut("/steps/0/name").unwrap() = json!("Check required job results");
        aggregate.body = body.to_string();
        calls.push(aggregate);
    }
    for _ in 0..2 {
        calls.extend([job(30, "completed"), log("--- FAIL: TestExample")]);
    }
    let provider = Fake::new(calls);
    let result = inspect(&provider, None, true).unwrap();
    assert_eq!(
        result
            .checks
            .iter()
            .filter(|check| check
                .evidence
                .as_ref()
                .is_some_and(|evidence| evidence.log.is_some()))
            .count(),
        2
    );
    provider.done();
}

fn output_reference() -> crate::OutputRef {
    format!("check:github.com/example/project:7:{HEAD}:110000000001")
        .parse()
        .unwrap()
}
fn output_call() -> Call {
    Call::ok(&["api", "--hostname", "github.com", "repos/example/project/check-runs/110000000001"],
        json!({"id":110_000_000_001_u64,"name":"external build","head_sha":HEAD,
            "status":"completed","conclusion":"failure","details_url":"https://checks.example.test/build",
            "output":{"title":"Build failed","summary":"Compiler diagnostics","text":"src/main.go:7: undefined symbol\n--- FAIL: TestExample (0.1s)","annotations_count":11}}).to_string())
}

#[test]
fn external_checks_keep_large_ids_and_reuse_inspection_head_for_output() {
    for id in [json!(110_000_000_001_u64), json!("110000000001")] {
        let node = json!({"__typename":"CheckRun","databaseId":id,"name":"external build",
            "status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://checks.example.test/build",
            "startedAt":"2026-01-01T00:00:00Z","isRequired":true,"checkSuite":null});
        let provider = Fake::new(vec![pr(), graph(&[node], HEAD, None), output_call()]);
        let result = inspect(&provider, None, true).unwrap();
        let check = result.checks.first().unwrap();
        assert!(check.reference.is_none());
        assert_eq!(check.output_reference.as_ref(), Some(&output_reference()));
        assert!(
            check
                .output_evidence
                .as_ref()
                .unwrap()
                .output
                .as_ref()
                .unwrap()
                .lines
                .iter()
                .any(|line| line.contains("undefined symbol"))
        );
        provider.done();
    }
}

#[test]
fn check_output_reports_changed_head_and_search_commands_are_supported() {
    let provider = Fake::new(vec![output_call(), head(&"b".repeat(40))]);
    let result = crate::check_output(
        &provider,
        &output_reference(),
        crate::OutputOptions {
            search: Some("--- FAIL:"),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("PR head changed"))
    );
    let log = result.output.unwrap();
    assert!(log.lines.is_empty());
    let hit = log.match_index.first().unwrap();
    assert!(hit.command.contains("ci output check:"));
    assert!(hit.test_output_command.is_none());
    provider.done();
}

#[test]
fn annotations_are_explicit_paged_bounded_and_have_an_exact_fallback() {
    let annotations = (0..10)
        .map(|_| {
            json!({"path":"src/main.go","start_line":7,"end_line":7,
        "annotation_level":"failure","message":"e\u{301}".repeat(1000),"title":"compiler"})
        })
        .collect::<Vec<_>>();
    let provider = Fake::new(vec![
        output_call(),
        head(HEAD),
        Call::ok(
            &[
                "api",
                "--hostname",
                "github.com",
                "repos/example/project/check-runs/110000000001/annotations?per_page=10&page=1",
            ],
            json!(annotations).to_string(),
        ),
    ]);
    let result = crate::check_output(
        &provider,
        &output_reference(),
        crate::OutputOptions {
            annotations: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.output.is_none());
    assert!(result.snapshot.is_none());
    assert!(
        result
            .next_annotations_command
            .unwrap()
            .ends_with("--annotation-page 2")
    );
    assert!(
        result
            .full_output_command
            .ends_with("/annotations?per_page=10&page=1'")
    );
    let annotation = result.annotations.unwrap().remove(0);
    assert!(annotation.shortened);
    assert!(annotation.message.len() <= 2048);
    assert!(annotation.message.ends_with("e\u{301}"));
    provider.done();
}

#[test]
fn name_filter_includes_passed_checks_and_preserves_filter_in_paging() {
    let nodes = (0..3).map(|n| json!({"__typename":"CheckRun","name":format!("Unit's test {n}"),
        "status":"COMPLETED","conclusion":"SUCCESS","detailsUrl":"https://checks.example.test/build",
        "startedAt":"2026-01-01T00:00:00Z","isRequired":true,"checkSuite":null})).collect::<Vec<_>>();
    let provider = Fake::new(vec![pr(), graph(&nodes, HEAD, None)]);
    let result = crate::inspect_with_options(
        &provider,
        None,
        crate::InspectOptions {
            include_evidence: false,
            show_all: false,
            name: Some("UNIT'S"),
            offset: 1,
            limit: 1,
        },
    )
    .unwrap();
    assert_eq!(result.summary.passed, 3);
    assert_eq!(result.matching_checks, 3);
    assert_eq!(result.checks.len(), 1);
    assert!(
        result
            .next_command
            .unwrap()
            .contains("--name 'UNIT'\"'\"'S' --offset 2")
    );
    provider.done();
}

#[test]
fn wrong_check_identity_and_invalid_options_stop_before_more_queries() {
    let mut call = output_call();
    call.body = call.body.replace("110000000001", "110000000002");
    let provider = Fake::new(vec![call]);
    assert!(
        crate::check_output(
            &provider,
            &output_reference(),
            crate::OutputOptions::default()
        )
        .is_err()
    );
    provider.done();
    let provider = Fake::new(vec![]);
    for options in [
        crate::OutputOptions {
            lines: 501,
            ..Default::default()
        },
        crate::OutputOptions {
            annotations: true,
            search: Some("error"),
            ..Default::default()
        },
        crate::OutputOptions {
            annotation_page: 2,
            ..Default::default()
        },
    ] {
        assert!(crate::check_output(&provider, &output_reference(), options).is_err());
    }
    provider.done();
}

#[test]
fn inaccessible_annotations_preserve_check_status_and_diagnostic() {
    let provider = Fake::new(vec![
        output_call(),
        head(HEAD),
        Call::error(
            &[
                "api",
                "--hostname",
                "github.com",
                "repos/example/project/check-runs/110000000001/annotations?per_page=10&page=1",
            ],
            "HTTP 403: forbidden",
        ),
    ]);
    let result = crate::check_output(
        &provider,
        &output_reference(),
        crate::OutputOptions {
            annotations: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.conclusion.as_deref(), Some("failure"));
    assert!(result.annotations.is_none());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("403"))
    );
    provider.done();
}

fn options() -> crate::InspectOptions<'static> {
    crate::InspectOptions {
        include_evidence: false,
        offset: 0,
        limit: 20,
        show_all: true,
        name: None,
    }
}
fn api_call(path: &str, body: impl serde::Serialize, paged: bool) -> Call {
    let endpoint = format!("repos/example/project/{path}");
    let mut args = vec!["api", "--hostname", "github.com", &endpoint];
    if paged {
        args.extend(["--paginate", "--slurp"]);
    }
    Call::ok(&args, serde_json::to_string(&body).unwrap())
}
fn workflow(attempt: u64, event: &str) -> serde_json::Value {
    json!({"id":20,"run_attempt":attempt,"head_sha":HEAD,"name":"CI","event":event,"head_branch":"main","status":"completed","conclusion":"failure"})
}
fn workflow_job(attempt: u64) -> serde_json::Value {
    json!({"id":30,"run_id":20,"run_attempt":attempt,"head_sha":HEAD,"name":"tests","status":"completed","conclusion":"failure","steps":[],"check_run_url":"https://api.github.com/repos/example/project/check-runs/40"})
}
fn run_provider(attempt: u64, event: &str) -> Fake {
    Fake::new(vec![
        api_call(
            &format!("actions/runs/20/attempts/{attempt}"),
            workflow(attempt, event),
            false,
        ),
        api_call(
            &format!("actions/runs/20/attempts/{attempt}/jobs?per_page=100"),
            json!([{"total_count":1,"jobs":[workflow_job(attempt)]}]),
            true,
        ),
    ])
}
fn commit_graph(nodes: impl Into<serde_json::Value>) -> Call {
    let nodes = nodes.into();
    let query = format!("query={}", crate::checks::COMMIT_QUERY);
    let head = format!("head={HEAD}");
    Call::ok(&["api","graphql","--hostname","github.com","--paginate","--slurp","-f",&query,"-f","owner=example","-f","repo=project","-f",&head],json!([{"data":{"repository":{"object":{"oid":HEAD,"statusCheckRollup":{"contexts":{"nodes":nodes,"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}}]).to_string())
}
fn commit_node(id: u64) -> serde_json::Value {
    json!({"__typename":"CheckRun","databaseId":id,"name":"same name","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":format!("https://checks.example.test/{id}"),"startedAt":"2026-01-01T00:00:00Z","checkSuite":null})
}

#[test]
fn non_pr_runs_preserve_event_attempt_and_scope_without_pr_queries() {
    for event in [
        "push",
        "schedule",
        "workflow_dispatch",
        "release",
        "merge_group",
    ] {
        let provider = run_provider(2, event);
        let result = crate::inspect_target(
            &provider,
            crate::Target::Run {
                value: "https://github.com/example/project/actions/runs/20/attempts/2",
                attempt: None,
            },
            None,
            options(),
        )
        .unwrap();
        assert!(result.subject.as_ref().unwrap().pr().is_none());
        assert!(
            matches!(result.subject.as_ref().unwrap(),crate::Subject::Run{attempt:2,event:actual,..} if actual==event)
        );
        assert_eq!(result.summary.failed, 1);
        let check = result.checks.first().unwrap();
        assert!(check.required.is_none());
        assert_eq!(check.reference.as_ref().unwrap().attempt, Some(2));
        assert!(check.output_reference.is_some());
        provider.done();
    }
}

#[test]
fn run_and_job_attempt_mismatches_fail_before_evidence() {
    let provider = Fake::new(vec![api_call(
        "actions/runs/20/attempts/2",
        workflow(3, "push"),
        false,
    )]);
    assert!(
        crate::inspect_target(
            &provider,
            crate::Target::Run {
                value: "20",
                attempt: Some(2)
            },
            Some(&"example/project".parse().unwrap()),
            options()
        )
        .is_err()
    );
    provider.done();
    let provider = Fake::new(vec![
        api_call("actions/runs/20", workflow(2, "push"), false),
        api_call(
            "actions/runs/20/attempts/2/jobs?per_page=100",
            json!([{"total_count":1,"jobs":[workflow_job(3)]}]),
            true,
        ),
    ]);
    assert!(
        crate::inspect_target(
            &provider,
            crate::Target::Run {
                value: "20",
                attempt: None
            },
            Some(&"example/project".parse().unwrap()),
            options()
        )
        .is_err()
    );
    provider.done();
    let provider = Fake::new(vec![job(30, "completed")]);
    let reference: CheckRef = format!("job:github.com/example/project:-:{HEAD}:20:3:30")
        .parse()
        .unwrap();
    assert!(
        job_logs(
            &provider,
            &reference.to_string(),
            None,
            LogOptions {
                snapshot: None,
                start: None,
                lines: 20,
                search: None,
                test: None
            }
        )
        .is_err()
    );
    provider.done();
}

#[test]
fn jobless_runs_preserve_failure_instead_of_implying_success() {
    let provider = Fake::new(vec![
        api_call("actions/runs/20", workflow(2, "workflow_dispatch"), false),
        api_call(
            "actions/runs/20/attempts/2/jobs?per_page=100",
            json!([{"total_count":0,"jobs":[]}]),
            true,
        ),
    ]);
    let result = crate::inspect_target(
        &provider,
        crate::Target::Run {
            value: "20",
            attempt: None,
        },
        Some(&"example/project".parse().unwrap()),
        options(),
    )
    .unwrap();
    assert_eq!(result.total_checks, 0);
    assert!(
        matches!(result.subject.unwrap(),crate::Subject::Run{conclusion:Some(value),..} if value=="failure")
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("does not establish success"))
    );
    provider.done();
}

#[test]
fn direct_job_urls_and_ids_reuse_metadata_and_never_resolve_a_pr() {
    for target in [JOB_URL, "30"] {
        let provider = Fake::new(vec![
            job(30, "completed"),
            log("##[error] cafe\u{301} compiler failure"),
        ]);
        let result = crate::job_logs(
            &provider,
            target,
            Some(&"example/project".parse().unwrap()),
            crate::LogOptions {
                snapshot: None,
                start: None,
                lines: 20,
                search: None,
                test: None,
            },
        )
        .unwrap();
        assert!(result.reference.pr.is_none());
        assert_eq!(result.reference.attempt, Some(2));
        assert!(result.observed_pr_head.is_none());
        assert!(result.current_pr_head.is_none());
        assert!(
            result
                .log
                .unwrap()
                .lines
                .first()
                .unwrap()
                .contains("compiler failure")
        );
        provider.done();
    }
}

#[test]
fn direct_check_urls_and_ids_reuse_metadata_without_pr_queries() {
    for value in [
        "110000000001",
        "https://github.com/example/project/runs/110000000001?check_suite_focus=true",
    ] {
        let provider = Fake::new(vec![output_call()]);
        let result = crate::check_output_target(
            &provider,
            value,
            Some(&"example/project".parse().unwrap()),
            crate::OutputOptions::default(),
        )
        .unwrap();
        assert!(result.observed_pr_head.is_none());
        assert!(result.current_pr_head.is_none());
        assert!(result.reference.to_string().contains(":-:"));
        provider.done();
    }
}

#[test]
fn commit_collection_preserves_distinct_check_ids_and_has_no_requiredness() {
    let provider = Fake::new(vec![commit_graph(json!([
        commit_node(40),
        commit_node(41)
    ]))]);
    let result = crate::inspect_target(
        &provider,
        crate::Target::Commit(HEAD),
        Some(&"example/project".parse().unwrap()),
        options(),
    )
    .unwrap();
    assert_eq!(result.total_checks, 2);
    assert_eq!(result.summary.failed, 2);
    assert!(result.checks.iter().all(|check| check.required.is_none()));
    assert!(result.subject.as_ref().unwrap().pr().is_none());
    assert_eq!(result.subject.unwrap().head(), HEAD);
    provider.done();
}

#[test]
fn remote_ref_is_resolved_once_and_ambiguous_branch_tag_is_rejected() {
    let branch = json!([[{"ref":"refs/heads/main","object":{"type":"commit","sha":HEAD}}]]);
    let provider = Fake::new(vec![
        api_call("git/matching-refs/heads%2Fmain", branch.clone(), true),
        api_call("git/matching-refs/tags%2Fmain", json!([[]]), true),
        commit_graph(json!([])),
    ]);
    let result = crate::inspect_target(
        &provider,
        crate::Target::Ref("main"),
        Some(&"example/project".parse().unwrap()),
        options(),
    )
    .unwrap();
    assert!(
        matches!(result.subject.unwrap(),crate::Subject::Commit{requested_ref:Some(value),sha,..} if value=="main" && sha==HEAD)
    );
    provider.done();
    let provider = Fake::new(vec![
        api_call("git/matching-refs/heads%2Fmain", branch, true),
        api_call(
            "git/matching-refs/tags%2Fmain",
            json!([[{"ref":"refs/tags/main","object":{"type":"commit","sha":HEAD}}]]),
            true,
        ),
    ]);
    assert!(
        crate::inspect_target(
            &provider,
            crate::Target::Ref("main"),
            Some(&"example/project".parse().unwrap()),
            options()
        )
        .unwrap_err()
        .to_string()
        .contains("ambiguous")
    );
    provider.done();
}

#[test]
fn annotated_tags_are_peeled_by_pinned_object_id() {
    let tag = "b".repeat(40);
    let provider = Fake::new(vec![
        api_call(
            "git/matching-refs/tags%2Frelease",
            json!([[{"ref":"refs/tags/release","object":{"type":"tag","sha":tag}}]]),
            true,
        ),
        api_call(
            &format!("git/tags/{tag}"),
            json!({"sha":tag,"object":{"type":"commit","sha":HEAD}}),
            false,
        ),
        commit_graph(json!([])),
    ]);
    let result = crate::inspect_target(
        &provider,
        crate::Target::Ref("refs/tags/release"),
        Some(&"example/project".parse().unwrap()),
        options(),
    )
    .unwrap();
    assert_eq!(result.subject.unwrap().head(), HEAD);
    provider.done();
}

#[test]
fn invalid_or_conflicting_targets_do_not_query_github() {
    let provider = Fake::new(vec![]);
    let repo = "other/project".parse().unwrap();
    assert!(
        crate::inspect_target(
            &provider,
            crate::Target::Run {
                value: "https://github.com/example/project/actions/runs/20/attempts/2",
                attempt: Some(3)
            },
            None,
            options()
        )
        .is_err()
    );
    assert!(
        crate::inspect_target(
            &provider,
            crate::Target::Run {
                value: "https://github.com/example/project/actions/runs/20",
                attempt: None
            },
            Some(&repo),
            options()
        )
        .is_err()
    );
    assert!(
        crate::inspect_target(
            &provider,
            crate::Target::Commit("main"),
            Some(&repo),
            options()
        )
        .is_err()
    );
    assert!(
        crate::job_logs(
            &provider,
            JOB_URL,
            Some(&repo),
            crate::LogOptions {
                snapshot: None,
                start: None,
                lines: 20,
                search: None,
                test: None
            }
        )
        .is_err()
    );
    provider.done();
}

#[test]
fn automatic_run_evidence_reuses_listed_job_metadata() {
    let provider = Fake::new(vec![
        api_call("actions/runs/20", workflow(2, "schedule"), false),
        api_call(
            "actions/runs/20/attempts/2/jobs?per_page=100",
            json!([{"total_count":1,"jobs":[workflow_job(2)]}]),
            true,
        ),
        log("##[error] scheduled build failed"),
    ]);
    let result = crate::inspect_target(
        &provider,
        crate::Target::Run {
            value: "20",
            attempt: None,
        },
        Some(&"example/project".parse().unwrap()),
        crate::InspectOptions {
            include_evidence: true,
            ..options()
        },
    )
    .unwrap();
    let evidence = result.checks.first().unwrap().evidence.as_ref().unwrap();
    assert_eq!(evidence.run_attempt, 2);
    assert!(
        evidence
            .log
            .as_ref()
            .unwrap()
            .lines
            .first()
            .unwrap()
            .contains("scheduled build failed")
    );
    provider.done();
}

#[test]
fn commit_pages_reject_revision_changes_errors_and_missing_continuation() {
    let call = commit_graph(json!([commit_node(40)]));
    for body in [
        call.body.replace(
            &format!("\"oid\":\"{HEAD}\""),
            &format!("\"oid\":\"{}\"", "b".repeat(40)),
        ),
        call.body
            .replace("\"hasNextPage\":false", "\"hasNextPage\":true"),
        "[]".into(),
        "[{\"errors\":[{\"message\":\"access denied\"}]}]".into(),
    ] {
        let mut call = commit_graph(json!([commit_node(40)]));
        call.body = body;
        let provider = Fake::new(vec![call]);
        assert!(
            crate::inspect_target(
                &provider,
                crate::Target::Commit(HEAD),
                Some(&"example/project".parse().unwrap()),
                options()
            )
            .is_err()
        );
        provider.done();
    }
}

#[test]
fn run_pages_reject_incomplete_and_duplicate_jobs() {
    for pages in [
        json!([{"total_count":2,"jobs":[workflow_job(2)]}]),
        json!([{"total_count":2,"jobs":[workflow_job(2)]},{"total_count":2,"jobs":[workflow_job(2)]}]),
    ] {
        let provider = Fake::new(vec![
            api_call("actions/runs/20", workflow(2, "push"), false),
            api_call("actions/runs/20/attempts/2/jobs?per_page=100", pages, true),
        ]);
        assert!(
            crate::inspect_target(
                &provider,
                crate::Target::Run {
                    value: "20",
                    attempt: None
                },
                Some(&"example/project".parse().unwrap()),
                options()
            )
            .is_err()
        );
        provider.done();
    }
}

#[test]
fn commit_pagination_keeps_global_counts_and_pinned_continuation() {
    let mut call = commit_graph(json!([commit_node(40)]));
    let mut first: serde_json::Value = serde_json::from_str(&call.body).unwrap();
    let mut second = first.as_array().unwrap().first().unwrap().clone();
    *first
        .pointer_mut("/0/data/repository/object/statusCheckRollup/contexts/pageInfo")
        .unwrap() = json!({"hasNextPage":true,"endCursor":"next"});
    *second
        .pointer_mut("/data/repository/object/statusCheckRollup/contexts/nodes")
        .unwrap() = json!([commit_node(41)]);
    first.as_array_mut().unwrap().push(second);
    call.body = first.to_string();
    let provider = Fake::new(vec![call]);
    let result = crate::inspect_target(
        &provider,
        crate::Target::Commit(HEAD),
        Some(&"example/project".parse().unwrap()),
        crate::InspectOptions {
            limit: 1,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(result.summary.failed, 2);
    assert_eq!(result.total_checks, 2);
    assert_eq!(result.checks.len(), 1);
    assert!(
        result
            .next_command
            .unwrap()
            .contains(&format!("--sha {HEAD} --all --offset 1"))
    );
    provider.done();
}
