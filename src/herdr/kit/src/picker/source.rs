//! Cancellable, bounded providers for explicitly activated picker scopes.
//!
//! Providers own request policy (debouncing, caching, pagination and quotas).
//! The picker owns activation and drops a query future as soon as its scope or
//! query changes. A provider must use cancellation-safe asynchronous I/O and
//! must not block while constructing or polling its future.

use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use tokio::sync::{mpsc, watch};

use super::{DynamicEntry, Entry, Error, Model, SourceStatus};

/// The complete replacement for one provider's group, including inert chrome.
#[derive(Debug)]
pub struct SourceResult<T> {
    pub entries: Vec<DynamicEntry<T>>,
    pub status: SourceStatus,
}

impl<T> SourceResult<T> {
    pub fn ready(entries: Vec<DynamicEntry<T>>) -> Self {
        let status = if entries.is_empty() {
            SourceStatus::Empty("No results".to_owned())
        } else {
            SourceStatus::Ready
        };
        Self { entries, status }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            entries: Vec::new(),
            status: SourceStatus::Error(message.into()),
        }
    }
}

/// A provider can retain its cache and rate budget across cancelled queries.
///
/// Construction must be inert: only `query` may access credentials or perform
/// I/O. Dropping the returned future cancels its work, so use async I/O rather
/// than detached tasks or blocking calls. Resources requiring cleanup belong
/// to guards owned by that future or by the provider itself.
pub trait Source<T>: Send {
    fn query(
        &mut self,
        query: String,
    ) -> Pin<Box<dyn Future<Output = SourceResult<T>> + Send + '_>>;
}

pub(super) struct Registration<T: Entry> {
    pub group: T::Group,
    pub limit: NonZeroUsize,
    pub provider: Box<dyn Source<T>>,
}

impl<T: Entry> std::fmt::Debug for Registration<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registration")
            .field("group", &self.group)
            .field("limit", &self.limit)
            .finish_non_exhaustive()
    }
}

pub(super) fn validate<T: Entry>(
    registrations: &[Registration<T>],
    model: &Model<T>,
) -> Result<(), Error> {
    for (index, registration) in registrations.iter().enumerate() {
        if !model.supports_source(registration.group) {
            return Err(Error::SourceConfiguration {
                message: format!(
                    "source group {:?} requires an exclusive scope",
                    registration.group
                ),
            });
        }
        if registrations
            .iter()
            .take(index)
            .any(|earlier| earlier.group == registration.group)
        {
            return Err(Error::SourceConfiguration {
                message: format!("source group {:?} is registered twice", registration.group),
            });
        }
    }
    Ok(())
}

/// A token belongs to exactly one query activation, including exit/re-entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Generation(u64);

#[derive(Debug, Clone)]
struct Request {
    generation: Generation,
    query: String,
}

struct Response<T> {
    source: usize,
    generation: Generation,
    result: SourceResult<T>,
}

struct Slot<G> {
    group: G,
    limit: NonZeroUsize,
    request: watch::Sender<Option<Request>>,
    active: Option<Request>,
    generation: u64,
}

/// One worker thread hosts all providers; request slots coalesce edits and the
/// result queue capacity equals the number of sources. Dropping this owner closes
/// the request slots, cancels every future, and joins the runtime thread.
pub(super) struct Sources<T: Entry> {
    slots: Vec<Slot<T::Group>>,
    results: mpsc::Receiver<Response<T>>,
    worker: Option<thread::JoinHandle<()>>,
    failed: Arc<AtomicBool>,
}

