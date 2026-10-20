//! Exercise the command interface in-process over neutral, hand-authored
//! `gh`/`git` replies; a few tests run the shipped binary for its process contract.
#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs::{self, File};
    use std::io::Write as _;
    use std::os::unix::process::ExitStatusExt as _;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitStatus, Output};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use dots_cli::Environment;
    use github_inspect::{Transport, TransportError};
    use serde_json::Value;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const AUTH: &str = "auth token --hostname github.com";
    const GRAPHQL: &str = "api graphql --hostname github.com --paginate --slurp";
    const LOGS: &str = "api --hostname github.com repos/example/project/actions/jobs/30/logs --allow-escape-sequences";
    const CHECKS: &str = r#"[{"data":{"repository":{"pullRequest":{"url":"https://github.com/example/project/pull/7","number":7,"state":"OPEN","headRefOid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","commits":{"nodes":[{"commit":{"oid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","statusCheckRollup":{"contexts":{"nodes":[{"__typename":"CheckRun","name":"tests","status":"COMPLETED","conclusion":"FAILURE","detailsUrl":"https://github.com/example/project/actions/runs/20/job/30","startedAt":"2026-01-01T00:00:00Z","isRequired":true,"checkSuite":{"workflowRun":{"event":"push","workflow":{"name":"CI"}}}},{"__typename":"StatusContext","context":"lint","state":"SUCCESS","createdAt":"2026-01-01T00:00:00Z","targetUrl":null,"isRequired":false}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}]}}}}}]"#;
    const CHECK_RUN: &str = "api --hostname github.com repos/example/project/check-runs/100";

    /// How a `gh` argument list, joined by spaces, selects a reply.
    #[derive(Debug, Clone, Copy)]
    enum Pattern {
        Is(&'static str),
        Starts(&'static str),
        Has(&'static str),
        Ends(&'static str),
    }

    impl Pattern {
        fn matches(self, query: &str) -> bool {
            match self {
                Self::Is(value) => query == value,
                Self::Starts(value) => query.starts_with(value),
                Self::Has(value) => query.contains(value),
                Self::Ends(value) => query.ends_with(value),
            }
        }
    }

    #[derive(Debug, Clone)]
    struct Route {
        program: &'static str,
        pattern: Pattern,
        stdout: Vec<u8>,
    }

    fn gh(pattern: Pattern, stdout: impl Into<Vec<u8>>) -> Route {
        Route {
            program: "gh",
            pattern,
            stdout: stdout.into(),
        }
    }

    fn git(args: &'static str, stdout: impl Into<Vec<u8>>) -> Route {
        Route {
            program: "git",
            pattern: Pattern::Is(args),
            stdout: stdout.into(),
        }
    }

    /// The pull request, its checks, and one failed job with a short log.
    fn routes() -> Vec<Route> {
        vec![
            gh(Pattern::Is(AUTH), "neutral-test-token\n"),
            gh(
                Pattern::Is("pr view --json url,number,headRefOid,state"),
                format!(
                    r#"{{"url":"https://github.com/example/project/pull/7","number":7,"headRefOid":"{HEAD}","state":"OPEN"}}"#
                ),
            ),
            gh(Pattern::Starts(GRAPHQL), CHECKS),
            gh(
                Pattern::Is("pr view https://github.com/example/project/pull/7 --json headRefOid"),
                format!(r#"{{"headRefOid":"{HEAD}"}}"#),
            ),
            gh(
                Pattern::Is("api --hostname github.com repos/example/project/actions/jobs/30"),
                format!(
                    r#"{{"id":30,"run_id":20,"run_attempt":2,"head_sha":"{HEAD}","name":"tests","status":"completed","conclusion":"failure","steps":[{{"name":"Test","number":1,"status":"completed","conclusion":"failure"}}]}}"#
                ),
            ),
            gh(
                Pattern::Is(LOGS),
                "setup\n##[error] expected 2, got 1\ncleanup\n",
            ),
        ]
    }

    /// Which `gh` queries are answered; local `git` is unaffected.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Access {
        Full,
        /// Only the local credential lookup succeeds; the network is unreachable.
        CredentialOnly,
        /// Even the credential is inaccessible.
        Closed,
    }

    #[derive(Debug)]
    struct State {
        routes: Vec<Route>,
        access: Access,
        calls: Vec<String>,
    }

    /// In-process `gh`/`git` replies; unmatched `git` behaves as outside a checkout.
    #[derive(Debug)]
    struct Fake(Mutex<State>);

    impl Fake {
        fn reply(&self, program: &str, args: &[&str]) -> Output {
            let query = args.join(" ");
            let mut state = self.0.lock().unwrap();
            state.calls.push(format!("{program} {query}"));
            let allowed = program == "git"
                || match state.access {
                    Access::Full => true,
                    Access::CredentialOnly => query == AUTH,
                    Access::Closed => false,
                };
            let route = state
                .routes
                .iter()
                .find(|route| route.program == program && route.pattern.matches(&query));
            match (allowed, route) {
                (true, Some(route)) => output(0, route.stdout.clone(), ""),
                (false, _) => output(99, Vec::new(), "network must not be queried"),
                (true, None) if program == "git" => {
                    output(128, Vec::new(), "fatal: not a git repository")
                }
                (true, None) => output(99, Vec::new(), &format!("unexpected gh query: {query}")),
            }
        }
    }

    fn output(code: i32, stdout: Vec<u8>, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout,
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    impl Transport for Fake {
        fn git(&self, _checkout: &Path, args: &[&str]) -> Result<Output, TransportError> {
            Ok(self.reply("git", args))
        }
        fn gh(
            &self,
            _checkout: &Path,
            args: &[&str],
            _timeout: Duration,
        ) -> Result<Output, TransportError> {
            Ok(self.reply("gh", args))
        }
        fn gh_log(
            &self,
            _checkout: &Path,
            args: &[&str],
            _timeout: Duration,
            mut destination: File,
        ) -> Result<Output, TransportError> {
            let mut output = self.reply("gh", args);
            destination.write_all(&output.stdout).unwrap();
            output.stdout.clear();
            Ok(output)
        }
    }

    struct Fixture {
        directory: PathBuf,
        fake: Fake,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "dots-cli-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).unwrap();
            Self {
                directory,
                fake: Fake(Mutex::new(State {
                    routes: routes(),
                    access: Access::Full,
                    calls: Vec::new(),
                })),
            }
        }

        fn path(&self, name: &str) -> String {
            self.directory.join(name).to_str().unwrap().to_owned()
        }

        fn set(&self, routes: Vec<Route>) {
            let mut state = self.fake.0.lock().unwrap();
            state.routes = routes;
            state.access = Access::Full;
        }

        fn prepend(&self, routes: impl IntoIterator<Item = Route>) {
            let mut state = self.fake.0.lock().unwrap();
            state.routes.splice(0..0, routes);
        }

        fn restrict(&self, access: Access) {
            self.fake.0.lock().unwrap().access = access;
        }

        fn calls(&self) -> Vec<String> {
            self.fake.0.lock().unwrap().calls.clone()
        }

        fn count(&self, pattern: Pattern) -> usize {
            self.calls()
                .iter()
                .filter_map(|call| call.strip_prefix("gh "))
                .filter(|query| pattern.matches(query))
                .count()
        }

        /// The binary's stdout on success, or its `dots:` diagnostic on failure.
        fn run<S: AsRef<str>>(&self, args: &[S]) -> Result<String, String> {
            let environment = Environment {
                directory: Some(self.directory.clone()),
                cache: Some(self.directory.join("cache/dots/github")),
                github: &self.fake,
            };
            dots_cli::run_in(
                &environment,
                args.iter().map(|arg| OsString::from(arg.as_ref())),
            )
            .map_err(|error| format!("{error:#}"))
        }

        fn ok<S: AsRef<str>>(&self, args: &[S]) -> String {
            self.run(args).unwrap_or_else(|error| {
                let args = args.iter().map(AsRef::as_ref).collect::<Vec<_>>();
                panic!("dots {args:?}: {error}")
            })
        }

        fn json<S: AsRef<str>>(&self, args: &[S]) -> Value {
            serde_json::from_str(&self.ok(args)).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    /// The arguments of a returned `dots ...` follow-up command.
    fn followup(command: &str) -> Vec<&str> {
        assert!(command.starts_with("dots "), "{command}");
        command.split_whitespace().skip(1).collect()
    }

    fn pointer<'a>(value: &'a Value, path: &str) -> &'a str {
        value.pointer(path).and_then(Value::as_str).unwrap()
    }

    #[test]
    fn generated_skills_are_checked_without_writes_or_provider_access() {
        let fixture = Fixture::new();
        let output = fixture.path("skill");
        let check = ["skills", "check", "--output", &output];
        let generate = ["skills", "generate", "--output", &output];
        assert!(fixture.run(&check).is_err());
        assert!(!fixture.directory.join("skill").exists());
        fixture.ok(&generate);
        let skill = fixture.directory.join("skill/SKILL.md");
        let reference = fixture.directory.join("skill/references/commands.md");
        let original = fs::read(&skill).unwrap();
        fixture.ok(&check);
        fs::write(&reference, "stale").unwrap();
        assert!(fixture.run(&check).is_err());
        assert_eq!(fs::read_to_string(&reference).unwrap(), "stale");
        assert_eq!(fs::read(&skill).unwrap(), original);
        fixture.ok(&generate);
        fixture.ok(&check);
        assert_eq!(fixture.calls(), Vec::<String>::new());
    }

    #[test]
    fn review_followups_preserve_json_and_saved_conversations_need_no_network() {
        let fixture = Fixture::new();
        let url = "https://github.com/example/project/pull/7";
        let comment = serde_json::json!({"id":"C1","databaseId":42,"author":{"login":"reviewer"},
            "body":"Check this branch","url":format!("{url}#discussion_r42"),"createdAt":"2026-01-01T00:00:00Z","replyTo":null});
        let mut thread = serde_json::json!({"id":"T1","isResolved":false,"isOutdated":true,
            "path":"src/main.rs","line":null,"originalLine":7,"diffSide":"RIGHT","subjectType":"LINE",
            "comments":{"totalCount":2,"nodes":[comment.clone()]}});
        let index = serde_json::json!({"data":{"repository":{"pullRequest":{"url":url,"headRefOid":HEAD,
            "reviewThreads":{"totalCount":1,"nodes":[thread.clone()],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}});
        let fields = thread.as_object_mut().unwrap();
        fields.insert(
            "pullRequest".into(),
            serde_json::json!({"url":url,"headRefOid":HEAD}),
        );
        fields.insert(
            "anchor".into(),
            serde_json::json!({"nodes":[{"url":"https://github.com/example/project/pull/7#discussion_r42","databaseId":42,"diffHunk":"@@ -7 +7 @@
-old
+new","originalCommit":{"oid":HEAD},"commit":{"oid":HEAD}}]}),
        );
        let mut reply = comment.clone();
        *reply.get_mut("id").unwrap() = Value::from("C2");
        fields.insert(
            "comments".into(),
            serde_json::json!({"totalCount":2,"nodes":[comment,reply],
            "pageInfo":{"hasNextPage":false,"endCursor":null}}),
        );
        fixture.set(vec![
            gh(Pattern::Is(AUTH), "neutral-test-token\n"),
            gh(
                Pattern::Has("reviewThreads(first:50"),
                serde_json::to_vec(&index).unwrap(),
            ),
            gh(
                Pattern::Has("anchor:comments"),
                serde_json::to_vec(&serde_json::json!({"data":{"node":thread}})).unwrap(),
            ),
        ]);
        let index = fixture.json(&["github", "reviews", "--pr", url, "--json"]);
        assert_eq!(index.pointer("/summary/unresolved_outdated").unwrap(), 1);
        let command = pointer(&index, "/threads/0/next_command");
        assert!(command.starts_with("dots --json "));
        let mut args = followup(command);
        args.extend(["--limit", "1"]);
        let thread = fixture.json(&args);
        assert_eq!(thread.get("comments_total").unwrap(), 2);
        assert_eq!(thread.get("returned").unwrap(), 1);
        fixture.restrict(Access::CredentialOnly);
        let page = fixture.json(&followup(pointer(&thread, "/next_command")));
        assert_eq!(page.get("source").unwrap(), "snapshot");
        assert_eq!(page.pointer("/comments/0/id").unwrap(), "C2");
        assert_eq!(page.get("observed_at"), thread.get("observed_at"));
    }

    #[test]
    fn empty_check_output_omits_synthetic_evidence_and_search() {
        let fixture = Fixture::new();
        fixture.prepend([gh(
            Pattern::Is(CHECK_RUN),
            format!(r#"{{"id":100,"name":"build","head_sha":"{HEAD}","status":"completed","conclusion":"failure","output":{{"title":null,"summary":"","text":null,"annotations_count":0}}}}"#),
        )]);
        let text = fixture.ok(&[
            "github",
            "ci",
            "output",
            "https://github.com/example/project/check-runs/100",
        ]);
        assert!(text.contains("no stored title, summary or text"));
        for absent in ["Evidence (", "Search:", "Go diagnostics", "Log source:"] {
            assert!(!text.contains(absent), "{text}");
        }
    }

    #[test]
    fn saved_job_navigation_never_queries_github_and_missing_logs_fail_closed() {
        let fixture = Fixture::new();
        let value = fixture.json(&["github", "ci", "--json"]);
        let command = pointer(&value, "/checks/0/next_command");
        assert!(command.contains("--snapshot snapshot:"));
        let args = followup(command);
        // Local credentials remain accessible; every network operation now fails.
        fixture.restrict(Access::CredentialOnly);
        for suffix in [
            vec![],
            vec!["--search", "expected"],
            vec!["--start", "1", "--lines", "1"],
            vec!["--test", "TestMissing"],
        ] {
            let mut query = args.clone();
            query.extend(suffix);
            let value = fixture.json(&query);
            assert_eq!(value.pointer("/evidence/source").unwrap(), "snapshot");
        }
        let mut search = args.clone();
        search.extend(["--search", "absent"]);
        let text = fixture.ok(&search);
        assert!(text.contains("No matching lines."));
        assert!(!text.contains("follow Context"));
        let mut disabled = args.clone();
        disabled.push("--no-cache");
        assert!(fixture.run(&disabled).is_err());
        // Evicted bytes must not silently trigger a download or return partial evidence.
        let logs = fixture.directory.join("cache/dots/github/logs");
        assert!(logs.is_dir(), "{}", logs.display());
        fs::remove_dir_all(logs).unwrap();
        let error = fixture.run(&args).unwrap_err();
        assert!(
            error.contains("refresh with dots github ci logs"),
            "{error}"
        );
        assert!(!error.contains("network must not be queried"), "{error}");
    }

    #[test]
    fn missing_test_does_not_render_an_empty_window() {
        let fixture = Fixture::new();
        let text = fixture.ok(&[
            "github",
            "ci",
            "logs",
            "job:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:20:-:30",
            "--test",
            "TestMissing",
        ]);
        assert!(text.contains("No attributed test output"));
        assert!(!text.contains("lines omitted before"));
    }

    #[test]
    fn output_search_context_and_annotations_work_through_the_command_interface() {
        let fixture = Fixture::new();
        let mut source = vec![
            gh(
                Pattern::Is(CHECK_RUN),
                format!(
                    r#"{{"id":100,"name":"build","head_sha":"{HEAD}","status":"completed","conclusion":"failure","output":{{"title":"Build","summary":"setup\nsetup\nsetup\nsetup\nerror: missing symbol","text":null,"annotations_count":1}}}}"#
                ),
            ),
            gh(
                Pattern::Is(
                    "api --hostname github.com repos/example/project/check-runs/100/annotations?per_page=10&page=1",
                ),
                r#"[{"path":"main.go","start_line":7,"end_line":7,"annotation_level":"failure","message":"missing symbol","title":null}]"#,
            ),
        ];
        source.extend(routes());
        fixture.set(source.clone());
        let reference =
            "check:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:100";
        let value = fixture.json(&[
            "github", "ci", "output", reference, "--search", "missing", "--json",
        ]);
        let context = followup(pointer(&value, "/evidence/output/match_index/0/command"));
        // Only local credential lookup is allowed after saving the observation.
        fixture.restrict(Access::CredentialOnly);
        fixture.json(&context);
        let rendered = fixture.ok(&context
            .iter()
            .filter(|arg| **arg != "--json")
            .collect::<Vec<_>>());
        let beginning = rendered
            .lines()
            .find_map(|line| line.strip_prefix("From beginning: "))
            .unwrap();
        assert!(beginning.contains("--snapshot snapshot:"));
        fixture.ok(&followup(beginning));
        let checks = CHECKS
            .replace(
                "\"__typename\":\"CheckRun\"",
                "\"__typename\":\"CheckRun\",\"databaseId\":100",
            )
            .replace(
                "https://github.com/example/project/actions/runs/20/job/30",
                "https://checks.example.test/build",
            );
        fixture.set(source.clone());
        fixture.prepend([gh(Pattern::Starts(GRAPHQL), checks)]);
        let inspection = fixture.json(&["github", "ci", "--json"]);
        let more = followup(pointer(&inspection, "/checks/0/next_command"));
        assert!(more.contains(&"--snapshot"));
        fixture.restrict(Access::CredentialOnly);
        fixture.ok(&more);
        fixture.restrict(Access::Closed);
        let diagnostic = fixture.run(&more).unwrap_err();
        assert!(diagnostic.contains("credential"), "{diagnostic}");
        assert!(diagnostic.contains("sandbox/keychain"), "{diagnostic}");
        assert!(!diagnostic.contains("expired"), "{diagnostic}");
        fixture.set(source);
        let text = fixture.ok(&["github", "ci", "output", reference, "--annotations"]);
        assert!(text.contains("main.go:7"), "{text}");
    }

    #[test]
    fn non_pr_targets_and_direct_job_commands_work_end_to_end() {
        let fixture = Fixture::new();
        fixture.prepend([
            gh(
                Pattern::Is("api --hostname github.com repos/example/project/actions/runs/20"),
                format!(r#"{{"id":20,"run_attempt":2,"head_sha":"{HEAD}","name":"Nightly","event":"schedule","head_branch":"main","status":"completed","conclusion":"failure"}}"#),
            ),
            gh(
                Pattern::Is("api --hostname github.com repos/example/project/actions/runs/20/attempts/2/jobs?per_page=100 --paginate --slurp"),
                format!(r#"[{{"total_count":1,"jobs":[{{"id":30,"run_id":20,"run_attempt":2,"head_sha":"{HEAD}","name":"tests","status":"completed","conclusion":"failure","steps":[]}}]}}]"#),
            ),
            gh(
                Pattern::Ends("head=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                format!(r#"[{{"data":{{"repository":{{"object":{{"oid":"{HEAD}","statusCheckRollup":null}}}}}}}}]"#),
            ),
            gh(
                Pattern::Is("api --hostname github.com repos/example/project/git/matching-refs/heads%2Fmain --paginate --slurp"),
                format!(r#"[[{{"ref":"refs/heads/main","object":{{"type":"commit","sha":"{HEAD}"}}}}]]"#),
            ),
            gh(
                Pattern::Is("api --hostname github.com repos/example/project/git/matching-refs/tags%2Fmain --paginate --slurp"),
                "[[]]",
            ),
        ]);
        for (target, kind) in [
            (
                vec![
                    "--run",
                    "https://github.com/example/project/actions/runs/20",
                ],
                "run",
            ),
            (vec!["--repo", "example/project", "--sha", HEAD], "commit"),
            (vec!["--repo", "example/project", "--ref", "main"], "commit"),
        ] {
            let mut args = vec!["github", "ci", "--no-logs", "--json"];
            args.extend(target);
            let value = fixture.json(&args);
            assert_eq!(value.pointer("/subject/kind").unwrap(), kind);
            assert!(value.get("schema_version").is_none());
            assert!(value.get("pull_request").is_none());
        }
        let value = fixture.json(&[
            "github",
            "ci",
            "logs",
            "https://github.com/example/project/actions/runs/20/job/30?pr=7",
            "--json",
        ]);
        assert!(
            value
                .pointer("/evidence/current_pr_head")
                .unwrap()
                .is_null()
        );
        assert!(pointer(&value, "/evidence/reference").contains(":20:2:30"));
    }

    #[test]
    fn invalid_or_conflicting_input_fails_before_any_provider_query() {
        let fixture = Fixture::new();
        for args in [
            vec!["github", "ci", "--pr", "7", "--ref", "main"],
            vec!["github", "ci", "--run", "20", "--sha", HEAD],
            vec!["github", "ci", "--attempt", "2"],
            vec!["github", "ci", "--pr"],
            vec!["github", "ci", "--lines", "20"],
            vec!["github", "ci", "logs", "bad"],
            vec!["github", "ci", "--json", "--json"],
        ] {
            assert!(fixture.run(&args).is_err(), "{args:?}");
        }
        assert_eq!(
            fixture.count(Pattern::Starts("")),
            0,
            "{:?}",
            fixture.calls()
        );
    }

    #[test]
    fn large_logs_search_and_pages_share_one_download() {
        let fixture = Fixture::new();
        fixture.prepend([gh(
            Pattern::Is(LOGS),
            format!(
                "{}\n--- FAIL: TestExample/cafe\u{301}\nassertion\n",
                "neutral output\n".repeat(250_000)
            ),
        )]);
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
            let value = fixture.json(&args);
            assert_eq!(
                value.pointer("/evidence/log/source").unwrap(),
                expected_source
            );
            assert_eq!(value.pointer("/evidence/log/matches").unwrap(), 1);
            assert!(
                value
                    .pointer("/evidence/log/total_lines")
                    .and_then(Value::as_u64)
                    .unwrap()
                    > 250_000
            );
        }
        assert_eq!(fixture.count(Pattern::Is(LOGS)), 1);
    }

    #[test]
    fn local_head_is_compared_only_for_a_matching_remote() {
        let fixture = Fixture::new();
        for (remote, expected) in [
            ("git@github.com:other/project.git\n", Value::Null),
            (
                "git@github.com:example/project.git\n",
                Value::from("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            ),
        ] {
            let mut source = vec![
                git("rev-parse --is-inside-work-tree", "true\n"),
                git("remote", "upstream\n"),
                git("remote get-url --all upstream", remote),
                git(
                    "rev-parse HEAD",
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
                ),
            ];
            source.extend(routes());
            fixture.set(source);
            let value = fixture.json(&[
                "github",
                "ci",
                "--pr",
                "https://github.com/example/project/pull/7",
                "--no-logs",
                "--json",
            ]);
            assert_eq!(value.get("local_head").unwrap(), &expected);
        }
    }

    #[test]
    fn exhausted_check_page_does_not_claim_no_checks_exist() {
        let fixture = Fixture::new();
        let text = fixture.ok(&["github", "ci", "--no-logs", "--offset", "99"]);
        assert!(text.contains("No checks on this page (offset 1; 1 total)"));
        assert!(!text.contains("No checks reported"));
    }

    #[test]
    fn failed_ci_is_success_and_the_returned_command_works_verbatim() {
        let fixture = Fixture::new();
        let value = fixture.json(&["github", "ci", "--json"]);
        assert!(value.get("schema_version").is_none());
        assert_eq!(
            value.pointer("/checks/0/bucket"),
            Some(&Value::from("fail"))
        );
        let args = followup(pointer(&value, "/checks/0/next_command"));
        let value = fixture.json(&args);
        assert_eq!(value.pointer("/evidence/run_attempt").unwrap(), 2);
        assert_eq!(value.pointer("/evidence/log/source").unwrap(), "cache");
        assert_eq!(fixture.count(Pattern::Is(LOGS)), 1);
        let mut uncached: Vec<_> = args
            .iter()
            .copied()
            .take_while(|arg| *arg != "--snapshot")
            .collect();
        uncached.push("--no-cache");
        let value = fixture.json(&uncached);
        assert_eq!(value.pointer("/evidence/log/source").unwrap(), "network");
        assert_eq!(fixture.count(Pattern::Is(LOGS)), 2);
        assert!(
            value
                .pointer("/evidence/log/lines")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .any(|line| line == "##[error] expected 2, got 1")
        );
    }

    #[test]
    fn check_continuation_reads_one_explicit_observation_without_api_queries() {
        let fixture = Fixture::new();
        let first = fixture.json(&[
            "github",
            "ci",
            "--no-logs",
            "--all",
            "--limit",
            "1",
            "--json",
        ]);
        let command = pointer(&first, "/next_command");
        assert!(command.contains("--snapshot snapshot:"));
        let mut args = followup(command);
        let second = fixture.json(&args);
        assert_eq!(
            second.get("source").and_then(Value::as_str),
            Some("snapshot")
        );
        assert_eq!(second.get("observed_at"), first.get("observed_at"));
        assert_eq!(second.get("summary"), first.get("summary"));
        assert_eq!(fixture.count(Pattern::Starts(GRAPHQL)), 1);
        assert_eq!(fixture.count(Pattern::Is(LOGS)), 0);
        args.push("--no-cache");
        assert!(fixture.run(&args).is_err());
    }

    /// The shipped binary: offline help, stdout reserved for data, and JSON errors.
    #[test]
    fn binary_keeps_help_offline_and_errors_off_stdout() {
        let fixture = Fixture::new();
        let dots = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_dots"))
                .args(args)
                .current_dir(&fixture.directory)
                .env("PATH", "")
                .env("XDG_CACHE_HOME", fixture.directory.join("cache"))
                .output()
                .unwrap()
        };
        let help = dots(&["github", "ci", "--help"]);
        assert!(help.status.success());
        assert!(String::from_utf8(help.stdout).unwrap().contains("logs"));
        let invalid = dots(&["github", "ci", "logs", "bad"]);
        assert!(!invalid.status.success());
        assert_eq!(invalid.stdout.len(), 0);
        assert!(
            String::from_utf8(invalid.stderr)
                .unwrap()
                .starts_with("dots: ")
        );
        // With no `gh` reachable the installed transport fails to start, and JSON
        // mode reports that failure as data.
        let unavailable = dots(&[
            "--json",
            "github",
            "ci",
            "--pr",
            "https://github.com/example/project/pull/7",
        ]);
        assert!(!unavailable.status.success());
        let error: Value = serde_json::from_slice(&unavailable.stdout).unwrap();
        let error = error.get("error").and_then(Value::as_str).unwrap();
        assert!(error.contains("could not start"), "{error}");
    }
}
