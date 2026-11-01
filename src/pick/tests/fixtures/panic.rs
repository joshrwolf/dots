//! A picker whose live fetch panics once the test opens a gate, for
//! terminal-restoration acceptance; never installed.
//!
//! `pick GATE` runs the picker; its watch command is `watch GATE`, this binary
//! again, which records its PID in `GATE.watch` and signals one change once
//! GATE exists.

use std::error::Error;
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dots_pick::{Cell, Column, Entry, Fetch, Group, Palette, Picker};
use ratatui::style::Color;

/// Long enough to outlive the test, short enough not to leak.
const HOLD: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct Item;

impl Entry for Item {
    type Group = ();

    fn group(&self) {}

    fn cells(&self, _palette: &Palette) -> Vec<Cell> {
        vec![Cell::new("item", Color::Reset)]
    }
}

/// Called from the fetch future, so the panic happens in the feed's job.
fn rows() -> Result<Vec<Item>, String> {
    panic!("fixture fetch invariant")
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let role = args.next().unwrap_or_default();
    let gate = args.next().ok_or("missing gate")?;
    match role.as_str() {
        "pick" => {
            let exe = std::env::current_exe()?.to_string_lossy().into_owned();
            let fetch: Fetch<Item> = Arc::new(|| Box::pin(async { rows() }));
            Picker::new(vec![Item])
                .group(Group::new((), "items", vec![Column::fill()]))
                .live(vec![exe, "watch".to_owned(), gate], fetch)
                .run()?;
            Err("the picker returned instead of resuming the fetch's panic".into())
        }
        "watch" => {
            let start = Instant::now();
            while !Path::new(&gate).exists() {
                if start.elapsed() > HOLD {
                    return Err("the gate never opened".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            std::fs::write(format!("{gate}.watch"), std::process::id().to_string())?;
            let mut stdout = std::io::stdout();
            writeln!(stdout, "changed")?;
            stdout.flush()?;
            std::thread::sleep(HOLD);
            Ok(())
        }
        _ => Err(format!("unknown role {role:?}").into()),
    }
}
