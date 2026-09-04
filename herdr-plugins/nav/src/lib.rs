//! `ctrl+hjkl` shared between herdr panes, vim splits, and fzf.
//!
//! The chord means "move left" to herdr and "go to the split on the left" to
//! vim, and the user should not have to know which one has the pane. So the
//! decision is made from what is actually running in it: forward the key to a
//! program that has its own notion of splits, move pane focus otherwise.

use herdrkit::api::{Direction, ProcessInfo};
use herdrkit::apps::{accepts_vim_navigation, is_fzf};

/// The chord the user pressed for `direction`, in herdr's syntax.
pub fn chord(direction: Direction) -> &'static str {
    match direction {
        Direction::Left => "ctrl+h",
        Direction::Down => "ctrl+j",
        Direction::Up => "ctrl+k",
        Direction::Right => "ctrl+l",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDecision {
    /// Hand the chord to the program in the pane.
    Forward,
    /// Move herdr's pane focus.
    MoveFocus,
}

/// What to do with `direction`, given the pane's foreground process.
///
/// `None` for the process means herdr could not say, and that resolves to
/// moving focus: a chord that does nothing reads as a broken keyboard, whereas
/// a focus move that was not wanted is undone by pressing the opposite one.
pub fn decide(direction: Direction, info: Option<&ProcessInfo>) -> NavigationDecision {
    let Some(info) = info else {
        return NavigationDecision::MoveFocus;
    };
    if info.find(accepts_vim_navigation).is_some() {
        return NavigationDecision::Forward;
    }
    // fzf binds ctrl-h to backward-delete-char and ctrl-l to clear-screen, so
    // forwarding those would eat a character out of the query instead of
    // moving anything. Only the vertical pair means navigation there.
    if info.find(is_fzf).is_some() && matches!(direction, Direction::Down | Direction::Up) {
        return NavigationDecision::Forward;
    }
    NavigationDecision::MoveFocus
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(commands: &[&str]) -> ProcessInfo {
        let processes: Vec<String> = commands
            .iter()
            .enumerate()
            .map(|(pid, name)| {
                format!(r#"{{"pid":{pid},"name":"{name}","argv0":"/usr/bin/{name}"}}"#)
            })
            .collect();
        serde_json::from_str(&format!(
            r#"{{"pane_id":"w1:p1","foreground_processes":[{}]}}"#,
            processes.join(",")
        ))
        .unwrap()
    }

    const EVERY: [Direction; 4] = [
        Direction::Left,
        Direction::Down,
        Direction::Up,
        Direction::Right,
    ];

    #[test]
    fn vim_gets_every_direction() {
        for direction in EVERY {
            assert_eq!(
                decide(direction, Some(&info(&["nvim"]))),
                NavigationDecision::Forward
            );
        }
    }

    #[test]
    fn fzf_gets_only_the_vertical_pair() {
        let fzf = info(&["fzf"]);
        assert_eq!(
            decide(Direction::Down, Some(&fzf)),
            NavigationDecision::Forward
        );
        assert_eq!(
            decide(Direction::Up, Some(&fzf)),
            NavigationDecision::Forward
        );
        assert_eq!(
            decide(Direction::Left, Some(&fzf)),
            NavigationDecision::MoveFocus
        );
        assert_eq!(
            decide(Direction::Right, Some(&fzf)),
            NavigationDecision::MoveFocus
        );
    }

    #[test]
    fn a_plain_shell_moves_pane_focus() {
        for direction in EVERY {
            assert_eq!(
                decide(direction, Some(&info(&["zsh"]))),
                NavigationDecision::MoveFocus
            );
        }
    }

    #[test]
    fn an_unreadable_pane_moves_focus_rather_than_doing_nothing() {
        for direction in EVERY {
            assert_eq!(decide(direction, None), NavigationDecision::MoveFocus);
            assert_eq!(
                decide(direction, Some(&info(&[]))),
                NavigationDecision::MoveFocus
            );
        }
    }

    #[test]
    fn vim_wins_over_a_shell_beneath_it() {
        assert_eq!(
            decide(Direction::Left, Some(&info(&["nvim", "zsh"]))),
            NavigationDecision::Forward
        );
    }

    /// The action ids in the manifest are the lowercase variant names, so the
    /// manifest and this parser cannot drift apart without this failing.
    #[test]
    fn every_direction_round_trips_through_its_action_id() {
        for direction in EVERY {
            let name = format!("{direction:?}").to_lowercase();
            assert_eq!(name.parse(), Ok(direction));
        }
        assert!("sideways".parse::<Direction>().is_err());
    }

    #[test]
    fn each_direction_has_its_own_chord() {
        let mut chords: Vec<&str> = EVERY.iter().map(|d| chord(*d)).collect();
        chords.sort_unstable();
        chords.dedup();
        assert_eq!(chords.len(), EVERY.len());
        assert_eq!(chord(Direction::Left), "ctrl+h");
    }
}

#[cfg(test)]
mod live_shape {
    use super::*;

    /// The exact reply `pane.process_info` gave for a pane running nvim,
    /// extra fields and all.
    const REAL: &str = r#"{"pane_id":"w2:p1T","shell_pid":88357,
        "foreground_process_group_id":88575,
        "foreground_processes":[{"pid":88575,"name":"nvim","argv0":"nvim",
        "argv":["/Users/dev/.local/bin/nvim","README.md"],
        "cmdline":"/Users/dev/.local/bin/nvim README.md","cwd":"/repo"}]}"#;

    #[test]
    fn a_real_reply_for_an_nvim_pane_forwards() {
        let info: ProcessInfo = serde_json::from_str(REAL).unwrap();
        assert_eq!(info.foreground_processes.len(), 1);
        assert_eq!(
            info.find(accepts_vim_navigation).map(|p| p.pid),
            Some(88575)
        );
        assert_eq!(
            decide(Direction::Left, Some(&info)),
            NavigationDecision::Forward
        );
    }
}
