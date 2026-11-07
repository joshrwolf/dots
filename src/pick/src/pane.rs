//! Interactive detail panes driven by the picker's terminal and background runtime.
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use ratatui::Frame;
use ratatui::crossterm::event::Event;
use ratatui::layout::Rect;
use tokio::sync::oneshot;

use crate::background::Background;
use crate::{Entry, Model, Palette};

type Pending<M> = Pin<Box<dyn Future<Output = M> + Send + 'static>>;

/// Work whose typed result returns to the pane on the terminal thread.
pub struct Task<M> {
    label: Option<String>,
    work: Pending<M>,
}

impl<M> std::fmt::Debug for Task<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl<M> Task<M> {
    /// Refresh work leaves input usable while it runs.
    pub fn background(work: impl Future<Output = M> + Send + 'static) -> Self {
        Self {
            label: None,
            work: Box::pin(work),
        }
    }

    /// An explicit action temporarily holds input. Pending input is discarded
    /// when it settles, so repeated Return cannot answer a different question.
    pub fn action(
        label: impl Into<String>,
        work: impl Future<Output = M> + Send + 'static,
    ) -> Self {
        Self {
            label: Some(label.into()),
            work: Box::pin(work),
        }
    }
}

/// An inbox keeps a bounded list beside its detail; narrow terminals show the
/// detail alone while the prompt and list-navigation keys remain available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneLayout {
    pub list_width: u16,
    pub collapse_below: u16,
}

impl PaneLayout {
    pub(crate) fn areas(self, body: Rect) -> (Option<Rect>, Rect) {
        if body.width < self.collapse_below {
            return (None, body);
        }
        let width = self.list_width.min(body.width / 3);
        let gap = body.width.saturating_sub(width).min(2);
        let list = Rect::new(body.x, body.y, width, body.height);
        let pane = Rect::new(
            body.x + width + gap,
            body.y,
            body.width - width - gap,
            body.height,
        );
        (Some(list), pane)
    }
}

/// Whether a pane handled input or a background result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Ignored,
    Redraw,
    Close,
}

/// Host-owned detail state. Callbacks run on the terminal thread and must not
/// do I/O; return work from `poll` for the shared runtime instead. The host
/// owns editing, validation and submission, while the picker owns the list.
pub trait Pane<T: Entry> {
    type Message: Send + 'static;

    fn layout(&self) -> PaneLayout;
    fn poll(&mut self, _now: Instant) -> Vec<Task<Self::Message>> {
        Vec::new()
    }
    fn update(&mut self, message: Self::Message, model: &mut Model<T>) -> Response;
    fn selected(&mut self, _entry: Option<&T>) {}
    fn event(&mut self, event: &Event) -> Response;
    /// The pane, rather than the filter, owns the visible caret.
    fn focused(&self) -> bool;
    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, palette: Palette);
}

pub(crate) struct Tasks<M> {
    pending: Vec<(Option<String>, oneshot::Receiver<M>)>,
}

impl<M: Send + 'static> Tasks<M> {
    pub(crate) fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub(crate) fn start(&mut self, task: Task<M>, background: &mut Background) {
        let (send, receive) = oneshot::channel();
        background.spawn(async move {
            let _ = send.send(task.work.await);
        });
        self.pending.push((task.label, receive));
    }

    pub(crate) fn action(&self) -> Option<&str> {
        self.pending.iter().find_map(|(label, _)| label.as_deref())
    }

    pub(crate) fn update(&mut self) -> Vec<(M, bool)> {
        let mut ready = Vec::new();
        let mut index = 0;
        while index < self.pending.len() {
            if let Ok(message) = self.pending[index].1.try_recv() {
                let (label, _) = self.pending.swap_remove(index);
                ready.push((message, label.is_some()));
            } else {
                // A producer that panics closes its sender; Background reports
                // that defect and restores the terminal before resuming it.
                index += 1;
            }
        }
        ready
    }
}

pub(crate) struct NoPane;

impl<T: Entry> Pane<T> for NoPane {
    type Message = std::convert::Infallible;
    fn layout(&self) -> PaneLayout {
        unreachable!("no interactive pane")
    }
    fn update(&mut self, message: Self::Message, _: &mut Model<T>) -> Response {
        match message {}
    }
    fn event(&mut self, _: &Event) -> Response {
        Response::Ignored
    }
    fn focused(&self) -> bool {
        false
    }
    fn draw(&mut self, _: &mut Frame<'_>, _: Rect, _: Palette) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_background_refresh_does_not_hold_input_and_actions_settle_independently() {
        let mut background = Background::start().unwrap();
        let mut tasks = Tasks::new();
        let (send, receive) = oneshot::channel();
        tasks.start(
            Task::background(async move {
                receive.await.unwrap();
                1
            }),
            &mut background,
        );
        assert_eq!(tasks.action(), None);
        tasks.start(Task::action("saving", async { 2 }), &mut background);
        assert_eq!(tasks.action(), Some("saving"));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut ready = Vec::new();
        while ready.is_empty() {
            ready = tasks.update();
            assert!(Instant::now() < deadline, "action did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ready, vec![(2, true)]);
        assert_eq!(tasks.action(), None);
        send.send(()).unwrap();
        let mut ready = Vec::new();
        while ready.is_empty() {
            ready = tasks.update();
            assert!(Instant::now() < deadline, "refresh did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(ready, vec![(1, false)]);
        assert!(tasks.pending.is_empty());
        background.shutdown().unwrap();
    }

    #[test]
    fn detail_geometry_is_bounded_and_collapses_without_losing_the_detail() {
        let layout = PaneLayout {
            list_width: 34,
            collapse_below: 72,
        };
        for width in 0..200 {
            let body = Rect::new(3, 5, width, 20);
            let (list, pane) = layout.areas(body);
            assert_eq!(pane.bottom(), body.bottom());
            assert_eq!(pane.right(), body.right());
            if width < 72 {
                assert_eq!(list, None);
                assert_eq!(pane, body);
            } else {
                let list = list.unwrap();
                assert!(list.width <= 34);
                assert!(list.right() < pane.x);
                assert!(pane.width > list.width);
            }
        }
    }
}
