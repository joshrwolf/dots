//! Exercise the shipped process interface with neutral, hand-authored gh replies.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::Value;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "dots-cli-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).unwrap();
            let script = directory.join("gh");
            fs::write(&script, r#"#!/bin/sh
case "$*" in
  'auth token --hostname github.com') printf '%s\n' 'neutral-test-token' ;;
  'pr view --json url,number,headRefOid,state')
    printf '%s\n' '{"url":"https://github.com/example/project/pull/7","number":7,"headRefOid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","state":"OPEN"}' ;;
  'api graphql --hostname github.com --paginate --slurp'*)
    printf '%s\n' query >> "$DOTS_TEST_CHECK_REQUESTS"
    printf '%s\n' '[{"data":{"repository":{"pullRequest":{"url":"https://github.com/example/project/pull/7","number":7,"state":"OPEN","headRefOid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","commits":{"nodes":[{"commit":{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","statusCheckRollup":{"contexts":{"nodes":[{"__typename":"CheckRun","name":"tests","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/example/project/actions/runs/20/job/30","startedAt":"2026-01-01T00:00:00Z","isRequired":true,"checkSuite":{"workflowRun":{"event":"push","workflow":{"name":"CI"}}}},{"__typename":"StatusContext","context":"lint","state":"SUCCESS","createdAt":"2026-01-01T00:00:00Z","targetUrl":null,"isRequired":false}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}]}}}}}]' ;;
  'pr view https://github.com/example/project/pull/7 --json headRefOid')
    printf '%s\n' '{"headRefOid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}' ;;
  'api --hostname github.com repos/example/project/actions/jobs/30')
    printf '%s\n' '{"id":30,"run_id":20,"run_attempt":2,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","name":"tests","status":"completed","conclusion":"failure","steps":[{"name":"Test","number":1,"status":"completed","conclusion":"failure"}]}' ;;
  'api --hostname github.com repos/example/project/actions/jobs/30/logs --allow-escape-sequences')
    printf '%s\n' download >> "$DOTS_TEST_LOG_REQUESTS"
    printf '%s\n' 'setup' '##[error] expected 2, got 1' 'cleanup' ;;
  *) printf '%s\n' "unexpected gh query: $*" >&2; exit 99 ;;
esac
"#).unwrap();
            fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
            Self(directory)
        }

        fn run(&self, args: &[&str]) -> Output {
            let path = std::env::join_paths(std::iter::once(self.0.clone()).chain(
                std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
            ))
            .unwrap();
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(args)
                .current_dir(&self.0)
                .env("PATH", path)
                .env("XDG_CACHE_HOME", self.0.join("cache"))
                .env("DOTS_TEST_LARGE_LOG", self.0.join("large-log"))
                .env("DOTS_TEST_LOG_REQUESTS", self.0.join("log-requests"))
                .env("DOTS_TEST_CHECK_REQUESTS", self.0.join("check-requests"))
                .output()
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn empty_check_output_omits_synthetic_evidence_and_search() {
        let fixture = Fixture::new();
        let script = fixture.0.join("gh");
        let source = fs::read_to_string(&script).unwrap().replace("case \"$*\" in", r#"case "$*" in
  'api --hostname github.com repos/example/project/check-runs/100')
    printf '%s\n' '{"id":100,"name":"build","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"completed","conclusion":"failure","output":{"title":null,"summary":"","text":null,"annotations_count":0}}' ;;
"#);
        fs::write(script, source).unwrap();
        let output = fixture.run(&[
            "github",
            "ci",
            "output",
            "https://github.com/example/project/check-runs/100",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("no stored title, summary or text"));
        for absent in ["Evidence (", "Search:", "Go diagnostics", "Log source:"] {
            assert!(!text.contains(absent), "{text}");
        }
    }

    #[test]
    fn saved_job_navigation_never_queries_github_and_missing_logs_fail_closed() {
        let fixture = Fixture::new();
        let output = fixture.run(&["github", "ci", "--json"]);
        assert!(output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let command = value
            .pointer("/checks/0/next_command")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(command.contains("--snapshot snapshot:"));
        let args: Vec<_> = command.split_whitespace().skip(1).collect();
        // Local credentials remain accessible; every network operation now fails.
        fs::write(
            fixture.0.join("gh"),
            r#"#!/bin/sh
case "$*" in
  'auth token --hostname github.com') echo neutral-test-token ;;
  *) echo 'network must not be queried' >&2; exit 99 ;;
esac
"#,
        )
        .unwrap();
        for suffix in [
            vec![],
            vec!["--search", "expected"],
            vec!["--start", "1", "--lines", "1"],
            vec!["--test", "TestMissing"],
        ] {
            let mut query = args.clone();
            query.extend(suffix);
            let output = fixture.run(&query);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8(output.stdout).unwrap();
            assert!(text.contains("Saved job evidence: GitHub was not queried."));
            assert!(text.contains("Refresh: dots github ci logs job:"));
        }
        let mut search = args.clone();
        search.extend(["--search", "absent"]);
        let output = fixture.run(&search);
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("No matching lines."));
        assert!(!text.contains("follow Context"));
        let mut disabled = args.clone();
        disabled.push("--no-cache");
        assert!(!fixture.run(&disabled).status.success());
        // Evicted bytes must not silently trigger a download or return partial evidence.
        let logs = fixture.0.join("cache/dots/github-ci/logs");
        assert!(logs.is_dir(), "{}", logs.display());
        fs::remove_dir_all(logs).unwrap();
        let output = fixture.run(&args);
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("refresh with dots github ci logs"));
        assert!(!error.contains("network must not be queried"));
    }

    #[test]
    fn missing_test_does_not_render_an_empty_window() {
        let fixture = Fixture::new();
        let output = fixture.run(&[
            "github",
            "ci",
            "logs",
            "job:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:20:-:30",
            "--test",
            "TestMissing",
        ]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("No attributed test output"));
        assert!(!text.contains("lines omitted before"));
    }

    #[test]
    fn output_search_context_and_annotations_work_through_the_process_interface() {
        let fixture = Fixture::new();
        let script = fixture.0.join("gh");
        let source = fs::read_to_string(&script).unwrap().replace("case \"$*\" in", r#"case "$*" in
  'api --hostname github.com repos/example/project/check-runs/100')
    printf '%s\n' '{"id":100,"name":"build","head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"completed","conclusion":"failure","output":{"title":"Build","summary":"setup\nsetup\nsetup\nsetup\nerror: missing symbol","text":null,"annotations_count":1}}' ;;
  'api --hostname github.com repos/example/project/check-runs/100/annotations?per_page=10&page=1')
    printf '%s\n' '[{"path":"main.go","start_line":7,"end_line":7,"annotation_level":"failure","message":"missing symbol","title":null}]' ;;
