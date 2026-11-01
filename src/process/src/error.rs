//! Why a subprocess could not be run or read.

use std::io;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("subprocess deadline and output limits must be positive")]
    InvalidLimits,
    #[error("starting subprocess runtime")]
    Runtime(#[source] io::Error),
    #[error("starting subprocess")]
    Spawn(#[source] io::Error),
    #[error("reading subprocess {stream}")]
    Read {
        stream: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("writing subprocess stdout to its destination")]
    Sink(#[source] io::Error),
    #[error("waiting for subprocess")]
    Wait(#[source] io::Error),
    #[error("subprocess exceeded its {0:?} deadline")]
    Timeout(Duration),
    #[error("subprocess {stream} exceeded {limit} bytes")]
    OutputLimit { stream: &'static str, limit: usize },
}
