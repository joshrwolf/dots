//! Cards in the diff stream: comments and agent notes, drawn under the
//! lines they are about, under a file's header, or above the first file.

use std::collections::HashMap;
use std::path::Path;

use crate::plan::{Kind, Layout, Row};
use crate::review::{self, Comment, Place, Side};
use crate::session::{Note, Session};
use crate::wrap;

/// Code lines of a suggestion shown on its card.
const SUGGESTION_LINES: usize = 12;

/// A card in the stream: a comment, or an agent's note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CardRef {
    Comment(u64),
    Note(u64),
}

/// A card placed in the stream: under row `row` from its line `start`
/// (row cards), or from line `start` of a stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placed {
    pub(crate) card: CardRef,
    pub(crate) row: u32,
    pub(crate) start: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    Title,
    Text,
    /// A line of suggested code.
    Code,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardLine {
    pub(crate) text: String,
    pub(crate) look: Look,
}

pub(crate) enum Card<'a> {
    Comment(&'a Comment),
    Note(&'a Note),
}

impl Card<'_> {
    pub(crate) fn place(&self) -> &Place {
        match self {
            Card::Comment(c) => &c.place,
            Card::Note(n) => &n.place,
        }
    }
}

pub(crate) fn get(session: &Session, card: CardRef) -> Option<Card<'_>> {
    match card {
        CardRef::Comment(id) => session
            .state()
            .comments
            .iter()
            .find(|c| c.id == id)
            .map(Card::Comment),
        CardRef::Note(id) => session.notes().iter().find(|n| n.id == id).map(Card::Note),
    }
}

fn range(lines: &review::Lines) -> String {
    let side = if lines.side == Side::Old { " old" } else { "" };
    if lines.end > lines.start {
        format!("lines {}-{}{side}", lines.start + 1, lines.end + 1)
    } else {
        format!("line {}{side}", lines.start + 1)
    }
}

/// A card's display lines at `width` cells. Cards in a stack name the
/// lines they are about; row cards sit under them.
pub(crate) fn lines(card: &Card<'_>, width: u16, in_stack: bool) -> Vec<CardLine> {
    let width = width.max(8);
    let mut title = Vec::new();
    let (text, suggestion) = match card {
        Card::Comment(c) => {
            title.push(c.kind.label().to_owned());
            if c.suggestion.is_some() {
                title.push("suggestion".to_owned());
            }
            title.push(if c.sent { "sent" } else { "draft" }.to_owned());
            if c.outdated {
                title.push("outdated".to_owned());
            }
            (c.text.as_str(), c.suggestion.as_deref())
        }
        Card::Note(n) => {
            title.push("note from the agent".to_owned());
            (n.text.as_str(), None)
        }
    };
    if card.place() == &Place::Review {
        title.insert(0, "review".to_owned());
    } else if let (true, Some(l)) = (in_stack, card.place().lines()) {
        title.push(range(l));
    }
    let mut out = vec![CardLine {
        text: title.join(" · "),
        look: Look::Title,
    }];
    for line in text.trim_end().lines() {
        for seg in wrap::wrap(line, width) {
            out.push(CardLine {
                text: format!(
                    "{}{}",
                    " ".repeat(usize::from(seg.indent)),
                    &line[seg.bytes]
                ),
                look: Look::Text,
            });
        }
    }
    if let Some(code) = suggestion {
        let code: Vec<&str> = code.lines().collect();
        for line in code.iter().take(SUGGESTION_LINES) {
            out.push(CardLine {
                text: (*line).to_owned(),
                look: Look::Code,
            });
        }
        if code.len() > SUGGESTION_LINES {
            out.push(CardLine {
                text: format!("… {} more lines", code.len() - SUGGESTION_LINES),
                look: Look::Text,
            });
        }
        if code.is_empty() {
            out.push(CardLine {
                text: "(delete the lines)".to_owned(),
                look: Look::Text,
            });
        }
    }
    out
}