"#);
        fs::write(&script, &source).unwrap();
        let reference =
            "check:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:100";
        let output = fixture.run(&[
            "github", "ci", "output", reference, "--search", "missing", "--json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        let context = value
            .pointer("/evidence/output/match_index/0/command")
            .unwrap()
            .as_str()
            .unwrap();
        // Only local credential lookup is allowed after saving the observation.
        fs::write(&script, "#!/bin/sh\ncase \"$*\" in 'auth token --hostname github.com') echo neutral-test-token ;; *) exit 99 ;; esac\n").unwrap();
        let output = fixture.run(&context.split_whitespace().skip(1).collect::<Vec<_>>());
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("error: missing symbol")
        );
        let rendered = fixture.run(&context.split_whitespace().skip(1).collect::<Vec<_>>());
        let rendered = String::from_utf8(rendered.stdout).unwrap();
        let beginning = rendered
            .lines()
            .find_map(|line| line.strip_prefix("From beginning: "))
            .unwrap();
        assert!(beginning.contains("--snapshot snapshot:"));
        assert!(
            fixture
                .run(&beginning.split_whitespace().skip(1).collect::<Vec<_>>())
                .status
                .success()
        );
        fs::write(
            &script,
            source
                .replace(
                    "\"__typename\":\"CheckRun\"",
                    "\"__typename\":\"CheckRun\",\"databaseId\":100",
                )
                .replace(
                    "https://github.com/example/project/actions/runs/20/job/30",
                    "https://checks.example.test/build",
                ),
        )
        .unwrap();
        let inspection = fixture.run(&["github", "ci", "--json"]);
        assert!(inspection.status.success());
        let inspection: Value = serde_json::from_slice(&inspection.stdout).unwrap();
        let more = inspection
            .pointer("/checks/0/next_command")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(more.contains("--snapshot snapshot:"));
        fs::write(&script, "#!/bin/sh\ncase \"$*\" in 'auth token --hostname github.com') echo neutral-test-token ;; *) exit 99 ;; esac\n").unwrap();
        assert!(
            fixture
                .run(&more.split_whitespace().skip(1).collect::<Vec<_>>())
                .status
                .success()
        );
        fs::write(&script, "#!/bin/sh\nexit 99\n").unwrap();
        let inaccessible = fixture.run(&more.split_whitespace().skip(1).collect::<Vec<_>>());
        assert!(!inaccessible.status.success());
        let diagnostic = String::from_utf8(inaccessible.stderr).unwrap();
        assert!(diagnostic.contains("credential"));
        assert!(diagnostic.contains("sandbox/keychain"));
        assert!(!diagnostic.contains("expired"));
        fs::write(&script, source).unwrap();
        let output = fixture.run(&["github", "ci", "output", reference, "--annotations"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("main.go:7")
        );
    }

    #[test]
    fn non_pr_targets_and_direct_job_commands_work_end_to_end() {
        let fixture = Fixture::new();
        let script = fixture.0.join("gh");
        let source=fs::read_to_string(&script).unwrap().replace("case \"$*\" in",r#"case "$*" in
  'api --hostname github.com repos/example/project/actions/runs/20')
    printf '%s\n' '{"id":20,"run_attempt":2,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","name":"Nightly","event":"schedule","head_branch":"main","status":"completed","conclusion":"failure"}' ;;
  'api --hostname github.com repos/example/project/actions/runs/20/attempts/2/jobs?per_page=100 --paginate --slurp')
    printf '%s\n' '[{"total_count":1,"jobs":[{"id":30,"run_id":20,"run_attempt":2,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","name":"tests","status":"completed","conclusion":"failure","steps":[]}]}]' ;;
  *'head=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')
    printf '%s\n' '[{"data":{"repository":{"object":{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","statusCheckRollup":null}}}}]' ;;
  'api --hostname github.com repos/example/project/git/matching-refs/heads%2Fmain --paginate --slurp')
    printf '%s\n' '[[{"ref":"refs/heads/main","object":{"type":"commit","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}]]' ;;
  'api --hostname github.com repos/example/project/git/matching-refs/tags%2Fmain --paginate --slurp')
    printf '%s\n' '[[]]' ;;
