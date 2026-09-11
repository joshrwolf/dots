use crate::github::{Check, CheckBucket, CiState, PullRequestState, ReviewStatus, SidebarSnapshot};

pub const PR_TOKEN: &str = "gh_pr";
pub const STATE_TOKEN: &str = "gh_state";
pub const REVIEW_TOKEN: &str = "gh_review";
pub const CHECKS_TOKEN: &str = "gh_checks";
pub const FAILED_TOKEN: &str = "gh_failed";
pub const PENDING_TOKEN: &str = "gh_pending";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub pull_request: String,
    pub state: &'static str,
    pub review: Option<&'static str>,
    pub checks: Option<&'static str>,
    pub failed: Option<String>,
    pub pending: Option<String>,
}

impl Preview {
    #[must_use]
    pub fn from_state(state: &CiState) -> Self {
        Self::from_parts(
            state.number,
            state.draft,
            &state.pull_request_state,
            state.review,
            &state.checks,
        )
    }

    pub(crate) fn from_snapshot(snapshot: &SidebarSnapshot) -> Self {
        let pr = &snapshot.pull_request;
        Self::from_parts(pr.number, pr.draft, &pr.state, pr.review, &snapshot.checks)
    }

    fn from_parts(
        number: u64,
        draft: bool,
        state: &PullRequestState,
        review: ReviewStatus,
        checks: &[Check],
    ) -> Self {
        let failed = checks
            .iter()
            .filter(|check| matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel))
            .count();
        let pending = checks
            .iter()
            .filter(|check| check.bucket == CheckBucket::Pending)
            .count();
        Self {
            pull_request: format!("#{number}"),
            state: match state {
                PullRequestState::Open if draft => "○ Draft",
                PullRequestState::Open => "↗ Open",
                PullRequestState::Closed => "× Closed",
                PullRequestState::Merged => "◆ Merged",
            },
            review: match review {
                ReviewStatus::Approved => Some("✓ Approved"),
                ReviewStatus::ChangesRequested => Some("! Changes"),
                _ if draft || *state != PullRequestState::Open => None,
                ReviewStatus::Needed | ReviewStatus::Requested => Some("◷ Review"),
                ReviewStatus::NotReviewed => Some("○ Unreviewed"),
                ReviewStatus::Unknown => Some("? Review"),
            },
            failed: (failed > 0).then(|| format!("✗ {failed}")),
            pending: (pending > 0).then(|| format!("◷ {pending}")),
            checks: if failed > 0 || pending > 0 || *state != PullRequestState::Open {
                None
            } else if checks.is_empty() {
                Some("— No checks")
            } else if checks
                .iter()
                .all(|check| check.bucket == CheckBucket::Skipping)
            {
                Some("— Skipped")
            } else {
                Some("✓ CI")
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::github::{Check, CheckBucket, CiVerdict, ReviewStatus};

    use super::*;

    fn state(pull_request_state: PullRequestState, verdict: CiVerdict) -> CiState {
        use std::collections::HashSet;

        use crate::github::{Check, CheckBucket};

        CiState {
            number: 42,
            draft: false,
            pull_request_state,
            review: ReviewStatus::Needed,
            verdict,
            checks: [CheckBucket::Fail, CheckBucket::Fail]
                .into_iter()
                .chain(std::iter::repeat_n(CheckBucket::Pending, 3))
                .chain(std::iter::repeat_n(CheckBucket::Pass, 5))
                .map(|bucket| Check {
                    bucket,
                    name: String::new(),
                    link: String::new(),
                    workflow: String::new(),
                })
                .collect(),
            required_check_names: HashSet::from(["test".to_owned()]),
            local_branch: "main".to_owned(),
            local_head_oid: "abc".to_owned(),
            remote_head_oid: "abc".to_owned(),
        }
    }

    #[test]
    fn every_open_signal_is_explicit() {
        assert_eq!(
            Preview::from_state(&state(PullRequestState::Open, CiVerdict::Failed)),
            Preview {
                pull_request: "#42".to_owned(),
                state: "↗ Open",
                review: Some("◷ Review"),
                checks: None,
                failed: Some("✗ 2".to_owned()),
                pending: Some("◷ 3".to_owned()),
            }
        );
    }

    #[test]
    fn draft_and_terminal_pull_requests_are_explicit() {
        let mut draft = state(PullRequestState::Open, CiVerdict::Green);
        draft.draft = true;
        assert_eq!(Preview::from_state(&draft).state, "○ Draft");
        assert_eq!(Preview::from_state(&draft).review, None);
        draft.review = ReviewStatus::ChangesRequested;
        assert_eq!(Preview::from_state(&draft).review, Some("! Changes"));
        draft.review = ReviewStatus::Approved;
        assert_eq!(Preview::from_state(&draft).review, Some("✓ Approved"));
        assert_eq!(
            Preview::from_state(&state(PullRequestState::Closed, CiVerdict::Green)).state,
            "× Closed"
        );
        assert_eq!(
            Preview::from_state(&state(PullRequestState::Merged, CiVerdict::Green)).state,
            "◆ Merged"
        );
    }

    #[test]
    fn no_checks_are_explicit() {
        let mut empty = state(PullRequestState::Open, CiVerdict::Green);
        empty.checks.clear();
        assert_eq!(Preview::from_state(&empty).checks, Some("— No checks"));
    }

    #[test]
    fn optional_failures_are_visible_even_when_required_checks_pass() {
        let mut passing_required = state(PullRequestState::Open, CiVerdict::Green);
        passing_required.checks = vec![Check {
            bucket: CheckBucket::Fail,
            name: "optional".to_owned(),
            link: String::new(),
            workflow: String::new(),
        }];
        assert_eq!(
            Preview::from_state(&passing_required).failed,
            Some("✗ 1".to_owned())
        );
    }
}
