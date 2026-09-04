//! The terminal half: draw, read a key, repeat.
//!
//! The key mapping is in [`super::keys`] and the list is in [`super::model`],
//! so what remains here is init, layout and the read loop.

use std::io;

use ratatui::Frame;
use ratatui::crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::{Theme, columns};

use super::model::{Model, Outcome};
use super::{Entry, Error, command};

pub(super) fn run<T: Entry>(mut model: Model<T>, prompt: &str) -> Result<Option<T>, Error> {
    let mut terminal = ratatui::try_init()?;
    let paste = match BracketedPaste::enable() {
        Ok(paste) => paste,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(error.into());
        }
    };
    let outcome = drive(&mut terminal, &mut model, prompt);
    // Restoring is unconditional: a loop that failed mid-draw has still left
    // the terminal in raw mode and on the alternate screen, and a popup stuck
    // there has no way out.
    let paste_restored = paste.disable();
    let restored = ratatui::try_restore();
    let outcome = outcome?;
    paste_restored?;
    restored?;

    Ok(match outcome {
        Outcome::Accept => model.take_selected(),
        Outcome::Abort => None,
    })
}

fn drive<T>(
    terminal: &mut ratatui::DefaultTerminal,
    model: &mut Model<T>,
    prompt: &str,
) -> std::io::Result<Outcome> {
    loop {
        let mut page = 1;
        terminal.draw(|frame| page = draw(frame, model, prompt))?;
        match event::read()? {
            Event::Key(key) => {
                if let Some(command) = command(key, page)
                    && let Some(outcome) = model.apply(command)
                {
                    return Ok(outcome);
                }
            }
            Event::Paste(text) => model.insert_text(&text),
            _ => {}
        }
    }
}

/// Restores bracketed paste even when drawing or reading fails. Keeping this
/// separate from ratatui's terminal guard matters because ratatui does not
/// enable or disable this crossterm mode itself.
struct BracketedPaste {
    enabled: bool,
}

impl BracketedPaste {
    fn enable() -> io::Result<Self> {
        ratatui::crossterm::execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(Self { enabled: true })
    }

    fn disable(mut self) -> io::Result<()> {
        ratatui::crossterm::execute!(io::stdout(), DisableBracketedPaste)?;
        self.enabled = false;
        Ok(())
    }
}

impl Drop for BracketedPaste {
    fn drop(&mut self) {
        if self.enabled {
            let _ = ratatui::crossterm::execute!(io::stdout(), DisableBracketedPaste);
        }
    }
}

/// Returns the page size, so a page key moves by exactly what is on screen.
fn draw<T>(frame: &mut Frame<'_>, model: &mut Model<T>, prompt: &str) -> usize {
    let theme = model.theme();
    let area = frame.area();
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let width = usize::from(area.width);
    let height = usize::from(body.height);

    model.scroll_into_view(height);
    frame.render_widget(
        Paragraph::new(model.lines(width, height)).style(Style::default().bg(theme.background)),
        body,
    );
    let caret = columns::width(prompt) + model.caret_cells();
    let (prompt_line, input_width) = prompt_line(model, prompt, theme, width);
    frame.render_widget(
        Paragraph::new(prompt_line).style(Style::default().bg(theme.background)),
        head,
    );

    let caret = caret.min(input_width.saturating_sub(1));
    let x = head
        .x
        .saturating_add(u16::try_from(caret).unwrap_or(u16::MAX));
    frame.set_cursor_position((x.min(head.right().saturating_sub(1)), head.y));

    height
}

fn prompt_line<T>(
    model: &Model<T>,
    prompt: &str,
    theme: Theme,
    width: usize,
) -> (Line<'static>, usize) {
    let count = format!("{}/{}", model.matched(), model.candidates());
    let prompt_width = columns::width(prompt);
    let count_width = columns::width(&count);
    // The count is secondary to an editable prompt. Only reserve it when the
    // complete prompt and the separating cell fit as well.
    let show_count = prompt_width.saturating_add(1).saturating_add(count_width) <= width;
    let input_width = if show_count {
        width.saturating_sub(count_width + 1)
    } else {
        width
    };

    let shown_prompt = columns::truncate(prompt, input_width);
    let query_width = input_width.saturating_sub(columns::width(&shown_prompt));
    let shown_query = columns::truncate(model.query(), query_width);
    let used = columns::width(&shown_prompt) + columns::width(&shown_query);
    let padding = input_width.saturating_sub(used) + usize::from(show_count);

    let mut spans = vec![
        Span::styled(shown_prompt.into_owned(), Style::default().fg(theme.blue)),
        Span::styled(shown_query.into_owned(), Style::default().fg(theme.strong)),
        Span::raw(" ".repeat(padding)),
    ];
    if show_count {
        spans.push(Span::styled(count, Style::default().fg(theme.muted)));
    }
    (Line::from(spans), input_width)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    use super::*;
    use crate::picker::{Cell, Column, Group, Picker};

    #[derive(Debug)]
    struct Item;

    impl Entry for Item {
        type Group = ();

        fn group(&self) {}

        fn cells(&self, _theme: &Theme) -> Vec<Cell> {
            vec![Cell::new("something searchable", Color::White)]
        }
    }

    fn model() -> Model<Item> {
        Picker::new(vec![Item])
            .group(Group::new((), "items", vec![Column::fill()]))
            .build()
            .unwrap()
    }

    #[test]
    fn prompt_never_exceeds_the_terminal_width() {
        let mut model = model();
        for ch in "a long query".chars() {
            model.apply(super::super::Command::Insert(ch));
        }
        for width in 0..20 {
            let (line, input_width) = prompt_line(&model, "> ", model.theme(), width);
            let drawn: usize = line
                .spans
                .iter()
                .map(|span| columns::width(&span.content))
                .sum();
            assert_eq!(drawn, width, "prompt at width {width}");
            assert!(input_width <= width);
        }
    }

    #[test]
    fn page_size_is_the_complete_visible_body() {
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut model = model();
        let mut page = usize::MAX;
        terminal
            .draw(|frame| page = draw(frame, &mut model, "> "))
            .unwrap();
        assert_eq!(page, 6, "one header row leaves six body rows");
    }
}