impl<T: Entry + Send + 'static> Sources<T> {
    pub(super) fn start(
        registrations: Vec<Registration<T>>,
        model: &Model<T>,
    ) -> Result<Self, Error> {
        validate(&registrations, model)?;
        let (results_tx, results) = mpsc::channel(registrations.len().max(1));
        let mut slots = Vec::with_capacity(registrations.len());
        let mut providers = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let (request, receiver) = watch::channel(None);
            slots.push(Slot {
                group: registration.group,
                limit: registration.limit,
                request,
                active: None,
                generation: 0,
            });
            providers.push((registration.provider, receiver, registration.limit));
        }
        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = Arc::clone(&failed);
        let worker = if providers.is_empty() {
            None
        } else {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            Some(
                thread::Builder::new()
                    .name("picker-sources".to_owned())
                    .spawn(move || {
                        runtime.block_on(async move {
                            let mut tasks = tokio::task::JoinSet::new();
                            for (index, (provider, requests, limit)) in
                                providers.into_iter().enumerate()
                            {
                                tasks.spawn(serve(
                                    index,
                                    provider,
                                    requests,
                                    results_tx.clone(),
                                    limit,
                                ));
                            }
                            drop(results_tx);
                            while let Some(result) = tasks.join_next().await {
                                if result.is_err() {
                                    worker_failed.store(true, Ordering::Release);
                                    tasks.abort_all();
                                }
                            }
                        });
                    })?,
            )
        };
        Ok(Self {
            slots,
            results,
            worker,
            failed,
        })
    }

    /// Reconcile scopes before receiving results, so leaving a scope always
    /// invalidates even a completed response already waiting in the queue.
    pub(super) fn update(&mut self, model: &mut Model<T>) -> Result<bool, Error> {
        if self.failed.load(Ordering::Acquire) {
            return Err(Error::SourcePanicked);
        }
        let mut changed = false;
        for slot in &mut self.slots {
            let query = model.scoped_query(slot.group).map(str::to_owned);
            if query.as_deref() == slot.active.as_ref().map(|request| request.query.as_str()) {
                continue;
            }
            changed = true;
            slot.generation =
                slot.generation
                    .checked_add(1)
                    .ok_or_else(|| Error::SourceConfiguration {
                        message: "picker source exhausted its query generations".to_owned(),
                    })?;
            slot.active = query.map(|query| Request {
                generation: Generation(slot.generation),
                query,
            });
            slot.request.send_replace(slot.active.clone());
            model.replace_group(slot.group, Vec::new(), slot.limit)?;
            model.set_status(
                slot.group,
                if slot.active.is_some() {
                    SourceStatus::Loading
                } else {
                    SourceStatus::Ready
                },
            )?;
        }
        // Never let a busy provider monopolize the terminal event loop.
        for _ in 0..self.slots.len() {
            let Ok(response) = self.results.try_recv() else {
                break;
            };
            let Some(slot) = self.slots.get(response.source) else {
                continue;
            };
            if slot.active.as_ref().map(|request| request.generation) != Some(response.generation) {
                continue;
            }
            changed = true;
            match model.replace_group(slot.group, response.result.entries, slot.limit) {
                Ok(()) => model.set_status(slot.group, response.result.status)?,
                Err(error) => {
                    model.set_status(slot.group, SourceStatus::Error(error.to_string()))?;
                }
            }
        }
        Ok(changed)
    }

    pub(super) fn finish(&mut self) -> Result<(), Error> {
        self.slots.clear();
        self.results.close();
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            self.failed.store(true, Ordering::Release);
        }
        if self.failed.load(Ordering::Acquire) {
            Err(Error::SourcePanicked)
        } else {
            Ok(())
        }
    }
}