/// The row showing the last of `lines`, if it is shown.
pub(crate) fn attach(rows: &[Row], lines: &review::Lines) -> Option<u32> {
    let n = lines.end;
    let hit = |row: &Row| match (*row, lines.side) {
        (Row::Line { new: Some(x), .. } | Row::Pair { new: Some(x), .. }, Side::New)
        | (
            Row::Line {
                kind: Kind::Delete | Kind::Context,
                old: Some(x),
                ..
            }
            | Row::Pair { old: Some(x), .. },
            Side::Old,
        ) => x == n,
        _ => false,
    };
    rows.iter()
        .position(hit)
        .and_then(|i| u32::try_from(i).ok())
}

/// Places the cards about shown lines under the rows showing them: a
/// row's cards follow its code, `code(row)` lines tall, one after another.
/// Returns them in row order, and the rest, which stack under the header:
/// cards not about lines, about lines not shown, or outdated.
pub(crate) fn place(
    session: &Session,
    cards: &[CardRef],
    rows: &[Row],
    code: impl Fn(u32) -> u32,
    pane: u16,
    layout: Layout,
    number_width: u16,
) -> (Vec<Placed>, Vec<CardRef>) {
    let mut stacked = Vec::new();
    let mut under: Vec<(u32, CardRef, Side)> = Vec::new();
    for &card in cards {
        let c = get(session, card);
        let outdated = matches!(c, Some(Card::Comment(c)) if c.outdated);
        let row = c
            .as_ref()
            .and_then(|c| c.place().lines())
            .filter(|_| !outdated)
            .and_then(|l| attach(rows, l).map(|r| (r, l.side)));
        match row {
            Some((r, side)) => under.push((r, card, side)),
            None => stacked.push(card),
        }
    }
    under.sort_by_key(|&(r, ..)| r);
    let mut placed: Vec<Placed> = Vec::with_capacity(under.len());
    for (row, card, side) in under {
        let start = match placed.last() {
            Some(p) if p.row == row => p.start + p.height,
            _ => code(row),
        };
        let width = text_width(span(pane, layout, number_width, Some(side)).1);
        let height = get(session, card).map_or(0, |c| lines(&c, width, false).len());
        placed.push(Placed {
            card,
            row,
            start,
            height: u32::try_from(height).unwrap_or(1),
        });
    }
    (placed, stacked)
}

/// Where a card is drawn within a pane `pane` wide: its first column and
/// width. Row cards line up with the code of their side; stacked cards
/// (`side` `None`) take the width.
pub(crate) fn span(pane: u16, layout: Layout, number_width: u16, side: Option<Side>) -> (u16, u16) {
    let (x, end) = match (side, layout) {
        (None, _) => (2, pane.saturating_sub(2)),
        (Some(_), Layout::Unified) => (number_width * 2 + 4, pane.saturating_sub(2)),
        (Some(Side::Old), Layout::Split) => {
            let half = pane.saturating_sub(1) / 2;
            (number_width + 3, half.saturating_sub(1))
        }
        (Some(Side::New), Layout::Split) => {
            let half = pane.saturating_sub(1) / 2;
            (half + 1 + number_width + 3, pane.saturating_sub(2))
        }
    };
    (x.min(pane), end.saturating_sub(x).max(1))
}

/// Inner width of a card's text: its span less the bar and padding.
pub(crate) fn text_width(span_width: u16) -> u16 {
    span_width.saturating_sub(3)
}

/// The worktree's cards: review-level ones, and the rest by file path.
#[derive(Debug, Default)]
pub(crate) struct Index<'a> {
    pub(crate) review: Vec<CardRef>,
    pub(crate) by_path: HashMap<&'a str, Vec<CardRef>>,
}

pub(crate) fn index<'a>(session: &'a Session, worktree: &Path) -> Index<'a> {
    let mut out = Index::default();
    let all = session
        .state()
        .comments
        .iter()
        .map(|c| (&c.place, CardRef::Comment(c.id)))
        .chain(
            session
                .notes()
                .iter()
                .filter(|n| n.worktree == worktree)
                .map(|n| (&n.place, CardRef::Note(n.id))),
        );
    for (place, card) in all {
        match place.path() {
            None => out.review.push(card),
            Some(path) => out.by_path.entry(path).or_default().push(card),
        }
    }
    out
}

