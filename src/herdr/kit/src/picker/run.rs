//! The terminal half: draw, read a key, repeat.
//!
//! The key mapping is in [`super::keys`] and the list is in [`super::model`],
//! so what remains here is init, layout and the read loop.

use std::io;
use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event};
use ratatui::layout::{Constraint, Layout, Margin};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::{Theme, columns};

use super::model::{Model, Outcome};
use super::source::Sources;
use super::{Entry, Error, Selection, Shortcut, command};

enum Exit {
    List(Outcome),
    Shortcut(usize),
}

pub(super) fn run<T: Entry + Send + 'static, A>(
    mut model: Model<T>,
    prompt: &str,
    mut sources: Sources<T>,
    mut shortcuts: Vec<Shortcut<A>>,
) -> Result<Selection<T, A>, Error> {
    let mut terminal = ratatui::try_init()?;
    let paste = match BracketedPaste::enable() {
        Ok(paste) => paste,
        Err(error) => {
            let _ = ratatui::try_restore();
            return Err(error.into());
        }
    };
    let outcome = drive(&mut terminal, &mut model, prompt, &mut sources, &shortcuts);
    // Restoring is unconditional: a loop that failed mid-draw has still left
    // the terminal in raw mode and on the alternate screen, and a popup stuck
    // there has no way out.
    let paste_restored = paste.disable();
    let restored = ratatui::try_restore();
    let stopped = sources.finish();
    let outcome = outcome?;
    stopped?;
    paste_restored?;
    restored?;

    Ok(match outcome {
        Exit::List(Outcome::Accept) => model
            .take_selected()
            .map_or(Selection::Cancelled, Selection::Entry),
        Exit::List(Outcome::Abort) => Selection::Cancelled,
        Exit::Shortcut(index) => Selection::Shortcut(shortcuts.remove(index).value),
    })
}

fn drive<T: Entry + Send + 'static, A>(
    terminal: &mut ratatui::DefaultTerminal,
    model: &mut Model<T>,
    prompt: &str,
    sources: &mut Sources<T>,
    shortcuts: &[Shortcut<A>],
) -> Result<Exit, Error> {
    let mut dirty = true;
    let mut page = 1;
    loop {
        dirty |= sources.update(model)?;
        if dirty {
            terminal.draw(|frame| page = draw(frame, model, prompt, shortcuts))?;
            dirty = false;
        }
        if !event::poll(Duration::from_millis(25))? {
            continue;
        }
        dirty = true;
        match event::read()? {
            Event::Key(key) => {
                if let Some(index) = shortcuts.iter().position(|shortcut| shortcut.matches(key)) {
                    return Ok(Exit::Shortcut(index));
                }
                if let Some(command) = command(key, page)
                    && let Some(outcome) = model.apply(command)
                {
                    return Ok(Exit::List(outcome));
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
fn draw<T: Entry, A>(
    frame: &mut Frame<'_>,
    model: &mut Model<T>,
    prompt: &str,
    shortcuts: &[Shortcut<A>],
) -> usize {
    let theme = model.theme();
    let area = frame.area();
    frame.render_widget(
        Paragraph::default().style(Style::default().bg(theme.background)),
        area,
    );
    let roomy = area.height >= 9 && area.width >= 12;
    let content = if roomy {
        area.inner(Margin::new(1, 1))
    } else {
        area
    };
    let chrome = u16::from(roomy);
    let [scope, head, gap, body, rule, footer] = Layout::vertical([
        Constraint::Length(chrome),
        Constraint::Length(1),
        Constraint::Length(chrome),
        Constraint::Min(0),
        Constraint::Length(chrome),
        Constraint::Length(2 * chrome),
    ])
    .areas(content);
    let width = usize::from(content.width);
    if roomy {
        let muted = Style::default().fg(theme.muted).bg(theme.background);
        frame.render_widget(
            Paragraph::new(format!("Scope · {}", model.scope_label())).style(muted),
            scope,
        );
        for area in [gap, rule] {
            frame.render_widget(
                Paragraph::new("─".repeat(width))
                    .style(muted.add_modifier(ratatui::style::Modifier::DIM)),
                area,
            );
        }
        let scopes = model.scope_help();
        let mut actions = String::new();
        for shortcut in shortcuts {
            actions.push_str(" · ");
            actions.push_str(&shortcut.hint());
        }
        let help = vec![
            Line::raw(format!(
                "enter open · esc close · ↑/↓ or ctrl+j/k move{actions}"
            )),
            Line::raw(format!("tab/shift+tab scope · ctrl+u clear{scopes}")),
        ];
        frame.render_widget(Paragraph::new(help).style(muted), footer);
    }
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
    if head.width > 0 && head.height > 0 {
        frame.set_cursor_position((x.min(head.right().saturating_sub(1)), head.y));
    }

    height
}

fn prompt_line<T: Entry>(
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
    use ratatui::backend::{Backend as _, TestBackend};
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
    fn chrome_and_cursor_fit_resized_terminals() {
        for width in [0, 1, 11, 12, 40, 120] {
            for height in [0, 1, 8, 9, 24] {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                let mut model = model();
                model.insert_text("cafe\u{301} 一 👩‍💻");
                terminal
                    .draw(|frame| {
                        draw(
                            frame,
                            &mut model,
                            "› ",
                            &[Shortcut::control('r', "review URL", ())],
                        );
                    })
                    .unwrap();
                if width > 0 && height > 0 {
                    let (x, y) = terminal.backend_mut().get_cursor_position().unwrap().into();
                    assert!(x < width && y < height);
                }
            }
        }
    }

    #[test]
    fn shortcut_hint_is_visible_even_when_nothing_matches() {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        let mut model = model();
        model.insert_text("no-matching-destination");
        assert_eq!(model.matched(), 0);
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    &mut model,
                    "› ",
                    &[Shortcut::control('r', "review URL", ())],
                );
            })
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(rendered.contains("ctrl+r review URL"));
    }

    #[test]
    fn page_size_is_the_complete_visible_body() {
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut model = model();
        let mut page = usize::MAX;
        terminal
            .draw(|frame| page = draw::<_, ()>(frame, &mut model, "> ", &[]))
            .unwrap();
        assert_eq!(page, 6, "one header row leaves six body rows");
    }
}
