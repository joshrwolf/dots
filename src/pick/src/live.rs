//! Keeping a running picker's rows current.
//!
//! A host supplies a watch command and a fetch. Every line the watch command
//! prints, its first included, means the rows may have changed: the feed then
//! fetches them again. Lines arriving while a fetch runs collapse into one
//! more fetch after it, so a burst never queues work and never postpones a
//! refresh indefinitely. A failed fetch keeps the rows already shown, says so,
//! and is retried. When the watch command ends the picker says its rows are no
//! longer updating.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use dots_process::Lines;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::background::Background;
use crate::model::Model;
use crate::{Entry, Error};

type Rows<T> = Pin<Box<dyn Future<Output = Result<Vec<T>, String>> + Send>>;

/// Reads a view's rows again. Dropping the future must cancel its work.
pub type Fetch<T> = Arc<dyn Fn() -> Rows<T> + Send + Sync>;

/// Longest watch line read; watch commands print short signals.
const LINE_LIMIT: usize = 4096;
/// Wait before retrying a failed fetch that no new change has retried.
const RETRY: Duration = Duration::from_secs(2);

/// What says the rows may have changed.
pub(crate) enum Watch {
    Command(Vec<String>),
    /// Each message is a change; closing the channel ends the watch, which
    /// then takes `ending` to say how it ended.
    #[cfg(test)]
    Channel {
        changes: mpsc::UnboundedReceiver<()>,
        ending: Option<Ending>,
    },
}

pub(crate) struct Spec<T> {
    pub(crate) watch: Watch,
    pub(crate) fetch: Fetch<T>,
}

impl<T> std::fmt::Debug for Spec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spec").finish_non_exhaustive()
    }
}

/// Why the rows on screen may not be current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    /// The last refresh failed; earlier rows remain.
    Stale(String),
    /// Nothing will refresh the rows any more.
    Stopped(String),
}

enum Event<T> {
    Rows(Vec<T>),
    Failed(String),
    Ended(String),
}

pub(crate) struct Feed<T> {
    events: mpsc::UnboundedReceiver<Event<T>>,
    notice: Option<Notice>,
    replaced: bool,
}

impl<T> std::fmt::Debug for Feed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Feed")
            .field("notice", &self.notice)
            .finish_non_exhaustive()
    }
}

impl<T: Entry + Send + 'static> Feed<T> {
    /// Starts the feed on `background`. It ends when this is dropped.
    pub(crate) fn spawn(background: &mut Background, spec: Spec<T>) -> Self {
        let (events, receiver) = mpsc::unbounded_channel();
        background.spawn(serve(spec, events));
        Self {
            events: receiver,
            notice: None,
            replaced: false,
        }
    }

    /// Applies the newest rows and any change in notice. Returns whether the
    /// screen changed.
    pub(crate) fn update(&mut self, model: &mut Model<T>) -> bool {
        let mut rows = None;
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Rows(batch) => rows = Some(batch),
                Event::Failed(message) => changed |= self.set(Notice::Stale(message)),
                Event::Ended(message) => changed |= self.set(Notice::Stopped(message)),
            }
        }
        if let Some(rows) = rows {
            changed = true;
            match model.replace_all(rows) {
                Ok(()) => {
                    self.replaced = true;
                    if matches!(self.notice, Some(Notice::Stale(_))) {
                        self.notice = None;
                    }
                }
                Err(error) => {
                    self.set(Notice::Stale(rejected(&error)));
                }
            }
        }
        changed
    }

    /// Take the signal that a successful fetch replaced the rows.
    pub(crate) fn take_replaced(&mut self) -> bool {
        std::mem::take(&mut self.replaced)
    }

    pub(crate) fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    fn set(&mut self, notice: Notice) -> bool {
        // A stopped feed stays stopped; a later failure must not hide that.
        if matches!(self.notice, Some(Notice::Stopped(_))) && matches!(notice, Notice::Stale(_)) {
            return false;
        }
        let changed = self.notice.as_ref() != Some(&notice);
        self.notice = Some(notice);
        changed
    }
}

fn rejected(error: &Error) -> String {
    format!("refreshed rows were rejected: {error}")
}

enum Step<T> {
    Changed,
    WatchEnded(String),
    Fetched(Result<Vec<T>, String>),
    Retry,
}

