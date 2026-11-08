//! Terminal input on its own thread, which can be paused so the viewer can
//! read the terminal's replies to colour queries without crossterm taking
//! them for keys.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Run,
    PauseWanted,
    Paused,
}

#[derive(Debug, Clone)]
pub(crate) struct Input {
    gate: Arc<(Mutex<Gate>, Condvar)>,
}

/// While held, the input thread reads nothing.
#[derive(Debug)]
pub(crate) struct Paused<'a> {
    input: &'a Input,
}

impl Drop for Paused<'_> {
    fn drop(&mut self) {
        let (lock, cv) = &*self.input.gate;
        if let Ok(mut g) = lock.lock() {
            *g = Gate::Run;
        }
        cv.notify_all();
    }
}

impl Input {
    /// Forwards every terminal event through `send` until it fails.
    pub(crate) fn spawn<M: Send + 'static>(tx: Sender<M>, wrap: fn(Event) -> M) -> Self {
        let gate = Arc::new((Mutex::new(Gate::Run), Condvar::new()));
        let input = Self { gate };
        let thread_gate = Arc::clone(&input.gate);
        std::thread::spawn(move || {
            loop {
                {
                    let (lock, cv) = &*thread_gate;
                    let Ok(mut g) = lock.lock() else { return };
                    if *g == Gate::PauseWanted {
                        *g = Gate::Paused;
                        cv.notify_all();
                        while *g != Gate::Run {
                            let Ok(next) = cv.wait(g) else { return };
                            g = next;
                        }
                    }
                }
                match event::poll(Duration::from_millis(25)) {
                    Ok(true) => {
                        let Ok(ev) = event::read() else { return };
                        if tx.send(wrap(ev)).is_err() {
                            return;
                        }
                    }
                    Ok(false) => {}
                    Err(_) => return,
                }
            }
        });
        input
    }

    /// Stops the input thread between reads and returns once it has.
    pub(crate) fn pause(&self) -> Paused<'_> {
        let (lock, cv) = &*self.gate;
        let mut g = lock.lock().expect("input gate");
        *g = Gate::PauseWanted;
        cv.notify_all();
        let deadline = std::time::Instant::now() + Duration::from_millis(200);
        while *g != Gate::Paused && std::time::Instant::now() < deadline {
            g = cv
                .wait_timeout(g, Duration::from_millis(50))
                .expect("input gate")
                .0;
        }
        drop(g);
        Paused { input: self }
    }
}