"#);
        fs::write(&script, source).unwrap();
        for (target, kind) in [
            (
                vec![
                    "--run",
                    "https://github.com/example/project/actions/runs/20",
                ],
                "run",
            ),
            (
                vec![
                    "--repo",
                    "example/project",
                    "--sha",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ],
                "commit",
            ),
            (vec!["--repo", "example/project", "--ref", "main"], "commit"),
        ] {
            let mut args = vec!["github", "ci", "--no-logs", "--json"];
            args.extend(target);
            let output = fixture.run(&args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value.pointer("/subject/kind").unwrap(), kind);
            assert!(value.get("schema_version").is_none());
            assert!(value.get("pull_request").is_none());
        }
        let output = fixture.run(&[
            "github",
            "ci",
            "logs",
            "https://github.com/example/project/actions/runs/20/job/30?pr=7",
            "--json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            value
                .pointer("/evidence/current_pr_head")
                .unwrap()
                .is_null()
        );
        assert!(
            value
                .pointer("/evidence/reference")
                .unwrap()
                .as_str()
                .unwrap()
                .contains(":20:2:30")
        );
    }

    #[test]
    fn conflicting_selectors_fail_before_any_provider_query() {
        let fixture = Fixture::new();
        fs::write(
            fixture.0.join("gh"),
            "#!/bin/sh\necho unexpected-provider >&2\nexit 99\n",
        )
        .unwrap();
        for target in [
            vec!["--pr", "7", "--ref", "main"],
            vec![
                "--run",
                "20",
                "--sha",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ],
            vec!["--attempt", "2"],
        ] {
            let mut args = vec!["github", "ci"];
            args.extend(target);
            let output = fixture.run(&args);
            assert!(!output.status.success());
            assert!(
                !String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("unexpected-provider")
            );
        }
    }

    #[test]
    fn large_logs_search_and_pages_share_one_download() {
        let fixture = Fixture::new();
        let script = fixture.0.join("gh");
        let source = fs::read_to_string(&script).unwrap().replace(
            "printf '%s\\n' 'setup' '##[error] expected 2, got 1' 'cleanup'",
            "cat \"$DOTS_TEST_LARGE_LOG\"",
        );
        fs::write(script, source).unwrap();
        fs::write(
            fixture.0.join("large-log"),
            format!(
                "{}\n--- FAIL: TestExample/cafe\u{301}\nassertion\n",
                "neutral output\n".repeat(250_000)
            ),
        )
        .unwrap();
        let reference =
            "job:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:20:-:30";
        let args = [
            "github",
            "ci",
            "logs",
            reference,
            "--search",
            "--- FAIL:",
            "--lines",
            "5",
            "--json",
        ];
        for expected_source in ["network", "cache"] {
            let output = fixture.run(&args);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                value.pointer("/evidence/log/source").unwrap(),
                expected_source
            );
            assert_eq!(value.pointer("/evidence/log/matches").unwrap(), 1);
            assert!(
                value
                    .pointer("/evidence/log/total_lines")
                    .unwrap()
                    .as_u64()
                    .unwrap()
                    > 250_000
            );
        }
        assert_eq!(
            fs::read_to_string(fixture.0.join("log-requests"))
                .unwrap()
                .lines()
                .count(),
            1
        );
    }

    #[test]
    fn local_head_is_compared_only_for_a_matching_remote() {
        let fixture = Fixture::new();
        let git = fixture.0.join("git");
        for (repository, expected) in [
            ("other/project", Value::Null),
            (
                "example/project",
                Value::from("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            ),
        ] {
            fs::write(
                &git,
                format!(
                    r#"#!/bin/sh
case "$*" in
 'rev-parse --is-inside-work-tree') echo true ;;
 'remote') echo upstream ;;
 'remote get-url --all upstream') echo git@github.com:{repository}.git ;;
 'rev-parse HEAD') echo bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ;;
 *) exit 99 ;;
esac
"#
                ),
            )
            .unwrap();
            fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
            let output = fixture.run(&[
                "github",
                "ci",
                "--pr",
                "https://github.com/example/project/pull/7",
                "--no-logs",
                "--json",
            ]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value.get("local_head").unwrap(), &expected);
        }
    }

    #[test]
    fn exhausted_check_page_does_not_claim_no_checks_exist() {
        let fixture = Fixture::new();
        let output = fixture.run(&["github", "ci", "--no-logs", "--offset", "99"]);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("No checks on this page (offset 1; 1 total)"));
        assert!(!text.contains("No checks reported"));
    }

    #[test]
    fn failed_ci_is_exit_zero_and_the_returned_command_works_verbatim() {
        let fixture = Fixture::new();
        let output = fixture.run(&["github", "ci", "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value.get("schema_version").is_none());
        assert_eq!(
            value.pointer("/checks/0/bucket"),
            Some(&Value::from("fail"))
        );
        let followup = value
            .pointer("/checks/0/next_command")
            .unwrap()
            .as_str()
            .unwrap();
        let args: Vec<_> = followup.split_whitespace().skip(1).collect();
        let output = fixture.run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("attempt 2"));
        assert!(text.contains("Log source: cache"));
        assert_eq!(
            fs::read_to_string(fixture.0.join("log-requests"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let mut uncached: Vec<_> = args
            .iter()
            .copied()
            .take_while(|arg| *arg != "--snapshot")
            .collect();
        uncached.push("--no-cache");
        let output = fixture.run(&uncached);
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("Log source: network")
        );
        assert_eq!(
            fs::read_to_string(fixture.0.join("log-requests"))
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert!(text.contains("2: ##[error] expected 2, got 1"));
    }

    #[test]
    fn check_continuation_reads_one_explicit_observation_without_api_queries() {
        let fixture = Fixture::new();
        let first = fixture.run(&[
            "github",
            "ci",
            "--no-logs",
            "--all",
            "--limit",
            "1",
            "--json",
        ]);
        assert!(
            first.status.success(),
            "{}",
            String::from_utf8_lossy(&first.stderr)
        );
        let first: Value = serde_json::from_slice(&first.stdout).unwrap();
        let command = first.get("next_command").unwrap().as_str().unwrap();
        assert!(command.contains("--snapshot snapshot:"));
        let mut args = command.split_whitespace().skip(1).collect::<Vec<_>>();
        args.push("--json");
        let second = fixture.run(&args);
        assert!(
            second.status.success(),
            "{}",
            String::from_utf8_lossy(&second.stderr)
        );
        let second: Value = serde_json::from_slice(&second.stdout).unwrap();
        assert_eq!(
            second.get("source").and_then(Value::as_str),
            Some("snapshot")
        );
        assert_eq!(second.get("observed_at"), first.get("observed_at"));
        assert_eq!(second.get("summary"), first.get("summary"));
        assert_eq!(
            fs::read_to_string(fixture.0.join("check-requests"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(!fixture.0.join("log-requests").exists());
        args.push("--no-cache");
        assert!(!fixture.run(&args).status.success());
    }

    #[test]
    fn help_is_offline_and_invalid_input_is_an_error_without_stdout_data() {
        let output = Command::new(env!("CARGO_BIN_EXE_dots"))
            .args(["github", "ci", "--help"])
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("logs"));
        let fixture = Fixture::new();
        for args in [
            vec!["github", "ci", "--pr"],
            vec!["github", "ci", "--lines", "20"],
            vec!["github", "ci", "logs", "bad"],
            vec!["github", "ci", "--json", "--json"],
        ] {
            let output = fixture.run(&args);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("unexpected gh query"));
        }
    }
}
