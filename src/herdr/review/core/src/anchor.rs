use crate::{AnchorLocation, AnchorResolution, AnchorStatus};

const CONTEXT_DEPTH: usize = 64;

pub(crate) fn resolve(
    target: &AnchorLocation,
    original: &str,
    current_path: Option<&str>,
    current: Option<&str>,
) -> AnchorResolution {
    let Some(current) = current else {
        return AnchorResolution {
            status: AnchorStatus::Deleted,
            current_location: None,
        };
    };
    let path = current_path.unwrap_or(&target.path).to_owned();
    let current_lines = lines(current);

    if original == current && path == target.path {
        return resolution(AnchorStatus::Exact, target.clone());
    }

    let original_lines = lines(original);
    let (start, end) = (target.start_line, target.end_line);
    let Some(start_index) = usize::try_from(start)
        .ok()
        .and_then(|line| line.checked_sub(1))
    else {
        return unresolved(AnchorStatus::Unavailable);
    };
    let Some(end_index) = usize::try_from(end).ok() else {
        return unresolved(AnchorStatus::Unavailable);
    };
    let Some(selected) = original_lines.get(start_index..end_index) else {
        return unresolved(AnchorStatus::Unavailable);
    };

    let matches = exact_matches(&current_lines, selected);
    if let Some(candidate) = choose_exact(
        &original_lines,
        start_index,
        end_index,
        &current_lines,
        &matches,
    ) {
        let relocated = target.relocated(
            path,
            u32::try_from(candidate + 1).unwrap_or(u32::MAX),
            u32::try_from(candidate + selected.len()).unwrap_or(u32::MAX),
        );
        let status = if relocated == *target {
            AnchorStatus::Exact
        } else {
            AnchorStatus::Shifted
        };
        return resolution(status, relocated);
    }
    if matches.len() > 1 {
        return unresolved(AnchorStatus::Ambiguous);
    }

    resolve_changed(
        target,
        path,
        &original_lines,
        start_index,
        end_index,
        &current_lines,
    )
}

fn resolve_changed(
    target: &AnchorLocation,
    path: String,
    original: &[&str],
    start: usize,
    end: usize,
    current: &[&str],
) -> AnchorResolution {
    let before = best_boundary_before(original, start, current);
    let after = best_boundary_after(original, end, current);
    let (new_start, new_end) = match (before, after) {
        (Some(left), Some(right)) if left <= right => (left, right),
        _ => return unresolved(AnchorStatus::Ambiguous),
    };
    if new_start == new_end {
        return unresolved(AnchorStatus::Deleted);
    }
    let Ok(start_line) = u32::try_from(new_start + 1) else {
        return unresolved(AnchorStatus::Unavailable);
    };
    let Ok(end_line) = u32::try_from(new_end) else {
        return unresolved(AnchorStatus::Unavailable);
    };
    resolution(
        AnchorStatus::Modified,
        target.relocated(path, start_line, end_line),
    )
}

fn best_boundary_before(original: &[&str], start: usize, current: &[&str]) -> Option<usize> {
    let depth = start.min(CONTEXT_DEPTH);
    for length in (1..=depth).rev() {
        let Some(needle) = original.get(start - length..start) else {
            continue;
        };
        let matches = exact_matches(current, needle);
        if let [candidate] = matches.as_slice() {
            return Some(candidate + length);
        }
    }
    (start == 0).then_some(0)
}

fn best_boundary_after(original: &[&str], end: usize, current: &[&str]) -> Option<usize> {
    let depth = original.len().saturating_sub(end).min(CONTEXT_DEPTH);
    for length in (1..=depth).rev() {
        let Some(needle) = original.get(end..end + length) else {
            continue;
        };
        let matches = exact_matches(current, needle);
        if let [candidate] = matches.as_slice() {
            return Some(*candidate);
        }
    }
    (end == original.len()).then_some(current.len())
}

