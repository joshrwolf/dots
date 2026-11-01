//! Reviews of real temporary repositories.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    use dots_review::{
        Anchor, Comment, Error, NewThread, REFRESH_AUTHOR, Reviews, RuleCitation, Severity, Side,
        Status, TargetSpec,
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
            anchor: anchor.map(|(path, side, start, end)| Anchor {
                path: path.into(),
                side,
                start,
                end,
            }),
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
    fn worktree_review_pins_tracked_and_untracked_content_but_not_ignored_files() {
        let repo = Repo::new();
        change_line_ten(&repo);
        repo.write("notes/new.md", "new file\n");
        repo.write("ignored.log", "noise\n");
        let overview = repo
            .reviews()
            .start(TargetSpec::Worktree, Some("standard".into()), 100)
            .unwrap();

        let paths: Vec<_> = overview
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(paths, ["notes/new.md", "src/lib.rs"]);
        let revision = overview.review.current();
        assert_eq!(revision.base, repo.git(&["rev-parse", "HEAD"]));
        assert!(revision.includes_worktree);
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
            reviews.start(TargetSpec::Worktree, None, 1),
            Err(Error::NothingToReview(_))
        ));
        assert!(matches!(TargetSpec::range("main"), Err(Error::Invalid(_))));
        assert!(matches!(TargetSpec::range("a...b"), Err(Error::Invalid(_))));
        assert!(matches!(
            reviews.start(
                TargetSpec::Base {
                    reference: "missing".into()
                },
                None,
                1
            ),
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            reviews.start(TargetSpec::Worktree, Some("Not A Name".into()), 1),
            Err(Error::Invalid(_))
        ));
        assert!(
            reviews.list(true).unwrap().is_empty(),
            "refused starts leave nothing behind"
        );
    }

    #[test]
    fn base_and_range_targets_compare_exact_revisions() {
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
                1,
            )
            .unwrap();
        assert_eq!(base.review.current().base, initial);
        let paths: Vec<_> = base.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["later.txt", "src/lib.rs"]);

        let range = reviews
            .start(TargetSpec::range("main..feature").unwrap(), None, 2)
            .unwrap();
        let revision = range.review.current();
        assert_eq!(revision.base, initial);
        assert_eq!(revision.head_commit.as_deref(), Some(feature.as_str()));
        assert!(!revision.includes_worktree);
        let paths: Vec<_> = range.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(paths, ["src/lib.rs"]);
        let listed: Vec<_> = reviews
            .list(false)
            .unwrap()
            .into_iter()
            .map(|summary| summary.id)
            .collect();
        assert_eq!(listed, [base.review.id, range.review.id]);
    }

    #[test]
    fn threads_must_anchor_inside_the_reviewed_diff_and_batches_are_all_or_nothing() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Worktree, None, 1)
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
        assert_eq!(reviews.show(&id).unwrap().review.threads.len(), 0);

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
            .start(TargetSpec::Worktree, None, 1)
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
            reviews.show(&id).unwrap().review.threads.len(),
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
            .start(TargetSpec::Worktree, None, 1)
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
                threads[0].anchor.as_ref().unwrap().start
            ),
            (0, 5)
        );
        assert_eq!(threads[1].status, Status::Open);
        assert_eq!(
            (
                threads[1].revision,
                threads[1].anchor.as_ref().unwrap().start
            ),
            (1, 17)
        );
        assert_eq!(threads[2].revision, 1);
        assert_eq!(
            (
                threads[3].status,
                threads[3].revision,
                threads[3].anchor.as_ref().unwrap().start
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
            .start(TargetSpec::Worktree, None, 1)
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
        let shown = reviews.show(&id).unwrap().review;
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
            reviews.show("r-00000000"),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(reviews.show("../../etc"), Err(Error::Invalid(_))));
        assert_eq!(
            reviews.show(&id).unwrap().review.threads[0].comments.len(),
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
                .start(TargetSpec::Worktree, None, 1)
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
            .start(TargetSpec::Worktree, None, 1)
            .unwrap()
            .review
            .id;
        // Only the primary checkout calls src/lib.rs binary.
        repo.write(".gitattributes", "src/lib.rs -diff\n");

        let primary = repo.reviews();
        let shown = primary.show(&id).unwrap();
        assert_eq!(shown.files.len(), 1);
        assert!(!shown.files[0].binary, "{:?}", shown.files);
        assert_eq!((shown.files[0].added, shown.files[0].removed), (1, 0));
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
            .start(TargetSpec::Worktree, None, 1)
            .unwrap()
            .review
            .id;
        repo.write("notes/new.md", "new file, edited\n");
        reviews.refresh(&id, 2).unwrap();
        let review = reviews.show(&id).unwrap().review;
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
            .start(TargetSpec::Worktree, None, 1)
            .unwrap()
            .review
            .id;
        let linked = repo.linked();
        fs::write(linked.join("src/lib.rs"), "replaced\n").unwrap();
        let other = Reviews::open(&linked).unwrap();
        let theirs = other
            .start(TargetSpec::Worktree, None, 2)
            .unwrap()
            .review
            .id;
        other.add(&theirs, vec![comment(None)], None, 3).unwrap();

        let ids = |reviews: &Reviews, all| -> Vec<String> {
            reviews
                .list(all)
                .unwrap()
                .into_iter()
                .map(|summary| summary.id)
                .collect()
        };
        assert_eq!(ids(&primary, false), [own.as_str()]);
        assert_eq!(ids(&other, false), [theirs.as_str()]);
        assert_eq!(ids(&primary, true), [own.as_str(), theirs.as_str()]);

        let listed = other.list(true).unwrap();
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
            .start(TargetSpec::Worktree, None, 1)
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
        let shown = primary.show(&id).unwrap();
        assert_eq!(shown.file, other.directory().join(format!("{id}.json")));
        assert_eq!(shown.review.worktree.path, linked);
        primary
            .reply(&id, "t1", "user".into(), "Seen from main.".into(), 3)
            .unwrap();
        assert_eq!(
            other.show(&id).unwrap().review.threads[0].comments.len(),
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
        assert_eq!(other.show(&id).unwrap().review.revisions.len(), 2);

        primary.remove(&id).unwrap();
        assert!(matches!(other.show(&id), Err(Error::NotFound(_))));
        assert_eq!(repo.git(&["for-each-ref", "refs/dots/"]), "");
    }

    #[test]
    fn reviewed_content_survives_garbage_collection_until_removed() {
        let repo = Repo::new();
        change_line_ten(&repo);
        let reviews = repo.reviews();
        let id = reviews
            .start(TargetSpec::Worktree, None, 1)
            .unwrap()
            .review
            .id;
        let head = reviews.show(&id).unwrap().review.current().head.clone();
        repo.git(&["checkout", "--", "src/lib.rs"]);
        repo.git(&["gc", "-q", "--prune=now"]);
        reviews.show(&id).unwrap();
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
            .start(TargetSpec::Worktree, None, 1)
            .unwrap()
            .review
            .id;
        fs::write(linked.join("src/lib.rs"), numbered(1..=20)).unwrap();
        repo.git(&["gc", "-q", "--prune=now"]);
        reviews.show(&id).unwrap();

        repo.git(&["worktree", "remove", "--force", linked.to_str().unwrap()]);
        change_line_ten(&repo);
        let primary = repo.reviews();
        let own = primary
            .start(TargetSpec::Worktree, None, 2)
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
            .start(TargetSpec::Worktree, None, 1)
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
        let threads = repo.reviews().show(&id).unwrap().review.threads;
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
            .start(TargetSpec::Worktree, None, 1)
            .unwrap()
            .review
            .id;
        let file = repo
            .root
            .join(".git/dots/reviews")
            .join(format!("{id}.json"));
        assert!(file.is_file());
        assert_eq!(reviews.directory(), file.parent().unwrap());

        fs::write(
            &file,
            format!("{{\"schema\": 1, \"id\": \"{id}\", \"findings\": []}}"),
        )
        .unwrap();
        let Err(error @ Error::Corrupt { .. }) = reviews.show(&id) else {
            panic!("a schema 1 file was read");
        };
        let message = error.to_string();
        assert!(
            message.contains(&file.display().to_string()) && message.contains("schema 1"),
            "{message}"
        );
        assert!(matches!(reviews.list(true), Err(Error::Corrupt { .. })));
        fs::write(&file, "{\"schema\": 2}").unwrap();
        assert!(matches!(reviews.show(&id), Err(Error::Corrupt { .. })));

        reviews.remove(&id).unwrap();
        assert!(!file.exists());
        assert!(matches!(reviews.remove(&id), Err(Error::NotFound(_))));
    }
}
