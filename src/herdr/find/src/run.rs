use std::path::PathBuf;

use anyhow::{Result, bail};
use herdrkit::picker::{Picker, Selection, Shortcut};
use herdrkit::{Invocation, InvocationKind};

use crate::{Sources, groups, open};

#[derive(Debug)]
enum Action {
    Review,
}

pub fn run(invocation: &Invocation) -> Result<()> {
    if !matches!(invocation.kind(), InvocationKind::PaneEntrypoint { id } if id == "picker") {
        bail!("find requires the picker entrypoint");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let destinations = Sources::collect(invocation.client())?.destinations(&home);
    match Picker::new(destinations)
        .groups(groups())
        .prompt("› ")
        .match_paths()
        .run_with_shortcuts(vec![Shortcut::control('r', "review URL", Action::Review)])?
    {
        Selection::Entry(destination) => open(invocation.client(), &destination)?,
        Selection::Shortcut(Action::Review) => herdr_review::open_pull_request(invocation)?,
        Selection::Cancelled => {}
    }
    Ok(())
}
