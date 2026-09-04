use crate::github::{CiState, CiVerdict, PullRequestState};

#[must_use]
pub fn token(state: &CiState) -> String {
    let draft = if state.draft { "◦" } else { "" };
    match state.pull_request_state {
        PullRequestState::Closed => format!("#{} closed", state.number),
        PullRequestState::Merged => format!("#{} merged", state.number),
        PullRequestState::Open if state.total_checks() == 0 => {
            format!("#{}{draft}", state.number)
        }
        PullRequestState::Open => match state.verdict {
            CiVerdict::Failed => format!(
                "#{}{draft} ✗ {}/{}",
                state.number,
                state.failed_checks(),
                state.total_checks()
            ),
            CiVerdict::Pending => format!(
                "#{}{draft} ● {}/{}",
                state.number,
                state.pending_checks(),
                state.total_checks()
            ),
            CiVerdict::Green if state.pending_checks() > 0 => format!(
                "#{}{draft} ✓ {} ●{}",
                state.number,
                state.total_checks(),
                state.pending_checks()
            ),
            CiVerdict::Green => {
                format!("#{}{draft} ✓ {}", state.number, state.total_checks())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pull_request_state: PullRequestState, verdict: CiVerdict) -> CiState {
        use std::collections::HashSet;

        use crate::github::{Check, CheckBucket};

        CiState {
            number: 42,
            draft: false,
            pull_request_state,
            review_decision: None,
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
    fn every_open_verdict_has_a_visible_glyph() {
        assert_eq!(
            token(&state(PullRequestState::Open, CiVerdict::Failed)),
            "#42 ✗ 2/10"
        );
        assert_eq!(
            token(&state(PullRequestState::Open, CiVerdict::Pending)),
            "#42 ● 3/10"
        );
        assert_eq!(
            token(&state(PullRequestState::Open, CiVerdict::Green)),
            "#42 ✓ 10 ●3"
        );
    }

    #[test]
    fn a_draft_and_terminal_pull_requests_are_explicit() {
        let mut draft = state(PullRequestState::Open, CiVerdict::Green);
        draft.draft = true;
        for check in &mut draft.checks {
            if check.bucket == crate::github::CheckBucket::Pending {
                check.bucket = crate::github::CheckBucket::Pass;
            }
        }
        assert_eq!(token(&draft), "#42◦ ✓ 10");
        assert_eq!(
            token(&state(PullRequestState::Closed, CiVerdict::Green)),
            "#42 closed"
        );
        assert_eq!(
            token(&state(PullRequestState::Merged, CiVerdict::Green)),
            "#42 merged"
        );
    }

    #[test]
    fn an_open_pull_request_with_no_checks_is_just_its_number() {
        let mut empty = state(PullRequestState::Open, CiVerdict::Green);
        empty.checks.clear();
        assert_eq!(token(&empty), "#42");
    }
}
