//! The viewer's key map: one table drives both dispatch and the help
//! overlay, so every listed key has a handler and every handled key is
//! listed.

use ratatui::termina::event::KeyCode;

/// What a key does. Where it depends on focus, the handler decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Down,
    Up,
    HalfDown,
    HalfUp,
    /// Space: opens or folds a folder in the file list, else pages down.
    Space,
    PageDown,
    PageUp,
    Top,
    Bottom,
    NextFile,
    PrevFile,
    NextChange,
    PrevChange,
    Comment,
    Suggest,
    Visual,
    Cancel,
    FileComment,
    /// `;`: the next key completes a review-wide command.
    Prefix,
    /// Enter: opens the file or folds the folder in the file list; in the
    /// diff, edits the card or expands the fold at the cursor.
    Enter,
    /// `e`: in the diff, edits the card or expands the fold at the cursor.
    Expand,
    ExpandAll,
    /// Folds the file at the cursor to its header, or unfolds it.
    Fold,
    FoldAll,
    /// Marks code moved between places, or stops.
    Moves,
    Delete,
    NextCard,
    PrevCard,
    OldSide,
    Send,
    PickAgent,
    Since,
    Reviewed,
    Authors,
    Switcher,
    NextWorktree,
    PrevWorktree,
    NextReview,
    PrevReview,
    Focus,
    Left,
    Right,
    Layout,
    Wrap,
    Compare,
    Sidebar,
    Narrower,
    Wider,
    Refresh,
    Help,
    Quit,
}

