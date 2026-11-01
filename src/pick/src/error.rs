//! Why a picker cannot be drawn or run.

use std::io;

/// A picker that cannot be drawn.
///
/// Layout errors are reported rather than tolerated: an entry in an undeclared group
/// would otherwise be silently absent from the list, and a cell count that
/// disagrees with its columns drops content or leaves a blank column that
/// reads as missing data. Neither is visible on screen.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid, reserved, or duplicate picker shortcut ctrl+{key}")]
    InvalidShortcut { key: char },

    #[error("invalid or ambiguous picker scope {sigil:?}")]
    InvalidScope { sigil: char },

    #[error("the terminal could not be driven")]
    Terminal(#[from] io::Error),

    #[error("no groups were declared, so no entry could ever be shown")]
    NoGroups,

    #[error("group replacement contains an empty or duplicate entry identity: {id:?}")]
    InvalidIdentity { id: String },

    #[error("group {group} is declared twice")]
    DuplicateGroup { group: String },

    #[error("an entry belongs to group {group}, which is not declared")]
    UndeclaredGroup { group: String },

    #[error("group {group} declares {declared} columns but an entry drew {drawn} cells")]
    Arity {
        group: String,
        declared: usize,
        drawn: usize,
    },
}
