//! A picker whose Enter action fails once, then succeeds, for in-place action
//! acceptance; it prints how the picker closed. Never installed.

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dots_pick::{Act, Actions, Cell, Column, Entry, Group, Palette, Picker};
use ratatui::style::Color;

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
    let runs = Arc::new(AtomicUsize::new(0));
    let act: Act<Item, ()> = Box::new(move |_choice| {
        let run = runs.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if run == 0 {
                Err("fixture action failed".to_owned())
            } else {
                Ok(())
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