async fn serve<T: Send + 'static>(spec: Spec<T>, events: mpsc::UnboundedSender<Event<T>>) {
    let mut watch = match spec.watch {
        Watch::Command(argv) => match start(&argv) {
            Ok(lines) => Source::Lines(Box::new(lines)),
            Err(message) => {
                let _ = events.send(Event::Ended(message));
                return;
            }
        },
        #[cfg(test)]
        Watch::Channel { changes, ending } => Source::Channel { changes, ending },
    };
    let mut open = true;
    let mut wanted = false;
    let mut running: Option<Rows<T>> = None;
    let mut retry: Option<Instant> = None;
    loop {
        if wanted && running.is_none() {
            running = Some((spec.fetch)());
            wanted = false;
        }
        if !open && running.is_none() && retry.is_none() {
            return;
        }
        let step = tokio::select! {
            step = watch.next(), if open => step,
            fetched = settle(&mut running), if running.is_some() => Step::Fetched(fetched),
            () = sleep_until(retry), if retry.is_some() => Step::Retry,
        };
        match step {
            Step::Changed => wanted = true,
            Step::WatchEnded(message) => {
                open = false;
                if events.send(Event::Ended(message)).is_err() {
                    return;
                }
            }
            Step::Fetched(result) => {
                running = None;
                let event = match result {
                    Ok(rows) => {
                        retry = None;
                        Event::Rows(rows)
                    }
                    Err(message) => {
                        retry = Some(Instant::now() + RETRY);
                        Event::Failed(message)
                    }
                };
                if events.send(event).is_err() {
                    return;
                }
            }
            Step::Retry => {
                retry = None;
                wanted = true;
            }
        }
    }
}

fn start(command: &[String]) -> Result<Lines, String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| "the watch command is empty".to_owned())?;
    let mut command = std::process::Command::new(program);
    command.args(args);
    Lines::spawn(command, LINE_LIMIT).map_err(|error| format!("starting {program}: {error}"))
}

/// How a watch is reporting how it ended, kept across `select!` iterations.
type Ending = Pin<Box<dyn Future<Output = String> + Send>>;

enum Source {
    Lines(Box<Lines>),
    /// Stdout closed; waiting for the command to finish. Owned here, so a
    /// fetch finishing first cannot lose it, and dropping the feed drops it,
    /// which kills the command's group.
    Ending(Ending),
    Ended,
    #[cfg(test)]
    Channel {
        changes: mpsc::UnboundedReceiver<()>,
        ending: Option<Ending>,
    },
}

impl Source {
    /// Cancel safe, so it can lose a `select!` without losing a line or how
    /// the watch ended.
    async fn next<T>(&mut self) -> Step<T> {
        loop {
            match self {
                Self::Lines(lines) => match lines.next_line().await {
                    Ok(Some(_)) => return Step::Changed,
                    Ok(None) => {
                        let Self::Lines(lines) = std::mem::replace(self, Self::Ended) else {
                            unreachable!("the source was just matched as lines");
                        };
                        *self = Self::Ending(Box::pin(exited(*lines)));
                    }
                    Err(error) => {
                        *self = Self::Ended;
                        return Step::WatchEnded(format!("reading the watch command: {error}"));
                    }
                },
                Self::Ending(ending) => {
                    let message = ending.as_mut().await;
                    *self = Self::Ended;
                    return Step::WatchEnded(message);
                }
                Self::Ended => return std::future::pending().await,
                #[cfg(test)]
                Self::Channel { changes, ending } => {
                    if changes.recv().await.is_some() {
                        return Step::Changed;
                    }
                    let ending = ending
                        .take()
                        .unwrap_or_else(|| Box::pin(async { "the watch closed".to_owned() }));
                    *self = Self::Ending(ending);
                }
            }
        }
    }
}

async fn exited(lines: Lines) -> String {
    match lines.wait().await {
        Ok(ended) => {
            let last = ended.stderr.lines().last().unwrap_or_default().to_owned();
            if last.is_empty() {
                format!("the watch command exited ({})", ended.status)
            } else {
                format!("the watch command exited ({}): {last}", ended.status)
            }
        }
        Err(error) => format!("the watch command stopped: {error}"),
    }
}