/// Stacks cards from line 0: each placed after the one before.
pub(crate) fn stack(
    session: &Session,
    cards: impl IntoIterator<Item = CardRef>,
    width: u16,
) -> (Vec<Placed>, u32) {
    let mut placed = Vec::new();
    let mut at = 0;
    for card in cards {
        let Some(c) = get(session, card) else {
            continue;
        };
        let height = u32::try_from(lines(&c, text_width(width), true).len()).unwrap_or(1);
        placed.push(Placed {
            card,
            row: 0,
            start: at,
            height,
        });
        at += height;
    }
    (placed, at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::Kind as CommentKind;

    fn lines_at(side: Side, start: u32, end: u32) -> review::Lines {
        review::Lines {
            side,
            start,
            end,
            text: Vec::new(),
        }
    }

    #[test]
    fn cards_attach_under_the_last_line_on_their_side() {
        let rows = [
            Row::Line {
                kind: Kind::Context,
                old: Some(0),
                new: Some(0),
            },
            Row::Line {
                kind: Kind::Delete,
                old: Some(1),
                new: None,
            },
            Row::Line {
                kind: Kind::Insert,
                old: None,
                new: Some(1),
            },
            Row::Gap { id: 1, hidden: 9 },
        ];
        assert_eq!(attach(&rows, &lines_at(Side::New, 0, 1)), Some(2));
        assert_eq!(attach(&rows, &lines_at(Side::Old, 1, 1)), Some(1));
        assert_eq!(attach(&rows, &lines_at(Side::Old, 0, 0)), Some(0));
        assert_eq!(attach(&rows, &lines_at(Side::New, 5, 5)), None);
    }

    #[test]
    fn cards_wrap_text_and_list_suggested_code() {
        let mut session = Session::default();
        let id = session.edit().add(
            Place::Lines {
                path: "a.rs".into(),
                lines: lines_at(Side::New, 2, 3),
            },
            CommentKind::Issue,
            "one two three four five\n\nsix".into(),
            Some("let x = 1;".into()),
        );
        let card = get(&session, CardRef::Comment(id)).expect("card");
        let text: Vec<String> = lines(&card, 12, true).into_iter().map(|l| l.text).collect();
        assert_eq!(
            text,
            [
                "issue · suggestion · draft · lines 3-4",
                "one two ",
                "three four ",
                "five",
                "",
                "six",
                "let x = 1;"
            ]
        );
        assert_eq!(
            lines(&card, 40, false)[0].text,
            "issue · suggestion · draft"
        );
    }

    #[test]
    fn cards_follow_their_row_and_the_rest_stack() {
        let rows = [
            Row::Line {
                kind: Kind::Context,
                old: Some(0),
                new: Some(0),
            },
            Row::Gap { id: 1, hidden: 9 },
        ];
        let mut session = Session::default();
        let mut add = |place| {
            let id = session
                .edit()
                .add(place, CommentKind::Comment, "x".into(), None);
            CardRef::Comment(id)
        };
        let line = |n| Place::Lines {
            path: "a.rs".into(),
            lines: lines_at(Side::New, n, n),
        };
        let (first, second, hidden) = (add(line(0)), add(line(0)), add(line(5)));
        let file = add(Place::File {
            path: "a.rs".into(),
        });
        let cards = [file, second, hidden, first];
        let (placed, stacked) = place(&session, &cards, &rows, |_| 2, 80, Layout::Unified, 3);
        let at: Vec<_> = placed.iter().map(|p| (p.card, p.row, p.start)).collect();
        // Each card is a title and one line, under the row's two lines of code.
        assert_eq!(at, [(second, 0, 2), (first, 0, 4)]);
        assert_eq!(stacked, [file, hidden]);
    }

    #[test]
    fn row_cards_line_up_with_their_side() {
        assert_eq!(span(100, Layout::Unified, 3, Some(Side::New)), (10, 88));
        assert_eq!(span(101, Layout::Split, 3, Some(Side::Old)), (6, 43));
        assert_eq!(span(101, Layout::Split, 3, Some(Side::New)), (57, 42));
        assert_eq!(span(100, Layout::Split, 3, None), (2, 96));
    }
}