fn choose_exact(
    original: &[&str],
    start: usize,
    end: usize,
    current: &[&str],
    matches: &[usize],
) -> Option<usize> {
    if matches.len() == 1 {
        return matches.first().copied();
    }
    let mut scored = matches
        .iter()
        .copied()
        .map(|candidate| {
            (
                context_score(original, start, end, current, candidate),
                candidate,
            )
        })
        .collect::<Vec<_>>();
    scored.sort_unstable_by(|left, right| right.cmp(left));
    match scored.as_slice() {
        [(score, candidate), ..]
            if *score > 0 && scored.get(1).is_none_or(|next| next.0 < *score) =>
        {
            Some(*candidate)
        }
        _ => None,
    }
}

fn context_score(
    original: &[&str],
    start: usize,
    end: usize,
    current: &[&str],
    candidate: usize,
) -> usize {
    let before_score = original
        .get(..start)
        .unwrap_or_default()
        .iter()
        .rev()
        .zip(current.get(..candidate).unwrap_or_default().iter().rev())
        .take(CONTEXT_DEPTH)
        .take_while(|(left, right)| left == right)
        .count();
    let current_after = candidate + end.saturating_sub(start);
    let after_score = original
        .get(end..)
        .unwrap_or_default()
        .iter()
        .zip(current.get(current_after..).unwrap_or_default())
        .take(CONTEXT_DEPTH)
        .take_while(|(left, right)| left == right)
        .count();
    before_score + after_score
}

fn exact_matches(haystack: &[&str], needle: &[&str]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, candidate)| (candidate == needle).then_some(index))
        .collect()
}

fn lines(content: &str) -> Vec<&str> {
    if content.is_empty() {
        vec![""]
    } else {
        content.lines().collect()
    }
}

fn resolution(status: AnchorStatus, target: AnchorLocation) -> AnchorResolution {
    AnchorResolution {
        status,
        current_location: Some(target),
    }
}

fn unresolved(status: AnchorStatus) -> AnchorResolution {
    AnchorResolution {
        status,
        current_location: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiffSide;

    fn target(start_line: u32, end_line: u32) -> AnchorLocation {
        AnchorLocation {
            path: "src/lib.rs".to_owned(),
            side: DiffSide::Target,
            start_line,
            end_line,
        }
    }

    #[test]
    fn follows_an_exact_selection_after_insertions() {
        let result = resolve(
            &target(2, 3),
            "a\nb\nc\nd\n",
            Some("src/lib.rs"),
            Some("new\na\nb\nc\nd\n"),
        );
        assert_eq!(result.status, AnchorStatus::Shifted);
        let location = result.current_location.unwrap();
        assert_eq!((location.start_line, location.end_line), (3, 4));
    }

    #[test]
    fn uses_surrounding_source_to_disambiguate_duplicates() {
        let result = resolve(
            &target(2, 2),
            "before\nsame\nafter\n",
            Some("src/lib.rs"),
            Some("same\nother\nbefore\nsame\nafter\n"),
        );
        assert_eq!(result.status, AnchorStatus::Shifted);
        let location = result.current_location.unwrap();
        assert_eq!((location.start_line, location.end_line), (4, 4));
    }

    #[test]
    fn marks_tied_duplicates_ambiguous() {
        let result = resolve(
            &target(1, 1),
            "same\n",
            Some("src/lib.rs"),
            Some("same\nother\nsame\n"),
        );
        assert_eq!(result.status, AnchorStatus::Ambiguous);
        assert!(result.current_location.is_none());
    }

    #[test]
    fn maps_a_changed_selection_between_stable_boundaries() {
        let result = resolve(
            &target(2, 3),
            "before\nold one\nold two\nafter\n",
            Some("src/lib.rs"),
            Some("before\nnew one\nnew two\nnew three\nafter\n"),
        );
        assert_eq!(result.status, AnchorStatus::Modified);
        let location = result.current_location.unwrap();
        assert_eq!((location.start_line, location.end_line), (2, 4));
    }

    #[test]
    fn recognizes_a_deleted_selection() {
        let result = resolve(
            &target(2, 2),
            "before\ngone\nafter\n",
            Some("src/lib.rs"),
            Some("before\nafter\n"),
        );
        assert_eq!(result.status, AnchorStatus::Deleted);
        assert!(result.current_location.is_none());
    }
}
