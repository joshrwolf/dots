//! Reviews of real temporary repositories.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use dots_review::{
        Anchor, AuthorKind, BaseChoice, BaseReason, Basis, Candidate, Comment, Error, FileStatus,
        Looked, NewDraft, NewThread, Overview, Preset, Published, REFRESH_AUTHOR, Reviews,
        RuleCitation, SCHEMA, Severity, ShowOptions, Side, Status, Submission, TargetSpec, USER,
        View,
    };

    struct Repo {
        root: PathBuf,
    }

    impl Repo {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dots-review-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            let repo = Self {
                root: root.canonicalize().unwrap(),
            };
            repo.git(&["init", "-q", "-b", "main"]);
            repo.write(".gitignore", "ignored.log\n");
            repo.write(
                "AGENTS.md",
                "# Rules\n\n- Errors are returned, not logged.\n",
            );
            repo.write("src/lib.rs", &numbered(1..=20));
            repo.commit("initial");
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
                .env("GIT_COMMITTER_NAME", "Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }

        fn write(&self, path: &str, content: &str) {
            let path = self.root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn commit(&self, message: &str) -> String {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--no-gpg-sign", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }

        fn reviews(&self) -> Reviews {
            Reviews::open(&self.root).unwrap()
        }

        /// A linked worktree on a new branch `linked`, beside the primary one.
        fn linked(&self) -> PathBuf {
            let linked = self.root.with_extension("linked");
            let _ = fs::remove_dir_all(&linked);
            self.git(&[
                "worktree",
                "add",
                "-q",
                linked.to_str().unwrap(),
                "-b",
                "linked",
            ]);
            linked
        }

        fn path(&self) -> &Path {
            &self.root
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
            let _ = fs::remove_dir_all(self.root.with_extension("linked"));
        }
    }

    fn numbered(lines: std::ops::RangeInclusive<u32>) -> String {
        lines
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    /// `src/lib.rs` with line 10 changed.
    fn change_line_ten(repo: &Repo) {
        let mut lines: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        lines[9] = "line ten, changed".into();
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
    }

    /// A finding: a thread with every finding field.
    fn finding(anchor: Option<(&str, Side, u32, u32)>) -> NewThread {
        NewThread {
            title: Some("Handle the empty case".into()),
            severity: Some(Severity::Important),
            pre_existing: false,
            confidence: Some(0.9),
            angle: Some("correctness".into()),
            rule: None,
            ..comment(anchor)
        }
    }

    /// A plain comment thread.
    fn comment(anchor: Option<(&str, Side, u32, u32)>) -> NewThread {
        NewThread {
            anchor: anchor.map(|(path, side, start, end)| Anchor::lines(path, side, start, end)),
            body: "When the list is empty this indexes past the end.".into(),
            author: "fixture".into(),
            title: None,
            severity: None,
            pre_existing: false,
            confidence: None,
            angle: None,
            rule: None,
        }
    }

    fn entry(author: &str, at: i64, body: Option<&str>, status: Option<Status>) -> Comment {
        Comment {
            author: author.into(),
            at,
            body: body.map(Into::into),
            status,
        }
    }

    #[test]
    fn uncommitted_review_pins_tracked_and_untracked_content_but_not_ignored_files() {
        let repo = Repo::new();
        change_line_ten(&repo);
        repo.write("notes/new.md", "new file\n");
        repo.write("ignored.log", "noise\n");
        let overview = repo
            .reviews()
            .start(
                TargetSpec::Uncommitted,
                Some("standard".into()),
                Some("pi-session".into()),
                100,
            )
            .unwrap();

        let paths: Vec<_> = overview
            .files
            .iter()
            .map(|file| file.change.path.as_str())
            .collect();
        assert_eq!(paths, ["notes/new.md", "src/lib.rs"]);
        let revision = overview.review.current();
        assert_eq!(revision.base, repo.git(&["rev-parse", "HEAD"]));
        assert_eq!(revision.basis.reason, BaseReason::Head);
        assert_eq!(
            repo.git(&["show", &format!("{}:src/lib.rs", revision.head)])
                .lines()
                .nth(9),
            Some("line ten, changed")
        );
        assert_eq!(
            overview.diff_command,
            format!(
                "git --attr-source={1} diff --no-renames {0} {1}",
                revision.base, revision.head
            )
        );
        assert_eq!(
            repo.git(&["status", "--porcelain"]).lines().count(),
            2,
            "the checkout's index is untouched"
        );
        assert!(overview.review.id.starts_with("r-"));
        assert_eq!(overview.review.profile.as_deref(), Some("standard"));
        assert_eq!(overview.review.owner.as_deref(), Some("pi-session"));
        let listed = repo.reviews().list(false, false).unwrap();
        assert_eq!(listed[0].owner.as_deref(), Some("pi-session"));
        assert_eq!(overview.review.worktree.path, repo.root);
        assert_eq!(overview.review.worktree.branch.as_deref(), Some("main"));
        assert_eq!(
            overview.file,
            repo.root
                .join(".git/dots/reviews")
                .join(format!("{}.json", overview.review.id))
        );
    }

    #[test]
    fn empty_changes_and_malformed_targets_are_refused() {
        let repo = Repo::new();
        let reviews = repo.reviews();
        assert!(matches!(
            reviews.start(TargetSpec::Uncommitted, None, None, 1),
            Err(Error::NothingToReview(_))
        ));
        repo.write("new.txt", "new\n");
        assert!(
            matches!(
                reviews.start(TargetSpec::Worktree { base: None }, None, None, 1),
                Err(Error::Invalid(_))
            ),
            "the worktree review is opened, never started"
        );
        fs::remove_file(repo.path().join("new.txt")).unwrap();
        assert!(matches!(
            reviews.start(
                TargetSpec::Base {
                    reference: "missing".into()
                },
                None,
                None,
                1
            ),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            reviews.start(TargetSpec::Uncommitted, Some("Not A Name".into()), None, 1),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            reviews.start(TargetSpec::Uncommitted, None, Some("two\nlines".into()), 1),
            Err(Error::Invalid(_))
        ));
        assert!(
            reviews.list(true, false).unwrap().is_empty(),
            "refused starts leave nothing behind"
        );
    }

    #[test]
    fn base_and_uncommitted_targets_compare_exact_revisions() {
        let repo = Repo::new();
        let initial = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "-q", "-c", "feature"]);
        change_line_ten(&repo);
        let feature = repo.commit("change ten");
        repo.write("later.txt", "uncommitted\n");

        let reviews = repo.reviews();
        let base = reviews
            .start(
                TargetSpec::Base {
                    reference: "main".into(),
                },
                None,
                None,
                1,
            )
            .unwrap();
        assert_eq!(base.review.current().base, initial);
        let paths: Vec<_> = base
            .files
            .iter()
            .map(|file| file.change.path.as_str())
            .collect();
        assert_eq!(paths, ["later.txt", "src/lib.rs"]);

        assert_eq!(base.review.current().head_commit, feature);
        assert_eq!(
            base.review.current().basis,
            Basis {
                reference: "main".into(),
                reason: BaseReason::Given,
                candidates: Vec::new(),
            }
        );

        let uncommitted = reviews
            .start(TargetSpec::Uncommitted, None, None, 2)
            .unwrap();
        let revision = uncommitted.review.current();
        assert_eq!(revision.base, feature);
        let paths: Vec<_> = uncommitted
            .files
            .iter()
            .map(|file| file.change.path.as_str())
            .collect();
        assert_eq!(paths, ["later.txt"]);
        let listed: Vec<_> = reviews
            .list(false, false)
            .unwrap()
            .into_iter()
            .map(|summary| summary.id)
            .collect();
        assert_eq!(listed, [base.review.id, uncommitted.review.id]);
    }

    #[test]
    fn threads_must_anchor_inside_the_reviewed_diff_and_batches_are_all_or_nothing() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;

        let mut bad_rule = finding(None);
        bad_rule.rule = Some(RuleCitation {
            path: "AGENTS.md".into(),
            start: 3,
            end: 9,
        });
        let mut bad_text = finding(None);
        bad_text.title = Some("two\nlines".into());
        bad_text.author = " ".into();
        bad_text.confidence = Some(1.5);
        bad_text.angle = Some("Correctness".into());
        let batch = vec![
            finding(Some(("src/lib.rs", Side::New, 10, 10))),
            finding(Some(("src/lib.rs", Side::New, 1, 2))),
            finding(Some(("AGENTS.md", Side::New, 1, 1))),
            finding(Some(("../escape", Side::New, 1, 1))),
            bad_rule,
            bad_text,
        ];
        let Err(Error::Rejected(rejections)) = reviews.add(&id, batch, None, 2) else {
            panic!("invalid batch was accepted");
        };
        let fields: Vec<_> = rejections
            .iter()
            .map(|rejection| (rejection.index, rejection.field))
            .collect();
        assert_eq!(
            fields,
            [
                (1, "anchor"),
                (2, "anchor"),
                (3, "anchor"),
                (4, "rule"),
                (5, "title"),
                (5, "author"),
                (5, "angle"),
                (5, "confidence"),
            ]
        );
        assert!(
            rejections[0].message.contains("hunks cover 7-13"),
            "{}",
            rejections[0].message
        );
        assert_eq!(
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .review
                .threads
                .len(),
            0
        );

        let mut cited = finding(Some(("src/lib.rs", Side::Old, 10, 10)));
        cited.rule = Some(RuleCitation {
            path: "AGENTS.md".into(),
            start: 3,
            end: 3,
        });
        let added = reviews
            .add(
                &id,
                vec![
                    finding(Some(("src/lib.rs", Side::New, 7, 13))),
                    cited,
                    comment(None),
                ],
                None,
                3,
            )
            .unwrap();
        let ids: Vec<_> = added.iter().map(|thread| thread.id.as_str()).collect();
        assert_eq!(ids, ["t1", "t2", "t3"]);
        assert!(
            added
                .iter()
                .all(|thread| thread.status == Status::Open && thread.revision == 0)
        );
        assert_eq!(
            added[2].comments,
            [entry(
                "fixture",
                3,
                Some("When the list is empty this indexes past the end."),
                None
            )],
            "the opening comment holds the body"
        );
        assert_eq!((added[2].title.as_ref(), added[2].severity), (None, None));
    }

    #[test]
    fn threads_anchored_against_an_earlier_revision_are_refused() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let at_ten = || vec![comment(Some(("src/lib.rs", Side::New, 10, 10)))];
        let added = reviews.add(&id, at_ten(), Some(0), 2).unwrap();
        assert_eq!(added[0].revision, 0);

        // Line ten moves to line eleven, so revision 0's coordinates are stale.
        let mut lines: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        lines[9] = "line ten, changed".into();
        lines.insert(0, "header".into());
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        reviews.refresh(&id, 3).unwrap();

        let moved = reviews.add(&id, at_ten(), Some(0), 4);
        assert!(
            matches!(
                moved,
                Err(Error::Moved {
                    expected: 0,
                    current: 1,
                    ..
                })
            ),
            "{moved:?}"
        );
        assert!(
            matches!(
                reviews.add(&id, at_ten(), Some(2), 4),
                Err(Error::Moved { expected: 2, .. })
            ),
            "a revision that does not exist is refused too"
        );
        assert_eq!(
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .review
                .threads
                .len(),
            1,
            "refused batches record nothing"
        );
        let added = reviews
            .add(
                &id,
                vec![comment(Some(("src/lib.rs", Side::New, 11, 11)))],
                Some(1),
                5,
            )
            .unwrap();
        assert_eq!(added[0].revision, 1);
    }

    #[test]
    fn refresh_moves_unchanged_threads_and_outdates_changed_ones() {
        let repo = Repo::new();
        let mut lines: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        lines[4] = "line five, changed".into();
        lines[14] = "line fifteen, changed".into();
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let threads = vec![
            finding(Some(("src/lib.rs", Side::New, 5, 5))),
            finding(Some(("src/lib.rs", Side::New, 15, 15))),
            finding(None),
            comment(Some(("src/lib.rs", Side::New, 14, 14))),
        ];
        reviews.add(&id, threads, None, 2).unwrap();
        reviews
            .set(&id, "t4", Status::Dismissed, "verifier".into(), None, 3)
            .unwrap();

        let unchanged = reviews.refresh(&id, 4).unwrap();
        assert!(!unchanged.changed && unchanged.review.revisions.len() == 1);

        // Two lines inserted at the top shift everything; line five changes again.
        lines[4] = "line five, fixed".into();
        lines.insert(0, "header two".into());
        lines.insert(0, "header one".into());
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        let refreshed = reviews.refresh(&id, 5).unwrap();
        assert_eq!(refreshed.carried, ["t2", "t3"]);
        assert_eq!(refreshed.outdated, ["t1"]);
        let delta = refreshed.delta.as_ref().expect("the content moved");
        assert_eq!(delta.files.len(), 1);
        let (from, to) = (&delta.from.head, &delta.to.head);
        assert_eq!(
            delta.diff_command,
            format!("git --attr-source={to} diff --no-renames {from} {to}")
        );

        let threads = &refreshed.review.threads;
        assert_eq!(threads[0].status, Status::Outdated);
        assert_eq!(
            threads[0].comments.last(),
            Some(&entry(REFRESH_AUTHOR, 5, None, Some(Status::Outdated))),
            "refresh records the status change it makes"
        );
        assert_eq!(
            (
                threads[0].revision,
                threads[0].anchor.as_ref().unwrap().lines.unwrap().start
            ),
            (0, 5)
        );
        assert_eq!(threads[1].status, Status::Open);
        assert_eq!(
            (
                threads[1].revision,
                threads[1].anchor.as_ref().unwrap().lines.unwrap().start
            ),
            (1, 17)
        );
        assert_eq!(threads[2].revision, 1);
        assert_eq!(
            (
                threads[3].status,
                threads[3].revision,
                threads[3].anchor.as_ref().unwrap().lines.unwrap().start
            ),
            (Status::Dismissed, 1, 16),
            "dispositions follow unchanged lines"
        );
        assert!(
            matches!(
                reviews.set(&id, "t1", Status::Open, "user".into(), None, 6),
                Err(Error::Invalid(_))
            ),
            "an outdated thread cannot be reopened at stale lines"
        );
        reviews
            .set(&id, "t4", Status::Open, "user".into(), None, 6)
            .unwrap();

        // A disposition whose lines change stays behind and cannot be reopened.
        reviews
            .set(&id, "t2", Status::Fixed, "user".into(), None, 7)
            .unwrap();
        lines[16] = "line fifteen, fixed".into();
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        let refreshed = reviews.refresh(&id, 8).unwrap();
        assert_eq!(
            (refreshed.carried.as_slice(), refreshed.outdated.as_slice()),
            (&["t3".to_owned(), "t4".to_owned()][..], &[][..])
        );
        let fixed = &refreshed.review.threads[1];
        assert_eq!((fixed.status, fixed.revision), (Status::Fixed, 1));
        assert!(matches!(
            reviews.set(&id, "t2", Status::Accepted, "user".into(), None, 9),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn replies_and_status_changes_record_their_authors_in_order() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        reviews.add(&id, vec![finding(None)], None, 2).unwrap();

        reviews
            .reply(&id, "t1", "user".into(), "Is this reachable?".into(), 3)
            .unwrap();
        reviews
            .set(
                &id,
                "t1",
                Status::Accepted,
                "verifier".into(),
                Some("Reproduced with an empty list.".into()),
                4,
            )
            .unwrap();
        let fixed = reviews
            .set(&id, "t1", Status::Fixed, "agent".into(), None, 5)
            .unwrap();
        assert_eq!((fixed.status, fixed.updated_at), (Status::Fixed, 5));
        assert_eq!(
            fixed.comments,
            [
                entry(
                    "fixture",
                    2,
                    Some("When the list is empty this indexes past the end."),
                    None
                ),
                entry("user", 3, Some("Is this reachable?"), None),
                entry(
                    "verifier",
                    4,
                    Some("Reproduced with an empty list."),
                    Some(Status::Accepted)
                ),
                entry("agent", 5, None, Some(Status::Fixed)),
            ]
        );
        let shown = reviews.show(&id, ShowOptions::default()).unwrap().review;
        assert_eq!(shown.threads[0], fixed);
        assert_eq!(shown.updated_at, 5);

        assert!(matches!(
            reviews.set(&id, "t1", Status::Outdated, "user".into(), None, 6),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            reviews.reply(&id, "t1", "user".into(), " \n".into(), 6),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            reviews.set(&id, "t1", Status::Open, "two\nlines".into(), None, 6),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            reviews.reply(&id, "t9", "user".into(), "Hello".into(), 6),
            Err(Error::NoThread { .. })
        ));
        assert!(matches!(
            reviews.show("r-00000000", ShowOptions::default()),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(
            reviews.show("../../etc", ShowOptions::default()),
            Err(Error::Invalid(_))
        ));
        assert_eq!(
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .review
                .threads[0]
                .comments
                .len(),
            4,
            "refused comments leave nothing behind"
        );
    }

    #[test]
    fn the_default_review_is_this_worktrees_most_recently_updated() {
        let repo = Repo::new();
        let reviews = repo.reviews();
        assert!(matches!(reviews.latest(), Err(Error::NoReviews)));
        change_line_ten(&repo);
        // Every change happens in the same second: the order is the order of
        // changes, not of timestamps or ids.
        let start = || {
            reviews
                .start(TargetSpec::Uncommitted, None, None, 1)
                .unwrap()
                .review
                .id
        };
        let ids: Vec<String> = (0..6).map(|_| start()).collect();
        assert_eq!(reviews.latest().unwrap(), ids[5]);
        for id in &ids {
            reviews.add(id, vec![comment(None)], None, 1).unwrap();
            assert_eq!(&reviews.latest().unwrap(), id);
        }
        reviews
            .reply(&ids[2], "t1", "user".into(), "Again.".into(), 1)
            .unwrap();
        assert_eq!(reviews.latest().unwrap(), ids[2]);
        reviews.refresh(&ids[4], 1).unwrap();
        assert_eq!(
            reviews.latest().unwrap(),
            ids[2],
            "a refresh that finds nothing new changes nothing"
        );

        let linked = repo.linked();
        let other = Reviews::open(&linked).unwrap();
        assert!(
            matches!(other.latest(), Err(Error::NoReviews)),
            "another worktree's reviews are not its default"
        );
        other
            .set(&ids[0], "t1", Status::Dismissed, "user".into(), None, 1)
            .unwrap();
        assert_eq!(
            reviews.latest().unwrap(),
            ids[0],
            "a change from another worktree counts in the review's own"
        );
    }

    #[test]
    fn the_reviewed_patch_ignores_the_calling_worktrees_attributes() {
        let repo = Repo::new();
        let linked = repo.linked();
        fs::write(linked.join("src/lib.rs"), numbered(1..=21)).unwrap();
        let other = Reviews::open(&linked).unwrap();
        let id = other
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        // Only the primary checkout calls src/lib.rs binary.
        repo.write(".gitattributes", "src/lib.rs -diff\n");

        let primary = repo.reviews();
        let shown = primary.show(&id, ShowOptions::default()).unwrap();
        assert_eq!(shown.files.len(), 1);
        assert!(!shown.files[0].change.binary, "{:?}", shown.files);
        assert_eq!(
            (shown.files[0].change.added, shown.files[0].change.removed),
            (1, 0)
        );
        primary
            .add(
                &id,
                vec![finding(Some(("src/lib.rs", Side::New, 21, 21)))],
                None,
                2,
            )
            .unwrap();
        let args: Vec<&str> = shown.diff_command.split(' ').skip(1).collect();
        assert!(
            repo.git(&args).contains("+line 21"),
            "diff_command prints the reviewed patch from any worktree"
        );

        fs::write(linked.join("src/lib.rs"), numbered(1..=22)).unwrap();
        let refreshed = primary.refresh(&id, 3).unwrap();
        assert_eq!(refreshed.carried, ["t1"]);
        assert!(!refreshed.delta.unwrap().files[0].binary);
    }

    #[test]
    fn each_revision_pins_a_snapshot_commit_of_its_head() {
        let repo = Repo::new();
        change_line_ten(&repo);
        repo.write("notes/new.md", "new file\n");
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        repo.write("notes/new.md", "new file, edited\n");
        reviews.refresh(&id, 2).unwrap();
        let review = reviews.show(&id, ShowOptions::default()).unwrap().review;
        assert_eq!(review.revisions.len(), 2);
        for revision in &review.revisions {
            let snapshot = &revision.snapshot;
            assert_eq!(
                repo.git(&["rev-parse", &format!("{snapshot}^{{tree}}")]),
                revision.head
            );
            assert_eq!(
                repo.git(&["diff", &revision.base, snapshot]),
                repo.git(&["diff", &revision.base, &revision.head]),
                "base..snapshot is the reviewed diff"
            );
        }
        assert_eq!(
            repo.git(&["rev-parse", &format!("refs/dots/reviews/{id}")]),
            review.current().snapshot,
            "the newest snapshot holds the pin"
        );
    }

    #[test]
    fn list_all_spans_every_worktree_of_the_repository() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let primary = repo.reviews();
        let own = primary
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let linked = repo.linked();
        fs::write(linked.join("src/lib.rs"), "replaced\n").unwrap();
        let other = Reviews::open(&linked).unwrap();
        let theirs = other
            .start(TargetSpec::Uncommitted, None, None, 2)
            .unwrap()
            .review
            .id;
        other.add(&theirs, vec![comment(None)], None, 3).unwrap();

        let ids = |reviews: &Reviews, all| -> Vec<String> {
            reviews
                .list(all, false)
                .unwrap()
                .into_iter()
                .map(|summary| summary.id)
                .collect()
        };
        assert_eq!(ids(&primary, false), [own.as_str()]);
        assert_eq!(ids(&other, false), [theirs.as_str()]);
        assert_eq!(ids(&primary, true), [own.as_str(), theirs.as_str()]);

        let listed = other.list(true, false).unwrap();
        assert_eq!(
            (
                listed[0].worktree.path.as_path(),
                listed[0].worktree.branch.as_deref()
            ),
            (repo.path(), Some("main"))
        );
        assert_eq!(
            (
                listed[1].worktree.path.as_path(),
                listed[1].worktree.branch.as_deref()
            ),
            (linked.as_path(), Some("linked"))
        );
        assert_eq!(listed[1].file.parent().unwrap(), other.directory());
        assert!(listed[1].file.is_file());
        assert_eq!(listed[1].threads.get(&Status::Open), Some(&1));
        assert_eq!(listed[1].updated_at, 3);
        assert!(listed[0].threads.is_empty());
    }

    #[test]
    fn any_worktree_reads_and_changes_another_worktrees_review() {
        let repo = Repo::new();
        let linked = repo.linked();
        fs::write(linked.join("src/lib.rs"), numbered(1..=21)).unwrap();
        let other = Reviews::open(&linked).unwrap();
        let id = other
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        other
            .add(
                &id,
                vec![finding(Some(("src/lib.rs", Side::New, 21, 21)))],
                None,
                2,
            )
            .unwrap();

        let primary = repo.reviews();
        let shown = primary.show(&id, ShowOptions::default()).unwrap();
        assert_eq!(shown.file, other.directory().join(format!("{id}.json")));
        assert_eq!(shown.review.worktree.path, linked);
        primary
            .reply(&id, "t1", "user".into(), "Seen from main.".into(), 3)
            .unwrap();
        assert_eq!(
            other
                .show(&id, ShowOptions::default())
                .unwrap()
                .review
                .threads[0]
                .comments
                .len(),
            2,
            "the reply lands in the linked worktree's review file"
        );
        assert!(
            !repo
                .root
                .join(format!(".git/dots/reviews/{id}.json"))
                .exists()
        );

        // Refresh from the primary checkout captures the linked working tree.
        fs::write(linked.join("src/lib.rs"), numbered(1..=22)).unwrap();
        change_line_ten(&repo);
        let refreshed = primary.refresh(&id, 4).unwrap();
        assert!(refreshed.changed);
        assert_eq!(refreshed.carried, ["t1"]);
        let head = &refreshed.review.current().head;
        assert_eq!(
            repo.git(&["show", &format!("{head}:src/lib.rs")]),
            numbered(1..=22).trim_end()
        );
        assert_eq!(
            other
                .show(&id, ShowOptions::default())
                .unwrap()
                .review
                .revisions
                .len(),
            2
        );

        primary.remove(&id).unwrap();
        assert!(matches!(
            other.show(&id, ShowOptions::default()),
            Err(Error::NotFound(_))
        ));
        assert_eq!(repo.git(&["for-each-ref", "refs/dots/"]), "");
    }

    #[test]
    fn reviewed_content_survives_garbage_collection_until_removed() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let head = reviews
            .show(&id, ShowOptions::default())
            .unwrap()
            .review
            .current()
            .head
            .clone();
        repo.git(&["checkout", "--", "src/lib.rs"]);
        repo.git(&["gc", "-q", "--prune=now"]);
        reviews.show(&id, ShowOptions::default()).unwrap();
        repo.git(&["cat-file", "-e", &format!("{head}:src/lib.rs")]);
        repo.git(&["rev-parse", "--verify", &format!("refs/dots/reviews/{id}")]);

        reviews.remove(&id).unwrap();
        let listed = repo.git(&["for-each-ref", "refs/dots/"]);
        assert_eq!(listed, "", "removing a review releases its content");
    }

    #[test]
    fn linked_worktree_reviews_survive_primary_gc_and_release_with_the_worktree() {
        let repo = Repo::new();
        let linked = repo.root.with_extension("linked");
        let _ = fs::remove_dir_all(&linked);
        repo.git(&[
            "worktree",
            "add",
            "-q",
            linked.to_str().unwrap(),
            "-b",
            "linked",
        ]);
        fs::write(linked.join("src/lib.rs"), "replaced\n").unwrap();
        let reviews = Reviews::open(&linked).unwrap();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        fs::write(linked.join("src/lib.rs"), numbered(1..=20)).unwrap();
        repo.git(&["gc", "-q", "--prune=now"]);
        reviews.show(&id, ShowOptions::default()).unwrap();

        repo.git(&["worktree", "remove", "--force", linked.to_str().unwrap()]);
        change_line_ten(&repo);
        let primary = repo.reviews();
        let own = primary
            .start(TargetSpec::Uncommitted, None, None, 2)
            .unwrap()
            .review
            .id;
        let listed = repo.git(&["for-each-ref", "--format=%(refname)", "refs/dots/"]);
        assert_eq!(
            listed,
            format!("refs/dots/reviews/{own}"),
            "the removed worktree's pin is released"
        );
    }

    #[test]
    fn parallel_reviewers_never_lose_threads() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let id = repo
            .reviews()
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    let reviews = Reviews::open(repo.path()).unwrap();
                    for _ in 0..3 {
                        reviews.add(&id, vec![finding(None)], None, 2).unwrap();
                    }
                });
            }
        });
        let threads = repo
            .reviews()
            .show(&id, ShowOptions::default())
            .unwrap()
            .review
            .threads;
        let mut ids: Vec<_> = threads.iter().map(|thread| thread.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 12);
    }

    #[test]
    fn reviews_live_in_the_worktree_git_directory_and_other_schemas_are_refused() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let file = repo
            .root
            .join(".git/dots/reviews")
            .join(format!("{id}.json"));
        assert!(file.is_file());
        assert_eq!(reviews.directory(), file.parent().unwrap());

        // Files written before reviews recorded an owner load without one.
        let mut written: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        written.as_object_mut().unwrap().remove("owner");
        fs::write(&file, serde_json::to_vec(&written).unwrap()).unwrap();
        assert_eq!(
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .review
                .owner,
            None
        );

        fs::write(
            &file,
            format!("{{\"schema\": 1, \"id\": \"{id}\", \"findings\": []}}"),
        )
        .unwrap();
        let Err(error @ Error::Corrupt { .. }) = reviews.show(&id, ShowOptions::default()) else {
            panic!("a schema 1 file was read");
        };
        let message = error.to_string();
        assert!(
            message.contains(&file.display().to_string()) && message.contains("schema 1"),
            "{message}"
        );
        assert!(matches!(
            reviews.list(true, false),
            Err(Error::Corrupt { .. })
        ));
        fs::write(&file, format!("{{\"schema\": {SCHEMA}}}")).unwrap();
        assert!(matches!(
            reviews.show(&id, ShowOptions::default()),
            Err(Error::Corrupt { .. })
        ));

        reviews.remove(&id).unwrap();
        assert!(!file.exists());
        assert!(matches!(reviews.remove(&id), Err(Error::NotFound(_))));
    }

    fn user(anchor: Option<(&str, Side, u32, u32)>) -> NewThread {
        NewThread {
            author: USER.into(),
            ..comment(anchor)
        }
    }

    fn draft(thread: Option<&str>, anchor: Option<Anchor>, body: &str) -> NewDraft {
        NewDraft {
            thread: thread.map(Into::into),
            anchor,
            body: body.into(),
        }
    }

    fn drafts() -> ShowOptions {
        ShowOptions {
            drafts: true,
            preset: None,
        }
    }

    #[test]
    fn only_the_user_settles_the_threads_the_user_started() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        reviews
            .add(&id, vec![user(None), finding(None)], None, 2)
            .unwrap();
        assert_eq!(AuthorKind::of(USER), AuthorKind::User);
        assert_eq!(AuthorKind::of(REFRESH_AUTHOR), AuthorKind::Refresh);
        assert_eq!(AuthorKind::of("claude-opus"), AuthorKind::Agent);

        let refused = reviews.set(&id, "t1", Status::Fixed, "agent".into(), None, 3);
        let Err(error @ Error::UserDecides { .. }) = refused else {
            panic!("an agent settled the user's thread: {refused:?}");
        };
        assert!(error.to_string().contains("only the user"), "{error}");
        let replied = reviews
            .reply(
                &id,
                "t1",
                "agent".into(),
                "Fixed in the next revision.".into(),
                4,
            )
            .unwrap();
        assert_eq!(replied.status, Status::Open, "a reply leaves the status");
        let settled = reviews
            .set(&id, "t1", Status::Fixed, USER.into(), None, 5)
            .unwrap();
        assert_eq!(settled.status, Status::Fixed);
        reviews
            .set(&id, "t2", Status::Dismissed, "verifier".into(), None, 6)
            .expect("agents settle agent threads");

        for result in [
            reviews.reply(&id, "t2", REFRESH_AUTHOR.into(), "Spoofed.".into(), 7),
            reviews.set(&id, "t2", Status::Open, REFRESH_AUTHOR.into(), None, 7),
        ] {
            assert!(matches!(result, Err(Error::Invalid(_))), "{result:?}");
        }
        let mut spoofed = comment(None);
        spoofed.author = REFRESH_AUTHOR.into();
        assert!(matches!(
            reviews.add(&id, vec![spoofed], None, 7),
            Err(Error::Rejected(_))
        ));
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "One draft lifecycle: private, refused, discarded, submitted, summary-only"
    )]
    fn drafts_stay_private_until_the_user_submits_them_at_once() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        reviews.add(&id, vec![finding(None)], None, 2).unwrap();
        let at_ten = Anchor::lines("src/lib.rs", Side::New, 10, 10);
        let everything = Preset::Everything;
        let d1 = reviews
            .draft(
                &id,
                draft(None, Some(at_ten.clone()), "Why here?"),
                everything,
                Some(0),
                3,
            )
            .unwrap();
        let d2 = reviews
            .draft(&id, draft(Some("t1"), None, "Agreed."), everything, None, 4)
            .unwrap();
        let d3 = reviews
            .draft(&id, draft(None, None, "Overall fine."), everything, None, 5)
            .unwrap();
        assert_eq!(
            [d1.id.as_str(), d2.id.as_str(), d3.id.as_str()],
            ["d1", "d2", "d3"]
        );
        assert_eq!(d1.anchor, Some(at_ten.clone()));

        let hidden = reviews.show(&id, ShowOptions::default()).unwrap();
        assert_eq!(hidden.review.drafts, []);
        assert_eq!(hidden.review.threads.len(), 1);
        assert_eq!(
            hidden.review.updated_at, 2,
            "drafts are not visible changes"
        );
        let json = serde_json::to_value(&hidden).unwrap();
        assert!(json["review"].get("drafts").is_none(), "{json}");
        assert_eq!(reviews.show(&id, drafts()).unwrap().review.drafts.len(), 3);
        assert_eq!(reviews.list(false, false).unwrap()[0].drafts, None);
        assert_eq!(reviews.list(false, true).unwrap()[0].drafts, Some(3));

        // Refused drafts record nothing.
        for refused in [
            reviews.draft(&id, draft(Some("t9"), None, "x"), everything, None, 6),
            reviews.draft(
                &id,
                draft(Some("t1"), Some(at_ten.clone()), "x"),
                everything,
                None,
                6,
            ),
            reviews.draft(&id, draft(None, None, " "), everything, None, 6),
            reviews.draft(
                &id,
                draft(
                    None,
                    Some(Anchor::lines("src/lib.rs", Side::New, 1, 1)),
                    "x",
                ),
                everything,
                None,
                6,
            ),
            reviews.draft(&id, draft(None, None, "x"), everything, Some(1), 6),
        ] {
            assert!(refused.is_err(), "{refused:?}");
        }
        assert!(matches!(
            reviews.discard(&id, Some("d9")),
            Err(Error::NoDraft { .. })
        ));
        assert_eq!(reviews.discard(&id, Some("d3")).unwrap()[0].id, "d3");
        assert_eq!(reviews.show(&id, drafts()).unwrap().review.drafts.len(), 2);

        let submitted = reviews.submit(&id, Some("Two notes.".into()), 7).unwrap();
        assert_eq!(
            submitted.submission,
            Submission {
                at: 7,
                body: Some("Two notes.".into()),
                comments: vec![
                    Published {
                        thread: "t2".into(),
                        comment: 0
                    },
                    Published {
                        thread: "t1".into(),
                        comment: 1
                    },
                ],
            }
        );
        let ids: Vec<_> = submitted
            .threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect();
        assert_eq!(ids, ["t2", "t1"]);
        let review = reviews.show(&id, drafts()).unwrap().review;
        assert_eq!(review.drafts, []);
        assert_eq!(review.submissions, [submitted.submission]);
        assert_eq!(review.updated_at, 7);
        let opened = &review.threads[1];
        assert_eq!(
            (opened.id.as_str(), opened.anchor.as_ref(), opened.status),
            ("t2", Some(&at_ten), Status::Open)
        );
        assert_eq!(opened.comments, [entry(USER, 7, Some("Why here?"), None)]);
        assert!(opened.started_by_user());
        assert_eq!(
            review.threads[0].comments.last(),
            Some(&entry(USER, 7, Some("Agreed."), None))
        );

        assert!(
            matches!(reviews.submit(&id, None, 8), Err(Error::Invalid(_))),
            "nothing to submit"
        );
        let summary = reviews.submit(&id, Some("Looks good.".into()), 8).unwrap();
        assert!(summary.submission.comments.is_empty() && summary.threads.is_empty());
        reviews
            .draft(&id, draft(None, None, "a"), everything, None, 9)
            .unwrap();
        reviews
            .draft(&id, draft(None, None, "b"), everything, None, 9)
            .unwrap();
        assert_eq!(reviews.discard(&id, None).unwrap().len(), 2);
        assert_eq!(reviews.show(&id, drafts()).unwrap().review.drafts, []);
    }

    #[test]
    fn a_submission_with_a_draft_whose_lines_changed_publishes_nothing() {
        let repo = Repo::new();
        let mut lines: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        lines[4] = "line five, changed".into();
        lines[14] = "line fifteen, changed".into();
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        reviews.add(&id, vec![finding(None)], None, 2).unwrap();
        let everything = Preset::Everything;
        let at = |line| Some(Anchor::lines("src/lib.rs", Side::New, line, line));
        reviews
            .draft(&id, draft(None, at(5), "five"), everything, None, 3)
            .unwrap();
        reviews
            .draft(&id, draft(None, at(15), "fifteen"), everything, None, 3)
            .unwrap();
        reviews
            .draft(&id, draft(Some("t1"), None, "reply"), everything, None, 3)
            .unwrap();

        lines[4] = "line five, fixed".into();
        lines.insert(0, "header".into());
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        let refreshed = reviews.refresh(&id, 4).unwrap();
        assert!(
            refreshed.review.drafts.is_empty(),
            "refresh output hides drafts"
        );
        let held = reviews.show(&id, drafts()).unwrap().review.drafts;
        assert_eq!(
            held.iter()
                .map(|draft| (draft.revision, draft.anchor.clone()))
                .collect::<Vec<_>>(),
            [(0, at(5)), (1, at(16)), (0, None)],
            "unchanged lines carry; changed ones stay behind"
        );

        let Err(Error::Rejected(rejections)) = reviews.submit(&id, Some("Batch".into()), 5) else {
            panic!("a stale draft was submitted");
        };
        assert_eq!(
            rejections
                .iter()
                .map(|rejection| (rejection.index, rejection.field))
                .collect::<Vec<_>>(),
            [(0, "anchor")]
        );
        assert!(
            rejections[0].message.contains("d1"),
            "{}",
            rejections[0].message
        );
        let review = reviews.show(&id, drafts()).unwrap().review;
        assert_eq!(
            (
                review.threads.len(),
                review.drafts.len(),
                review.submissions.len()
            ),
            (1, 3, 0),
            "nothing was published"
        );
        assert_eq!(review.threads[0].comments.len(), 1);

        reviews.discard(&id, Some("d1")).unwrap();
        let submitted = reviews.submit(&id, None, 6).unwrap();
        assert_eq!(submitted.submission.comments.len(), 2);
        let review = reviews.show(&id, ShowOptions::default()).unwrap().review;
        assert_eq!(
            (review.threads[1].revision, review.threads[1].anchor.clone()),
            (1, at(16))
        );
    }

    #[test]
    fn whole_file_threads_anchor_to_changed_paths_and_carry_while_they_change() {
        let repo = Repo::new();
        change_line_ten(&repo);
        repo.write("notes.md", "new\n");
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let file = |path: &str| NewThread {
            anchor: Some(Anchor::file(path)),
            ..comment(None)
        };
        let Err(Error::Rejected(rejections)) =
            reviews.add(&id, vec![file("AGENTS.md"), file("/abs")], None, 2)
        else {
            panic!("an unchanged path took a whole-file thread");
        };
        assert_eq!(
            rejections
                .iter()
                .map(|rejection| (rejection.index, rejection.field))
                .collect::<Vec<_>>(),
            [(0, "anchor"), (1, "anchor")]
        );
        reviews
            .add(&id, vec![file("src/lib.rs"), file("notes.md")], None, 2)
            .unwrap();

        assert_eq!(
            serde_json::to_value(Anchor::file("a.rs")).unwrap(),
            serde_json::json!({"path": "a.rs"})
        );
        assert_eq!(
            serde_json::to_value(Anchor::lines("a.rs", Side::Old, 2, 3)).unwrap(),
            serde_json::json!({"path": "a.rs", "side": "old", "start": 2, "end": 3})
        );
        let partial =
            serde_json::from_value::<Anchor>(serde_json::json!({"path": "a.rs", "start": 2}));
        assert!(
            partial
                .as_ref()
                .is_err_and(|error| error.to_string().contains("whole-file")),
            "{partial:?}"
        );

        // notes.md stays changed; src/lib.rs is reverted, so it is not.
        repo.write("notes.md", "newer\n");
        repo.git(&["checkout", "--", "src/lib.rs"]);
        let refreshed = reviews.refresh(&id, 3).unwrap();
        assert_eq!(refreshed.carried, ["t2"]);
        assert_eq!(refreshed.outdated, ["t1"]);
    }

    #[test]
    fn reviewed_marks_hold_exactly_while_the_files_change_is_the_one_marked() {
        let repo = Repo::new();
        change_line_ten(&repo);
        repo.write("notes.md", "new\n");
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let state = || -> Vec<(String, bool)> {
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .files
                .into_iter()
                .map(|file| (file.change.path, file.reviewed))
                .collect()
        };
        let marked = reviews.mark(&id, "src/lib.rs", true, None).unwrap();
        assert!(marked.reviewed && marked.change.path == "src/lib.rs");
        assert_eq!(marked.change.status, FileStatus::Modified);
        assert_eq!(
            state(),
            [("notes.md".into(), false), ("src/lib.rs".into(), true)]
        );
        assert!(matches!(
            reviews.mark(&id, "AGENTS.md", true, None),
            Err(Error::Invalid(_))
        ));

        // Another file's change leaves the mark; this file's change lapses it.
        repo.write("notes.md", "newer\n");
        reviews.refresh(&id, 2).unwrap();
        assert_eq!(state()[1], ("src/lib.rs".into(), true));
        let mut lines: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        lines[9] = "line ten, changed again".into();
        repo.write("src/lib.rs", &(lines.join("\n") + "\n"));
        reviews.refresh(&id, 3).unwrap();
        assert_eq!(state()[1], ("src/lib.rs".into(), false));
        // The very change it covered, back again, is reviewed again.
        change_line_ten(&repo);
        reviews.refresh(&id, 4).unwrap();
        assert_eq!(state()[1], ("src/lib.rs".into(), true));

        assert!(
            !reviews
                .mark(&id, "src/lib.rs", false, None)
                .unwrap()
                .reviewed
        );
        assert_eq!(state()[1], ("src/lib.rs".into(), false));
    }

    #[test]
    fn changed_since_last_looked_compares_each_files_change() {
        let repo = Repo::new();
        change_line_ten(&repo);
        repo.write("notes.md", "new\n");
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let changed = || -> Vec<(String, bool)> {
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .files
                .into_iter()
                .map(|file| (file.change.path, file.changed_since_looked))
                .collect()
        };
        assert_eq!(
            changed(),
            [("notes.md".into(), false), ("src/lib.rs".into(), false)],
            "never looked"
        );
        assert_eq!(
            reviews.look(&id, None, 2).unwrap(),
            Looked { revision: 0, at: 2 }
        );
        repo.write("notes.md", "newer\n");
        repo.write("added.txt", "added\n");
        reviews.refresh(&id, 3).unwrap();
        assert_eq!(
            changed(),
            [
                ("added.txt".into(), true),
                ("notes.md".into(), true),
                ("src/lib.rs".into(), false)
            ]
        );
        let review = reviews.show(&id, ShowOptions::default()).unwrap().review;
        assert_eq!(review.updated_at, 3, "looking is not a visible change");
        reviews.look(&id, None, 4).unwrap();
        assert!(changed().iter().all(|(_, changed)| !changed));
    }

    /// A review of branch `feature` against `main`: two commits' worth of
    /// committed change and uncommitted work on top, so every preset differs.
    /// Returns the base commit and the HEAD commit.
    fn layered(repo: &Repo) -> (String, String) {
        let base = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "-q", "-c", "feature"]);
        // HEAD: two header lines, then line five changed (HEAD line 7).
        let mut committed: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        committed[4] = "line five, committed".into();
        committed.splice(0..0, ["header 1".to_owned(), "header 2".to_owned()]);
        repo.write("src/lib.rs", &(committed.join("\n") + "\n"));
        repo.write("notes.md", "committed notes\n");
        let head = repo.commit("committed work");
        // Working tree: a line inserted after HEAD line 10, and line fifteen
        // (HEAD line 17, now 18) changed.
        let mut working = committed;
        working[16] = "line fifteen, uncommitted".into();
        working.insert(10, "inserted".into());
        repo.write("src/lib.rs", &(working.join("\n") + "\n"));
        (base, head)
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "One layered revision placed in each of the three presets"
    )]
    fn presets_place_every_anchor_in_their_own_line_numbers() {
        let repo = Repo::new();
        let (base, head) = layered(&repo);
        let reviews = repo.reviews();
        let overview = reviews
            .start(
                TargetSpec::Base {
                    reference: "main".into(),
                },
                None,
                None,
                1,
            )
            .unwrap();
        let id = overview.review.id.clone();
        let snapshot = overview.review.current().snapshot.clone();
        let new = |line| Anchor::lines("src/lib.rs", Side::New, line, line);
        let old = |line| Anchor::lines("src/lib.rs", Side::Old, line, line);
        let threads = vec![
            new(18),                    // t1: uncommitted change
            new(7),                     // t2: committed change
            old(5),                     // t3: base line the commit replaced
            new(13),                    // t4: context shifted by the insertion
            old(3),                     // t5: base line the headers shifted
            Anchor::file("src/lib.rs"), // t6
            Anchor::file("notes.md"),   // t7: changed only by the commit
        ];
        reviews
            .add(
                &id,
                threads
                    .into_iter()
                    .map(|anchor| NewThread {
                        anchor: Some(anchor),
                        ..comment(None)
                    })
                    .collect(),
                None,
                2,
            )
            .unwrap();
        reviews
            .draft(
                &id,
                draft(None, Some(new(13)), "draft"),
                Preset::Everything,
                None,
                3,
            )
            .unwrap();
        let view = |preset| {
            reviews
                .show(
                    &id,
                    ShowOptions {
                        drafts: true,
                        preset: Some(preset),
                    },
                )
                .unwrap()
                .view
                .unwrap()
        };
        let placed = |view: &View| -> Vec<Option<Anchor>> {
            ["t1", "t2", "t3", "t4", "t5", "t6", "t7", "d1"]
                .into_iter()
                .map(|id| view.anchors[id].clone())
                .collect()
        };
        let paths = |view: &View| -> Vec<String> {
            view.files.iter().map(|file| file.path.clone()).collect()
        };
        let file = |path| Some(Anchor::file(path));

        let everything = view(Preset::Everything);
        assert_eq!(
            everything.available,
            [Preset::Everything, Preset::Uncommitted, Preset::Committed]
        );
        assert_eq!(
            (everything.base.as_str(), everything.head.as_str()),
            (base.as_str(), snapshot.as_str())
        );
        assert_eq!(paths(&everything), ["notes.md", "src/lib.rs"]);
        assert_eq!(
            placed(&everything),
            [
                Some(new(18)),
                Some(new(7)),
                Some(old(5)),
                Some(new(13)),
                Some(old(3)),
                file("src/lib.rs"),
                file("notes.md"),
                Some(new(13)),
            ]
        );

        let uncommitted = view(Preset::Uncommitted);
        assert_eq!(
            (uncommitted.base.as_str(), uncommitted.head.as_str()),
            (head.as_str(), snapshot.as_str())
        );
        assert_eq!(paths(&uncommitted), ["src/lib.rs"]);
        assert_eq!(
            placed(&uncommitted),
            [
                Some(new(18)),
                Some(new(7)),
                None,
                Some(new(13)),
                Some(old(5)),
                file("src/lib.rs"),
                None,
                Some(new(13)),
            ]
        );

        let committed = view(Preset::Committed);
        assert_eq!(
            (committed.base.as_str(), committed.head.as_str()),
            (base.as_str(), head.as_str())
        );
        assert_eq!(paths(&committed), ["notes.md", "src/lib.rs"]);
        assert_eq!(
            placed(&committed),
            [
                None,
                Some(new(7)),
                Some(old(5)),
                Some(new(12)),
                Some(old(3)),
                file("src/lib.rs"),
                file("notes.md"),
                Some(new(12)),
            ]
        );
        assert!(
            reviews
                .show(&id, ShowOptions::default())
                .unwrap()
                .view
                .is_none(),
            "no view unless asked"
        );
    }

    #[test]
    fn comments_from_a_preset_map_back_or_are_refused() {
        let repo = Repo::new();
        layered(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(
                TargetSpec::Base {
                    reference: "main".into(),
                },
                None,
                None,
                1,
            )
            .unwrap()
            .review
            .id;
        let new = |line| Anchor::lines("src/lib.rs", Side::New, line, line);
        let old = |line| Anchor::lines("src/lib.rs", Side::Old, line, line);
        let at = |anchor: Anchor, preset| {
            reviews
                .draft(&id, draft(None, Some(anchor), "here"), preset, None, 2)
                .map(|draft| draft.anchor.unwrap())
        };
        assert_eq!(at(new(12), Preset::Committed).unwrap(), new(13));
        assert_eq!(at(old(5), Preset::Uncommitted).unwrap(), old(3));
        assert_eq!(at(new(18), Preset::Uncommitted).unwrap(), new(18));
        assert_eq!(
            at(Anchor::file("notes.md"), Preset::Committed).unwrap(),
            Anchor::file("notes.md")
        );
        for (anchor, preset, why) in [
            (new(17), Preset::Committed, "differs from the working tree"),
            (old(7), Preset::Uncommitted, "differs from the base"),
            (
                Anchor::file("notes.md"),
                Preset::Uncommitted,
                "not changed in the uncommitted view",
            ),
        ] {
            let refused = at(anchor, preset);
            assert!(
                matches!(&refused, Err(Error::Invalid(message)) if message.contains(why)),
                "{refused:?}"
            );
        }
        assert_eq!(
            reviews.show(&id, drafts()).unwrap().review.drafts.len(),
            4,
            "refused comments record nothing"
        );
    }

    #[test]
    fn presets_need_changes_of_their_own() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let worktree = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        let view = |id: &str, preset| {
            reviews.show(
                id,
                ShowOptions {
                    drafts: false,
                    preset: Some(preset),
                },
            )
        };
        assert_eq!(
            view(&worktree, Preset::Everything)
                .unwrap()
                .view
                .unwrap()
                .available,
            [Preset::Everything, Preset::Uncommitted],
            "a worktree review has nothing committed"
        );
        assert!(matches!(
            view(&worktree, Preset::Committed),
            Err(Error::Invalid(_))
        ));

        let feature = repo.commit("ten");
        let committed = reviews
            .start(
                TargetSpec::Base {
                    reference: format!("{feature}~1"),
                },
                None,
                None,
                2,
            )
            .unwrap()
            .review
            .id;
        let Err(Error::Invalid(message)) = view(&committed, Preset::Uncommitted) else {
            panic!("a clean working tree has no uncommitted view");
        };
        assert!(
            message.contains("available: everything, committed"),
            "{message}"
        );
    }

    #[test]
    fn refresh_moves_to_a_new_head_commit_even_when_the_trees_are_unchanged() {
        let repo = Repo::new();
        let (_, _) = layered(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(
                TargetSpec::Base {
                    reference: "main".into(),
                },
                None,
                None,
                1,
            )
            .unwrap()
            .review
            .id;
        let available = || {
            reviews
                .show(
                    &id,
                    ShowOptions {
                        drafts: false,
                        preset: Some(Preset::Everything),
                    },
                )
                .unwrap()
                .view
                .unwrap()
                .available
        };
        assert_eq!(
            available(),
            [Preset::Everything, Preset::Uncommitted, Preset::Committed]
        );
        // Commit the captured bytes as they are: same base, same head tree.
        let commit = repo.commit("the rest");
        let refreshed = reviews.refresh(&id, 2).unwrap();
        assert!(refreshed.changed, "a new HEAD commit is a new revision");
        assert_eq!(refreshed.review.revisions.len(), 2);
        assert_eq!(refreshed.review.current().head_commit, commit);
        assert_eq!(available(), [Preset::Everything, Preset::Committed]);
        assert!(!reviews.refresh(&id, 3).unwrap().changed);
    }

    #[test]
    fn marks_refuse_a_revision_the_review_has_moved_past() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        repo.write("src/lib.rs", "moved on\n");
        reviews.refresh(&id, 2).unwrap();
        assert!(matches!(
            reviews.mark(&id, "src/lib.rs", true, Some(0)),
            Err(Error::Moved {
                expected: 0,
                current: 1,
                ..
            })
        ));
        let files = reviews.show(&id, ShowOptions::default()).unwrap().files;
        assert!(!files[0].reviewed, "nothing was marked");
        assert!(
            reviews
                .mark(&id, "src/lib.rs", true, Some(1))
                .unwrap()
                .reviewed
        );
        assert!(reviews.mark(&id, "src/lib.rs", false, None).is_ok());
    }

    #[test]
    fn looking_records_the_revision_seen_and_never_moves_back() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Uncommitted, None, None, 1)
            .unwrap()
            .review
            .id;
        repo.write("notes.md", "unseen\n");
        reviews.refresh(&id, 2).unwrap();
        // The user saw revision 0 only: the refresh's change stays unseen.
        assert_eq!(
            reviews.look(&id, Some(0), 3).unwrap(),
            Looked { revision: 0, at: 3 }
        );
        let files = reviews.show(&id, ShowOptions::default()).unwrap().files;
        let notes = files
            .iter()
            .find(|file| file.change.path == "notes.md")
            .unwrap();
        assert!(notes.changed_since_looked);
        assert_eq!(
            reviews.look(&id, Some(1), 4).unwrap(),
            Looked { revision: 1, at: 4 }
        );
        assert_eq!(
            reviews.look(&id, Some(0), 5).unwrap(),
            Looked { revision: 1, at: 4 },
            "an older view closing later keeps the newer look"
        );
        assert!(matches!(
            reviews.look(&id, Some(2), 6),
            Err(Error::Invalid(_))
        ));
    }
    /// Open the worktree review of the checkout at `directory`.
    fn open_at(directory: &Path, base: BaseChoice, owner: Option<&str>, now: i64) -> Overview {
        Reviews::open(directory)
            .unwrap()
            .open_worktree(base, owner.map(Into::into), ShowOptions::default(), now)
            .unwrap()
    }

    impl Repo {
        /// Make `commit` the default branch `branch` of remote `remote`, as a
        /// fetch from a clone would leave it.
        fn remote_default(&self, remote: &str, branch: &str, commit: &str) {
            let tracking = format!("refs/remotes/{remote}/{branch}");
            self.git(&["update-ref", &tracking, commit]);
            self.git(&[
                "symbolic-ref",
                &format!("refs/remotes/{remote}/HEAD"),
                &tracking,
            ]);
        }

        fn open(&self, base: BaseChoice, owner: Option<&str>, now: i64) -> Overview {
            open_at(&self.root, base, owner, now)
        }
    }

    fn candidate(reference: &str, commits: u64) -> Candidate {
        Candidate {
            reference: reference.into(),
            commits,
        }
    }

    #[test]
    fn the_worktree_review_compares_with_the_nearest_default_branch() {
        let repo = Repo::new();
        let stale = repo.git(&["rev-parse", "HEAD"]);
        repo.write("upstream.txt", "landed upstream\n");
        let landed = repo.commit("landed upstream");
        // Local main is behind the remote's default.
        repo.git(&["reset", "-q", "--hard", &stale]);
        repo.remote_default("origin", "main", &landed);
        repo.git(&["switch", "-q", "--no-track", "-c", "feature", &landed]);
        change_line_ten(&repo);
        repo.commit("feature");
        repo.write("notes.md", "uncommitted\n");

        let overview = repo.open(BaseChoice::Keep, None, 1);
        let revision = overview.review.current();
        assert_eq!(revision.base, landed);
        assert_eq!(
            revision.basis,
            Basis {
                reference: "origin/main".into(),
                reason: BaseReason::Default,
                candidates: vec![candidate("origin/main", 1), candidate("main", 2)],
            }
        );
        let paths: Vec<_> = overview
            .files
            .iter()
            .map(|file| file.change.path.as_str())
            .collect();
        assert_eq!(paths, ["notes.md", "src/lib.rs"]);

        // A local main ahead of the remote, unpushed, is nearer for the
        // branches built on it; at equal distance a remote wins.
        repo.git(&["update-ref", "refs/remotes/origin/main", &stale]);
        repo.git(&["branch", "-f", "main", &landed]);
        let revision = repo
            .open(BaseChoice::Keep, None, 2)
            .review
            .current()
            .clone();
        assert_eq!(revision.base, landed);
        assert_eq!(
            revision.basis.candidates,
            [candidate("main", 1), candidate("origin/main", 2)]
        );
        repo.git(&["update-ref", "refs/remotes/origin/main", &landed]);
        repo.remote_default("upstream", "main", &landed);
        let basis = repo
            .open(BaseChoice::Keep, None, 3)
            .review
            .current()
            .basis
            .clone();
        assert_eq!(basis.reference, "upstream/main");
        assert_eq!(
            basis.candidates,
            [
                candidate("upstream/main", 1),
                candidate("origin/main", 1),
                candidate("main", 1)
            ]
        );
    }

    #[test]
    fn without_a_remote_the_local_default_branch_is_the_base() {
        let repo = Repo::new();
        let main = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "-q", "-c", "feature"]);
        change_line_ten(&repo);
        repo.commit("feature");
        let revision = repo
            .open(BaseChoice::Keep, None, 1)
            .review
            .current()
            .clone();
        assert_eq!(revision.base, main);
        assert_eq!(
            revision.basis,
            Basis {
                reference: "main".into(),
                reason: BaseReason::Default,
                candidates: vec![candidate("main", 1)],
            }
        );
    }

    #[test]
    fn a_branchs_own_upstream_is_ignored_and_another_branch_upstream_is_the_base() {
        let repo = Repo::new();
        let main = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "-q", "-c", "feature"]);
        change_line_ten(&repo);
        let feature = repo.commit("feature");
        // Pushed: the upstream is the same branch on the remote.
        repo.git(&["update-ref", "refs/remotes/origin/feature", &feature]);
        repo.git(&["config", "branch.feature.remote", "origin"]);
        repo.git(&["config", "branch.feature.merge", "refs/heads/feature"]);
        let overview = repo.open(BaseChoice::Keep, None, 1);
        assert_eq!(overview.review.current().base, main);
        assert_eq!(overview.review.current().basis.reason, BaseReason::Default);

        // A stacked branch tracks the branch it builds on.
        let child = repo.path().with_extension("child");
        let _ = fs::remove_dir_all(&child);
        repo.git(&[
            "worktree",
            "add",
            "-q",
            "--track",
            "-b",
            "child",
            child.to_str().unwrap(),
            "feature",
        ]);
        fs::write(child.join("child.txt"), "stacked\n").unwrap();
        let revision = open_at(&child, BaseChoice::Keep, None, 2)
            .review
            .current()
            .clone();
        fs::remove_dir_all(&child).unwrap();
        assert_eq!(revision.base, feature);
        assert_eq!(
            revision.basis,
            Basis {
                reference: "feature".into(),
                reason: BaseReason::Upstream,
                candidates: Vec::new(),
            }
        );
    }

    #[test]
    fn an_explicit_base_is_kept_across_refreshes_until_dropped() {
        let repo = Repo::new();
        let main = repo.git(&["rev-parse", "HEAD"]);
        repo.git(&["switch", "-q", "-c", "feature"]);
        change_line_ten(&repo);
        let feature = repo.commit("feature");
        repo.git(&["switch", "-q", "--no-track", "-c", "child"]);
        repo.write("child.txt", "stacked\n");

        let opened = repo.open(BaseChoice::Reference("feature".into()), None, 1);
        let id = opened.review.id.clone();
        assert_eq!(
            opened.review.target,
            TargetSpec::Worktree {
                base: Some("feature".into())
            }
        );
        assert_eq!(opened.review.current().base, feature);
        assert_eq!(opened.review.current().basis.reason, BaseReason::Override);
        assert!(matches!(
            repo.reviews().open_worktree(
                BaseChoice::Reference("missing".into()),
                None,
                ShowOptions::default(),
                2
            ),
            Err(Error::Invalid(_))
        ));

        repo.commit("child");
        let kept = repo.open(BaseChoice::Keep, None, 3);
        assert_eq!(kept.review.id, id);
        assert_eq!(kept.review.revisions.len(), 2);
        assert_eq!(kept.review.current().base, feature);
        assert_eq!(kept.review.current().basis.reason, BaseReason::Override);
        let refreshed = repo.reviews().refresh(&id, 4).unwrap();
        assert_eq!(
            refreshed.review.target,
            TargetSpec::Worktree {
                base: Some("feature".into())
            }
        );

        let dropped = repo.open(BaseChoice::Default, None, 5);
        assert_eq!(dropped.review.target, TargetSpec::Worktree { base: None });
        assert_eq!(dropped.review.current().base, main);
        assert_eq!(dropped.review.current().basis.reason, BaseReason::Default);
    }

    #[test]
    fn on_the_default_branch_the_review_covers_uncommitted_changes() {
        let repo = Repo::new();
        let pushed = repo.git(&["rev-parse", "HEAD"]);
        repo.remote_default("origin", "main", &pushed);
        repo.write("unpushed.txt", "committed, not pushed\n");
        let head = repo.commit("unpushed");
        assert!(matches!(
            repo.reviews()
                .open_worktree(BaseChoice::Keep, None, ShowOptions::default(), 1),
            Err(Error::NothingToReview(_))
        ));
        change_line_ten(&repo);
        let overview = repo.open(BaseChoice::Keep, None, 2);
        let revision = overview.review.current();
        assert_eq!(revision.base, head);
        assert_eq!(
            revision.basis,
            Basis {
                reference: "HEAD".into(),
                reason: BaseReason::DefaultBranch,
                candidates: Vec::new(),
            }
        );
        assert_eq!(overview.files.len(), 1);
        assert_eq!(overview.files[0].change.path, "src/lib.rs");
    }

    #[test]
    fn opening_creates_the_worktree_review_once_and_reuses_it() {
        let repo = Repo::new();
        repo.git(&["switch", "-q", "-c", "feature"]);
        let reviews = repo.reviews();
        let Err(Error::NothingToReview(message)) =
            reviews.open_worktree(BaseChoice::Keep, None, ShowOptions::default(), 1)
        else {
            panic!("a clean worktree has nothing to review");
        };
        assert!(
            message.contains("main (nearest default branch"),
            "{message}"
        );
        assert!(reviews.list(false, false).unwrap().is_empty());

        change_line_ten(&repo);
        let created = repo.open(BaseChoice::Keep, Some("pi-first"), 2);
        let opened = created.opened.as_ref().unwrap();
        assert!(opened.created);
        assert_eq!(created.review.owner.as_deref(), Some("pi-first"));
        assert_eq!(created.review.target, TargetSpec::Worktree { base: None });

        // Another review of this worktree is not its own review.
        let other = reviews
            .start(TargetSpec::Uncommitted, None, None, 3)
            .unwrap()
            .review
            .id;
        let reused = repo.open(BaseChoice::Keep, None, 4);
        assert_eq!(reused.review.id, created.review.id);
        let opened = reused.opened.as_ref().unwrap();
        assert!(!opened.created && !opened.changed);
        assert_eq!(reused.review.revisions.len(), 1);
        assert_eq!(
            reused.review.owner.as_deref(),
            Some("pi-first"),
            "the user opening it keeps its owner"
        );
        let taken = repo.open(BaseChoice::Keep, Some("pi-second"), 5);
        assert_eq!(taken.review.owner.as_deref(), Some("pi-second"));
        assert_eq!(taken.review.id, created.review.id);
        assert_eq!(
            reviews.latest().unwrap(),
            created.review.id,
            "taking ownership is a change"
        );
        let mut ids: Vec<_> = reviews
            .list(false, false)
            .unwrap()
            .into_iter()
            .map(|summary| summary.id)
            .collect();
        ids.sort();
        let mut want = vec![created.review.id.clone(), other];
        want.sort();
        assert_eq!(ids, want);
    }

    #[test]
    fn the_worktree_review_follows_new_commits_and_carries_threads() {
        let repo = Repo::new();
        repo.git(&["switch", "-q", "-c", "feature"]);
        change_line_ten(&repo);
        let first = repo.open(BaseChoice::Keep, None, 1);
        let id = first.review.id.clone();
        let reviews = repo.reviews();
        reviews
            .add(
                &id,
                vec![finding(Some(("src/lib.rs", Side::New, 10, 10)))],
                Some(0),
                2,
            )
            .unwrap();

        let fixed = repo.commit("feature");
        repo.write("fix.txt", "the fix\n");
        let fix = repo.commit("fix");
        let moved = repo.open(BaseChoice::Keep, None, 3);
        assert_eq!(moved.review.id, id);
        let opened = moved.opened.as_ref().unwrap();
        assert!(opened.changed);
        assert_eq!(opened.carried, ["t1"]);
        assert_eq!(opened.outdated, Vec::<String>::new());
        let delta = opened.delta.as_ref().unwrap();
        let delta_paths: Vec<_> = delta.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(delta_paths, ["fix.txt"]);
        let revision = moved.review.current();
        assert_eq!(revision.head_commit, fix);
        assert_ne!(revision.head_commit, fixed);
        assert_eq!(moved.review.threads[0].revision, 1);
        assert_eq!(moved.review.threads[0].status, Status::Open);
        let paths: Vec<_> = moved
            .files
            .iter()
            .map(|file| file.change.path.as_str())
            .collect();
        assert_eq!(paths, ["fix.txt", "src/lib.rs"]);
    }

    /// Two repositories with linked worktrees: repository one's has a review
    /// asking things of the user, repository two's only an untracked file.
    struct Fleet {
        one: Repo,
        two: Repo,
        id: String,
    }

    fn fleet() -> Fleet {
        let one = Repo::new();
        let linked = one.linked();
        let reviews = Reviews::open(&linked).unwrap();
        let mut lines: Vec<String> = (1..=20).map(|line| format!("line {line}")).collect();
        lines[2] = "line three, changed".into();
        lines[9] = "line ten, changed".into();
        fs::write(linked.join("src/lib.rs"), lines.join("\n") + "\n").unwrap();
        fs::write(linked.join("seen.md"), "seen\n").unwrap();
        let id = open_at(&linked, BaseChoice::Keep, Some("pi-agent"), 1)
            .review
            .id;
        // t1 and t2, an agent's findings; it dismisses t2.
        reviews
            .add(
                &id,
                vec![
                    finding(Some(("src/lib.rs", Side::New, 10, 10))),
                    finding(None),
                ],
                None,
                2,
            )
            .unwrap();
        // t3, the user's question an agent answers, and t4, unanswered.
        for body in ["Why this?", "And this?"] {
            reviews
                .draft(&id, draft(None, None, body), Preset::Everything, None, 3)
                .unwrap();
        }
        reviews.submit(&id, None, 4).unwrap();
        reviews
            .reply(&id, "t3", "agent".into(), "Because.".into(), 5)
            .unwrap();
        reviews
            .set(&id, "t2", Status::Dismissed, "agent".into(), None, 5)
            .unwrap();
        reviews
            .draft(
                &id,
                draft(Some("t3"), None, "Thanks"),
                Preset::Everything,
                None,
                6,
            )
            .unwrap();
        reviews.look(&id, None, 7).unwrap();
        // After the look: an edit, a new untracked file, and an untracked
        // file the user saw, unchanged.
        fs::write(
            linked.join("src/lib.rs"),
            numbered(1..=20).replace("line 3\n", "line 3 again\n"),
        )
        .unwrap();
        fs::write(linked.join("new.md"), "one\ntwo\n").unwrap();

        let two = Repo::new();
        let other = two.linked();
        fs::write(other.join("draft.txt"), "a\nb\nc").unwrap();
        Fleet { one, two, id }
    }

    #[test]
    fn the_inbox_summarises_worktrees_across_repositories() {
        let fleet = fleet();
        let one_linked = fleet.one.path().with_extension("linked");
        let two_linked = fleet.two.path().with_extension("linked");
        let missing = fleet.one.path().with_extension("missing");
        let inbox = dots_review::inbox(
            &[
                one_linked.clone(),
                fleet.one.path().join(".git"),
                fleet.two.path().to_owned(),
                missing.clone(),
            ],
            false,
        );
        assert_eq!(inbox.schema, SCHEMA);
        let paths: Vec<_> = inbox
            .worktrees
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        assert_eq!(
            paths,
            [one_linked.clone(), two_linked.clone()],
            "clean worktrees without reviews are left out; the one waiting on the user leads"
        );
        let skipped: Vec<_> = inbox
            .skipped
            .iter()
            .map(|skipped| skipped.path.clone())
            .collect();
        assert_eq!(skipped, [missing]);

        let first = &inbox.worktrees[0];
        assert_eq!(first.repository, fleet.one.path().join(".git"));
        assert_eq!(first.branch.as_deref(), Some("linked"));
        assert_eq!(first.basis.reference, "main");
        assert_eq!(first.base, fleet.one.git(&["rev-parse", "main"]));
        assert_eq!(first.diff.files, 3, "src/lib.rs, seen.md and new.md");
        assert_eq!((first.diff.added, first.diff.removed), (4, 1));
        let review = first.review.as_ref().unwrap();
        assert_eq!(review.id, fleet.id);
        assert_eq!(review.owner.as_deref(), Some("pi-agent"));
        assert_eq!(review.waiting, ["t1", "t3"]);
        assert_eq!(review.drafts, 1);
        assert_eq!(
            review.changed_since_looked.as_deref(),
            Some(&["new.md".to_owned(), "src/lib.rs".to_owned()][..])
        );
        assert_eq!(first.other_reviews, 0);

        let second = &inbox.worktrees[1];
        assert!(second.review.is_none());
        assert_eq!(
            (second.diff.files, second.diff.added, second.diff.removed),
            (1, 3, 0)
        );

        let everything = dots_review::inbox(
            &[fleet.one.path().to_owned(), fleet.two.path().to_owned()],
            true,
        );
        assert_eq!(everything.worktrees.len(), 4);
        assert!(everything.skipped.is_empty());
    }

    /// Every file under `roots` with its size and modification time, and the
    /// refs of each repository.
    fn state(roots: &[&Path]) -> Vec<String> {
        fn walk(path: &Path, out: &mut Vec<String>) {
            let metadata = fs::symlink_metadata(path).unwrap();
            out.push(format!(
                "{} {} {:?}",
                path.display(),
                metadata.len(),
                metadata.modified().unwrap()
            ));
            if metadata.is_dir() {
                let mut entries: Vec<_> = fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();
                entries.sort();
                for entry in entries {
                    walk(&entry, out);
                }
            }
        }
        let mut out = Vec::new();
        for root in roots {
            walk(root, &mut out);
        }
        out
    }

    #[test]
    fn the_inbox_writes_nothing() {
        let fleet = fleet();
        let one_linked = fleet.one.path().with_extension("linked");
        let two_linked = fleet.two.path().with_extension("linked");
        let roots = [fleet.one.path(), &one_linked, fleet.two.path(), &two_linked];
        let before = state(&roots);
        let refs = (
            fleet.one.git(&["for-each-ref"]),
            fleet.two.git(&["for-each-ref"]),
        );
        let inbox = dots_review::inbox(
            &[fleet.one.path().to_owned(), fleet.two.path().to_owned()],
            true,
        );
        assert_eq!(inbox.worktrees.len(), 4);
        assert_eq!(state(&roots), before);
        assert_eq!(
            (
                fleet.one.git(&["for-each-ref"]),
                fleet.two.git(&["for-each-ref"])
            ),
            refs
        );
    }
}