impl Action {
    /// Acts on the diff cursor, so needs the diff focused.
    pub(crate) fn needs_diff(self) -> bool {
        matches!(
            self,
            Self::Comment
                | Self::Suggest
                | Self::Visual
                | Self::Delete
                | Self::NextCard
                | Self::PrevCard
                | Self::OldSide
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) code: KeyCode,
    pub(crate) ctrl: bool,
}

const fn key(c: char) -> Key {
    Key {
        code: KeyCode::Char(c),
        ctrl: false,
    }
}

const fn ctrl(c: char) -> Key {
    Key {
        code: KeyCode::Char(c),
        ctrl: true,
    }
}

const fn named(code: KeyCode) -> Key {
    Key { code, ctrl: false }
}

/// One help row and the keys it describes.
pub(crate) struct Binding {
    pub(crate) label: &'static str,
    pub(crate) help: &'static str,
    pub(crate) keys: &'static [(Key, Action)],
}

const fn row(label: &'static str, help: &'static str, keys: &'static [(Key, Action)]) -> Binding {
    Binding { label, help, keys }
}

pub(crate) const BINDINGS: &[Binding] = &[
    row(
        "j k  ↓ ↑",
        "move the cursor, or in the file list",
        &[
            (key('j'), Action::Down),
            (named(KeyCode::Down), Action::Down),
            (key('k'), Action::Up),
            (named(KeyCode::Up), Action::Up),
        ],
    ),
    row(
        "^d ^u",
        "half page down, up",
        &[(ctrl('d'), Action::HalfDown), (ctrl('u'), Action::HalfUp)],
    ),
    row(
        "space ^f  ^b",
        "page down, up (PgDn, PgUp)",
        &[
            (key(' '), Action::Space),
            (ctrl('f'), Action::PageDown),
            (named(KeyCode::PageDown), Action::PageDown),
            (ctrl('b'), Action::PageUp),
            (named(KeyCode::PageUp), Action::PageUp),
        ],
    ),
    row(
        "g  G",
        "top, bottom",
        &[
            (key('g'), Action::Top),
            (named(KeyCode::Home), Action::Top),
            (key('G'), Action::Bottom),
        ],
    ),
    row(
        "J K  ] [",
        "next/previous file",
        &[
            (key('J'), Action::NextFile),
            (key(']'), Action::NextFile),
            (key('K'), Action::PrevFile),
            (key('['), Action::PrevFile),
        ],
    ),
    row(
        "n  N p",
        "next/previous change",
        &[
            (key('n'), Action::NextChange),
            (key('N'), Action::PrevChange),
            (key('p'), Action::PrevChange),
        ],
    ),
    row(
        "c  s",
        "comment on the line or selection; suggest code for it",
        &[(key('c'), Action::Comment), (key('s'), Action::Suggest)],
    ),
    row(
        "v  esc",
        "select lines (j k or drag to extend); cancel",
        &[
            (key('v'), Action::Visual),
            (named(KeyCode::Escape), Action::Cancel),
        ],
    ),
    row(
        "C  ;c",
        "comment on the file; on the whole review",
        &[(key('C'), Action::FileComment), (key(';'), Action::Prefix)],
    ),
    row(
        "enter",
        "files: open file, fold folder; diff: edit card, expand fold",
        &[(named(KeyCode::Enter), Action::Enter)],
    ),
    row(
        "e  E",
        "edit the card or expand the fold at the cursor; all folds",
        &[(key('e'), Action::Expand), (key('E'), Action::ExpandAll)],
    ),
    row(
        "f  F",
        "fold the file at the cursor to its header; all files",
        &[(key('f'), Action::Fold), (key('F'), Action::FoldAll)],
    ),
    row(
        "d x",
        "on a card: delete the comment, dismiss the note",
        &[(key('d'), Action::Delete), (key('x'), Action::Delete)],
    ),
    row(
        "(  )",
        "previous/next comment or note",
        &[(key('('), Action::PrevCard), (key(')'), Action::NextCard)],
    ),
    row(
        "o",
        "split view: comment on the old side",
        &[(key('o'), Action::OldSide)],
    ),
    row(
        "S",
        "send all drafts to the agent",
        &[(key('S'), Action::Send)],
    ),
    row(
        "A",
        "choose the agent reviews go to",
        &[(key('A'), Action::PickAgent)],
    ),
    row(
        "R",
        "show only changes since the last send",
        &[(key('R'), Action::Since)],
    ),
    row("m", "mark file reviewed", &[(key('m'), Action::Reviewed)]),
    row(
        "a",
        "show one agent's files, then unknown, then all (experimental)",
        &[(key('a'), Action::Authors)],
    ),
    row(
        "w",
        "switch worktree, questions and reviews first (type to filter)",
        &[(key('w'), Action::Switcher)],
    ),
    row(
        "{  }",
        "previous/next worktree",
        &[
            (key('{'), Action::PrevWorktree),
            (key('}'), Action::NextWorktree),
        ],
    ),
    row(
        ",  .",
        "previous/next worktree that asks or waits for review",
        &[
            (key(','), Action::PrevReview),
            (key('.'), Action::NextReview),
        ],
    ),
    row(
        "tab",
        "focus files or diff",
        &[
            (named(KeyCode::Tab), Action::Focus),
            (named(KeyCode::BackTab), Action::Focus),
        ],
    ),
    row(
        "h l  ← →",
        "files: fold, unfold, parent; else focus files, diff",
        &[
            (key('h'), Action::Left),
            (named(KeyCode::Left), Action::Left),
            (key('l'), Action::Right),
            (named(KeyCode::Right), Action::Right),
        ],
    ),
    row(
        "L",
        "layout: auto, unified, split",
        &[(key('L'), Action::Layout)],
    ),
    row("z", "wrap long lines", &[(key('z'), Action::Wrap)]),
    row("M", "mark moved code", &[(key('M'), Action::Moves)]),
    row(
        "b",
        "compare: branch (merge base) or uncommitted",
        &[(key('b'), Action::Compare)],
    ),
    row(
        "t  < >",
        "show sidebar; narrower, wider",
        &[
            (key('t'), Action::Sidebar),
            (key('<'), Action::Narrower),
            (key('>'), Action::Wider),
        ],
    ),
    row("r", "refresh now", &[(key('r'), Action::Refresh)]),
    row("?", "this help", &[(key('?'), Action::Help)]),
    row(
        "q  ^c",
        "quit",
        &[(key('q'), Action::Quit), (ctrl('c'), Action::Quit)],
    ),
];

/// The action bound to a key.
pub(crate) fn action(code: KeyCode, ctrl: bool) -> Option<Action> {
    let pressed = Key { code, ctrl };
    BINDINGS
        .iter()
        .flat_map(|b| b.keys)
        .find(|(k, _)| *k == pressed)
        .map(|(_, a)| *a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_is_bound_once_and_every_row_has_keys() {
        let mut seen: Vec<Key> = Vec::new();
        for b in BINDINGS {
            assert!(!b.keys.is_empty(), "{} lists no keys", b.label);
            for (k, _) in b.keys {
                assert!(!seen.contains(k), "{k:?} is bound twice");
                seen.push(*k);
            }
        }
    }

    #[test]
    fn every_char_key_appears_in_its_row_label() {
        for b in BINDINGS {
            for (k, a) in b.keys {
                let KeyCode::Char(c) = k.code else { continue };
                let shown = match (c, k.ctrl) {
                    (' ', _) => "space".to_owned(),
                    (c, true) => format!("^{c}"),
                    (c, false) => c.to_string(),
                };
                assert!(
                    b.label
                        .split_whitespace()
                        .any(|w| w == shown || (*a == Action::Prefix && w.starts_with(&shown))),
                    "{shown} is handled but not shown in {:?}",
                    b.label
                );
            }
        }
    }

    #[test]
    fn lookup_finds_bound_keys() {
        assert_eq!(action(KeyCode::Char('v'), false), Some(Action::Visual));
        assert_eq!(action(KeyCode::Char('c'), true), Some(Action::Quit));
        assert_eq!(action(KeyCode::Char('Z'), false), None);
        assert_eq!(action(KeyCode::Char('.'), false), Some(Action::NextReview));
        assert_eq!(action(KeyCode::Char(','), false), Some(Action::PrevReview));
    }
}