impl<T: Entry> Drop for Sources<T> {
    fn drop(&mut self) {
        self.slots.clear();
        self.results.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn serve<T: Send + 'static>(
    source: usize,
    mut provider: Box<dyn Source<T>>,
    mut requests: watch::Receiver<Option<Request>>,
    results: mpsc::Sender<Response<T>>,
    limit: NonZeroUsize,
) {
    loop {
        if requests.changed().await.is_err() {
            return;
        }
        loop {
            let request = requests.borrow_and_update().clone();
            let Some(request) = request else { break };
            let result = tokio::select! {
                biased;
                changed = requests.changed() => {
                    if changed.is_err() { return; }
                    continue;
                }
                result = provider.query(request.query.clone()) => result,
            };
            let result = if result.entries.len() > limit.get() {
                SourceResult::error(format!("Source returned more than {} results", limit.get()))
            } else {
                result
            };
            let response = Response {
                source,
                generation: request.generation,
                result,
            };
            tokio::select! {
                biased;
                changed = requests.changed() => {
                    if changed.is_err() { return; }
                }
                sent = results.send(response) => {
                    if sent.is_err() { return; }
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::Theme;
    use crate::picker::{Cell, Column, Command, Group, Matching, Picker};

    #[derive(Debug)]
    struct Item(String);

    impl Entry for Item {
        type Group = bool;
        fn group(&self) -> bool {
            true
        }
        fn cells(&self, theme: &Theme) -> Vec<Cell> {
            vec![Cell::new(&self.0, theme.strong)]
        }
    }

    struct Pending {
        calls: Arc<Mutex<Vec<String>>>,
        dropped: Arc<AtomicUsize>,
    }

    struct Guard(Arc<AtomicUsize>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Source<Item> for Pending {
        fn query(
            &mut self,
            query: String,
        ) -> Pin<Box<dyn Future<Output = SourceResult<Item>> + Send + '_>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(query);
                let _guard = Guard(Arc::clone(&self.dropped));
                std::future::pending().await
            })
        }
    }

    fn model() -> Model<Item> {
        Picker::new(Vec::new())
            .groups([
                Group::new(false, "local", vec![Column::fill()]),
                Group::new(true, "remote", vec![Column::fill()])
                    .exclusive('#')
                    .matching(Matching::ProviderRanked),
            ])
            .build()
            .unwrap()
    }

    fn registration(provider: impl Source<Item> + 'static) -> Registration<Item> {
        Registration {
            group: true,
            limit: NonZeroUsize::new(3).unwrap(),
            provider: Box::new(provider),
        }
    }

    fn eventually(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !condition() {
            assert!(Instant::now() < deadline, "source worker missed deadline");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn explicit_activation_cancels_on_edit_exit_and_shutdown() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut model = model();
        let mut sources = Sources::start(
            vec![registration(Pending {
                calls: Arc::clone(&calls),
                dropped: Arc::clone(&dropped),
            })],
            &model,
        )
        .unwrap();
        sources.update(&mut model).unwrap();
        for _ in 0..8 {
            model.apply(Command::NextScope);
            sources.update(&mut model).unwrap();
        }
        assert!(calls.lock().unwrap().is_empty());
        model.insert_text("#one");
        sources.update(&mut model).unwrap();
        eventually(|| calls.lock().unwrap().len() == 1);
        model.apply(Command::Insert('x'));
        sources.update(&mut model).unwrap();
        eventually(|| dropped.load(Ordering::SeqCst) == 1 && calls.lock().unwrap().len() == 2);
        model.apply(Command::ClearQuery);
        sources.update(&mut model).unwrap();
        eventually(|| dropped.load(Ordering::SeqCst) == 2);
        model.insert_text("#onex");
        sources.update(&mut model).unwrap();
        eventually(|| calls.lock().unwrap().len() == 3);
        drop(sources);
        assert_eq!(dropped.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn queued_results_cannot_cross_scope_reentry_with_identical_text() {
        let mut model = model();
        let (request, _receiver) = watch::channel(None);
        let (results_tx, results) = mpsc::channel(2);
        let mut sources = Sources {
            slots: vec![Slot {
                group: true,
                limit: NonZeroUsize::new(3).unwrap(),
                request,
                active: None,
                generation: 0,
            }],
            results,
            worker: None,
            failed: Arc::new(AtomicBool::new(false)),
        };
        model.insert_text("#old");
        sources.update(&mut model).unwrap();
        let old = sources.slots.first().unwrap().active.clone().unwrap();
        model.apply(Command::ClearQuery);
        sources.update(&mut model).unwrap();
        model.insert_text("#old");
        results_tx
            .try_send(Response {
                source: 0,
                generation: old.generation,
                result: SourceResult::ready(vec![DynamicEntry::new(
                    "old",
                    Item("stale".to_owned()),
                )]),
            })
            .unwrap_or_else(|_| panic!("empty result queue"));
        sources.update(&mut model).unwrap();
        assert_eq!(model.matched(), 0);
        let current = sources.slots.first().unwrap().active.clone().unwrap();
        results_tx
            .try_send(Response {
                source: 0,
                generation: current.generation,
                result: SourceResult::ready(vec![DynamicEntry::new(
                    "new",
                    Item("cafe\u{301} 👨‍👩‍👧".to_owned()),
                )]),
            })
            .unwrap_or_else(|_| panic!("empty result queue"));
        sources.update(&mut model).unwrap();
        assert_eq!(model.matched(), 1);
        assert_eq!(model.take_selected().unwrap().0, "cafe\u{301} 👨‍👩‍👧");
    }

    #[test]
    fn request_slots_coalesce_a_burst_before_the_worker_runs() {
        let (sender, mut receiver) = watch::channel(None);
        for generation in 0..1000 {
            sender.send_replace(Some(Request {
                generation: Generation(generation),
                query: generation.to_string(),
            }));
        }
        assert_eq!(receiver.borrow_and_update().as_ref().unwrap().query, "999");
        assert!(!receiver.has_changed().unwrap());
    }

    struct Immediate;
    impl Source<Item> for Immediate {
        fn query(
            &mut self,
            query: String,
        ) -> Pin<Box<dyn Future<Output = SourceResult<Item>> + Send + '_>> {
            Box::pin(
                async move { SourceResult::ready(vec![DynamicEntry::new("one", Item(query))]) },
            )
        }
    }

    #[test]
    fn worker_results_reach_the_model_without_keyboard_input() {
        let mut model = model();
        let mut sources = Sources::start(vec![registration(Immediate)], &model).unwrap();
        model.insert_text("#cafe\u{301}");
        sources.update(&mut model).unwrap();
        eventually(|| {
            sources.update(&mut model).unwrap();
            model.matched() == 1
        });
        assert_eq!(model.take_selected().unwrap().0, "cafe\u{301}");
    }

    struct Panicking;
    impl Source<Item> for Panicking {
        fn query(
            &mut self,
            _query: String,
        ) -> Pin<Box<dyn Future<Output = SourceResult<Item>> + Send + '_>> {
            Box::pin(async { panic!("hand-authored provider failure") })
        }
    }

    #[test]
    fn provider_panic_ends_the_picker_instead_of_leaving_loading_chrome() {
        let mut model = model();
        let mut sources = Sources::start(vec![registration(Panicking)], &model).unwrap();
        model.insert_text("#");
        sources.update(&mut model).unwrap();
        eventually(|| matches!(sources.update(&mut model), Err(Error::SourcePanicked)));
        assert!(matches!(sources.finish(), Err(Error::SourcePanicked)));
    }

    struct Oversized;
    impl Source<Item> for Oversized {
        fn query(
            &mut self,
            _query: String,
        ) -> Pin<Box<dyn Future<Output = SourceResult<Item>> + Send + '_>> {
            Box::pin(async {
                SourceResult::ready(
                    (0..4)
                        .map(|id| DynamicEntry::new(id.to_string(), Item(id.to_string())))
                        .collect(),
                )
            })
        }
    }

    #[test]
    fn oversized_batches_become_inert_errors_before_result_queue_delivery() {
        let mut model = model();
        let mut sources = Sources::start(vec![registration(Oversized)], &model).unwrap();
        model.insert_text("#");
        sources.update(&mut model).unwrap();
        eventually(|| !sources.results.is_empty());
        let response = sources.results.try_recv().unwrap();
        assert!(response.result.entries.is_empty());
        assert!(matches!(response.result.status, SourceStatus::Error(_)));
    }

    #[test]
    fn build_rejects_sources_without_exclusive_scopes_and_duplicate_providers() {
        let limit = NonZeroUsize::new(3).unwrap();
        let unscoped = Picker::<Item>::new(Vec::new())
            .group(Group::new(true, "remote", vec![Column::fill()]))
            .source(true, limit, Immediate)
            .build();
        assert!(matches!(unscoped, Err(Error::SourceConfiguration { .. })));
        let duplicate = Picker::<Item>::new(Vec::new())
            .group(Group::new(true, "remote", vec![Column::fill()]).exclusive('#'))
            .source(true, limit, Immediate)
            .source(true, limit, Immediate)
            .build();
        assert!(matches!(duplicate, Err(Error::SourceConfiguration { .. })));
    }

    #[test]
    fn initial_query_uses_paste_sanitization_and_starts_at_the_end() {
        let mut model = Picker::<Item>::new(Vec::new())
            .group(Group::new(true, "remote", vec![Column::fill()]).exclusive('#'))
            .query("#cafe\u{301}\n")
            .build()
            .unwrap();
        assert_eq!(model.query(), "#cafe\u{301} ");
        model.apply(Command::Insert('x'));
        assert_eq!(model.scoped_query(true), Some("cafe\u{301} x"));
    }
}
