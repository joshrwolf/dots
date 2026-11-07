//! A picker whose Enter action fails once, then succeeds, for in-place action
//! acceptance; it prints how the picker closed. Never installed.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dots_pick::{
    Act, Actions, Cell, Column, Entry, Group, Model, Palette, Pane, PaneLayout, Picker, Response,
    Task,
};
use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Paragraph;

#[derive(Debug)]
struct Composer {
    queued: Vec<Task<()>>,
    editing: bool,
    draft: String,
    saved: usize,
}

impl Pane<Item> for Composer {
    type Message = ();
    fn layout(&self) -> PaneLayout {
        PaneLayout {
            list_width: 24,
            collapse_below: 60,
        }
    }
    fn poll(&mut self, _: std::time::Instant) -> Vec<Task<()>> {
        std::mem::take(&mut self.queued)
    }
    fn update(&mut self, (): (), _: &mut Model<Item>) -> Response {
        self.saved += 1;
        self.editing = false;
        Response::Redraw
    }
    fn event(&mut self, event: &Event) -> Response {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc => return Response::Close,
                KeyCode::Enter if !self.editing => self.editing = true,
                KeyCode::Enter => self.queued.push(Task::action("saving", async {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                })),
                KeyCode::Char(c) if self.editing => self.draft.push(c),
                _ => return Response::Ignored,
            },
            Event::Paste(text) if self.editing => self.draft.push_str(text),
            _ => return Response::Ignored,
        }
        Response::Redraw
    }
    fn focused(&self) -> bool {
        self.editing
    }
    fn draw(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect, _: Palette) {
        frame.render_widget(
            Paragraph::new(format!(
                "Original question\nDraft: {}\n{}",
                self.draft,
                if self.saved == 0 {
                    "Not sent yet".to_owned()
                } else {
                    format!("CONFIRMED {}", self.saved)
                }
            )),
            area,
        );
    }
}

fn pane() -> Result<(), Box<dyn Error>> {
    let mut view = Picker::new(vec![Item])
        .group(Group::new((), "requests", vec![Column::fill()]))
        .into_view()?;
    let mut composer = Composer {
        queued: vec![Task::background(std::future::pending())],
        editing: false,
        draft: String::new(),
        saved: 0,
    };
    view.run_pane(&mut composer)?;
    println!(
        "query={} draft={} saved={}",
        view.model().query(),
        composer.draft,
        composer.saved
    );
    Ok(())
}

#[derive(Debug)]
struct Item;

impl Entry for Item {
    type Group = ();

    fn group(&self) {}

    fn cells(&self, _palette: &Palette) -> Vec<Cell> {
        vec![Cell::new("item", Color::Reset)]
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() == Some("pane") {
        return pane();
    }
    let runs = Arc::new(AtomicUsize::new(0));
    let act: Act<Item, ()> = Box::new(move |_choice| {
        let run = runs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if run == 0 {
                Err("fixture action failed".to_owned())
            } else {
                Ok(dots_pick::Action::Close)
            }
        })
    });
    let closed = Picker::new(vec![Item])
        .group(Group::new((), "items", vec![Column::fill()]))
        .run_actions(&Actions {
            shortcuts: Vec::new(),
            act,
        })?;
    println!("closed: {closed:?}");
    Ok(())
}