async fn settle<T>(running: &mut Option<Rows<T>>) -> Result<Vec<T>, String> {
    match running {
        Some(rows) => rows.await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ratatui::style::Color;

    use super::*;
    use crate::background::Background;
    use crate::{Cell, Column, Group, Palette, Picker};

    #[derive(Debug, Clone)]
    struct Item(&'static str);

    impl Entry for Item {
        type Group = ();
        fn group(&self) {}
        fn cells(&self, _palette: &Palette) -> Vec<Cell> {
            vec![Cell::new(self.0, Color::White)]
        }
        fn identity(&self) -> Option<Cow<'_, str>> {
            (!self.0.is_empty()).then_some(Cow::Borrowed(self.0))
        }
    }

    fn model(items: &[&'static str]) -> Model<Item> {
        Picker::new(items.iter().map(|name| Item(name)).collect())
            .group(Group::new((), "items", vec![Column::fill()]))
            .build()
            .unwrap()
    }

    /// A fetch that answers from a script of results, counting its calls.
    fn scripted(results: Vec<Result<Vec<Item>, String>>) -> (Fetch<Item>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let script = Arc::new(Mutex::new(results.into_iter()));
        let counter = Arc::clone(&calls);
        let fetch: Fetch<Item> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let next = script
                .lock()
                .unwrap()
                .next()
                .expect("the feed fetched more often than the test scripted");
            Box::pin(async move { next })
        });
        (fetch, calls)
    }

    fn feed(fetch: Fetch<Item>) -> (Background, Feed<Item>, mpsc::UnboundedSender<()>) {
        feed_ending(fetch, None)
    }

    /// A feed whose watch, once its channel closes, reports through `ending`.
    fn feed_ending(
        fetch: Fetch<Item>,
        ending: Option<Ending>,
    ) -> (Background, Feed<Item>, mpsc::UnboundedSender<()>) {
        let mut background = Background::start().unwrap();
        let (changes, receiver) = mpsc::unbounded_channel();
        let feed = Feed::spawn(
            &mut background,
            Spec {
                watch: Watch::Channel {
                    changes: receiver,
                    ending,
                },
                fetch,
            },
        );
        (background, feed, changes)
    }

    /// Applies updates until `done`, failing on a timeout or a failed job.
    fn until(
        background: &mut Background,
        feed: &mut Feed<Item>,
        model: &mut Model<Item>,
        done: impl Fn(&Feed<Item>, &Model<Item>) -> bool,
    ) {
        let start = std::time::Instant::now();
        while !done(feed, model) {
            assert!(!background.failed(), "a background job panicked");
            feed.update(model);
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "{:?}",
                feed.notice()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn selected(model: &Model<Item>) -> Option<&'static str> {
        model.selected_entry().map(|item| item.0)
    }

    #[test]
    fn each_change_fetches_and_replaces_the_rows() {
        let (fetch, calls) = scripted(vec![Ok(vec![Item("a"), Item("b")])]);
        let (mut background, mut feed, changes) = feed(fetch);
        let mut model = model(&["old"]);
        changes.send(()).unwrap();
        until(&mut background, &mut feed, &mut model, |_, model| {
            model.candidates() == 2
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(feed.take_replaced());
        assert!(!feed.take_replaced());
        assert_eq!(feed.notice(), None);
    }

    #[test]
    fn a_burst_while_fetching_collapses_into_one_more_fetch() {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let (opened, counter) = (Arc::clone(&gate), Arc::clone(&calls));
        let fetch: Fetch<Item> = Arc::new(move || {
            let call = counter.fetch_add(1, Ordering::SeqCst);
            let gate = Arc::clone(&opened);
            Box::pin(async move {
                if call == 0 {
                    let _permit = gate.acquire().await;
                }
                Ok(vec![Item("a")])
            })
        });
        let (mut background, mut feed, changes) = feed(fetch);
        let mut model = model(&[]);
        changes.send(()).unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        for _ in 0..5 {
            changes.send(()).unwrap();
        }
        std::thread::sleep(Duration::from_millis(30));
        gate.add_permits(1);
        until(&mut background, &mut feed, &mut model, |_, model| {
            model.candidates() == 1
        });
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "the burst became one fetch"
        );
    }

    #[test]
    fn a_failed_fetch_keeps_the_rows_says_so_and_retries() {
        let (fetch, calls) = scripted(vec![Err("rex is down".into()), Ok(vec![Item("x")])]);
        let (mut background, mut feed, changes) = feed(fetch);
        let mut model = model(&["kept"]);
        changes.send(()).unwrap();
        until(&mut background, &mut feed, &mut model, |feed, _| {
            feed.notice().is_some()
        });
        assert_eq!(feed.notice(), Some(&Notice::Stale("rex is down".into())));
        assert_eq!(selected(&model), Some("kept"));
        // No new change arrives; the retry alone brings the rows.
        until(&mut background, &mut feed, &mut model, |_, model| {
            selected(model) == Some("x")
        });
        assert_eq!(feed.notice(), None);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn rows_the_picker_rejects_are_a_stale_notice_not_a_new_list() {
        let (fetch, _) = scripted(vec![Ok(vec![Item("same"), Item("same")])]);
        let (mut background, mut feed, changes) = feed(fetch);
        let mut model = model(&["kept"]);
        changes.send(()).unwrap();
        until(&mut background, &mut feed, &mut model, |feed, _| {
            feed.notice().is_some()
        });
        assert!(
            matches!(feed.notice(), Some(Notice::Stale(message)) if message.contains("rejected"))
        );
        assert_eq!(selected(&model), Some("kept"));
        assert!(!feed.take_replaced());
    }

    #[test]
    fn an_ended_watch_says_the_rows_no_longer_update() {
        let (fetch, _) = scripted(Vec::new());
        let (mut background, mut feed, changes) = feed(fetch);
        let mut model = model(&["kept"]);
        drop(changes);
        until(&mut background, &mut feed, &mut model, |feed, _| {
            feed.notice().is_some()
        });
        assert!(matches!(feed.notice(), Some(Notice::Stopped(_))));
    }

    /// A dropped value that records it was dropped.
    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_watch_still_ending_neither_blocks_fetches_nor_shutdown() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let flag = Dropped(Arc::clone(&dropped));
        // Like a watch whose stderr a descendant keeps open: it never ends.
        let ending: Ending = Box::pin(async move {
            let _flag = flag;
            std::future::pending().await
        });
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let (opened, counter) = (Arc::clone(&gate), Arc::clone(&calls));
        let fetch: Fetch<Item> = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let gate = Arc::clone(&opened);
            Box::pin(async move {
                let _permit = gate.acquire().await;
                Ok(vec![Item("fresh")])
            })
        });
        let (mut background, mut feed, changes) = feed_ending(fetch, Some(ending));
        let mut model = model(&["old"]);
        changes.send(()).unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            std::thread::yield_now();
        }
        // The watch ends while its fetch runs; the fetch still lands.
        drop(changes);
        std::thread::sleep(Duration::from_millis(30));
        gate.add_permits(1);
        until(&mut background, &mut feed, &mut model, |_, model| {
            selected(model) == Some("fresh")
        });
        assert_eq!(feed.notice(), None, "the watch has not finished ending");
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        drop(feed);
        let started = std::time::Instant::now();
        assert!(background.shutdown().is_ok());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(dropped.load(Ordering::SeqCst), 1, "shutdown dropped it");
    }

    #[test]
    fn a_watch_command_that_exits_after_a_change_says_it_stopped() {
        let (fetch, calls) = scripted(vec![Ok(vec![Item("fresh")])]);
        let mut background = Background::start().unwrap();
        let mut feed = Feed::spawn(
            &mut background,
            Spec {
                watch: Watch::Command(vec!["/bin/echo".to_owned(), "changed".to_owned()]),
                fetch,
            },
        );
        let mut model = model(&["old"]);
        until(&mut background, &mut feed, &mut model, |feed, model| {
            selected(model) == Some("fresh") && feed.notice().is_some()
        });
        assert!(
            matches!(feed.notice(), Some(Notice::Stopped(message)) if message.contains("exited")),
            "{:?}",
            feed.notice()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_fetch_that_panics_is_a_failed_job_not_a_stale_feed() {
        let fetch: Fetch<Item> = Arc::new(|| -> Rows<Item> { panic!("fetch invariant") });
        let (mut background, mut feed, changes) = feed(fetch);
        let mut model = model(&["kept"]);
        changes.send(()).unwrap();
        let start = std::time::Instant::now();
        while !background.failed() {
            assert!(start.elapsed() < Duration::from_secs(5), "never reported");
            std::thread::sleep(Duration::from_millis(5));
        }
        // The feed alone would look quiet: its events just stop.
        assert!(!feed.update(&mut model));
        assert_eq!(feed.notice(), None);
        let panic = background.shutdown().unwrap_err();
        assert_eq!(panic.downcast_ref::<&str>(), Some(&"fetch invariant"));
    }
}
