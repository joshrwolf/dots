//! Run with `cargo run -p herdrkit --example dynamic_picker` and type `#`.
//! This provider uses a cancellable timer in place of an HTTP request.
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::time::Duration;

use herdrkit::Theme;
use herdrkit::picker::{
    Cell, Column, DynamicEntry, Entry, Group, Matching, Picker, Source, SourceResult,
};

#[derive(Debug)]
struct Suggestion(String);

impl Entry for Suggestion {
    type Group = ();
    fn group(&self) {}
    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        vec![Cell::new(&self.0, theme.strong)]
    }
}

struct Suggestions;

impl Source<Suggestion> for Suggestions {
    fn query(
        &mut self,
        query: String,
    ) -> Pin<Box<dyn Future<Output = SourceResult<Suggestion>> + Send + '_>> {
        Box::pin(async move {
            // The provider owns its debounce. The picker drops this future on
            // another edit, on leaving #, and on popup exit.
            tokio::time::sleep(Duration::from_millis(300)).await;
            let labels = ["cafe\u{301}", "family 👨‍👩‍👧", "garden"];
            SourceResult::ready(
                labels
                    .into_iter()
                    .filter(|label| label.contains(&query))
                    .map(|label| DynamicEntry::new(label, Suggestion(label.to_owned())))
                    .collect(),
            )
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let limit = NonZeroUsize::new(10).ok_or("result limit must be positive")?;
    if let Some(selected) = Picker::new(Vec::new())
        .group(
            Group::new((), "suggestions", vec![Column::fill()])
                .exclusive('#')
                .matching(Matching::ProviderRanked),
        )
        .source((), limit, Suggestions)
        .run()?
    {
        println!("{}", selected.0);
    }
    Ok(())
}
