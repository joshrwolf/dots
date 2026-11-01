//! One observation of a PR and the stored words for its parts.
use serde::Serialize;

/// The variants of a status part and the text that stores and serializes
/// each one, declared once.
macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }

        impl $name {
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            pub(crate) fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

wire_enum! {
    /// Lifecycle state; `Unknown` until the first successful check.
    State { Open => "open", Draft => "draft", Merged => "merged", Closed => "closed", Unknown => "unknown" }
}

wire_enum! {
    /// GitHub's review decision; `None` when the repository requires no review decision.
    Review {
        Approved => "approved",
        ChangesRequested => "changes_requested",
        ReviewRequired => "review_required",
        None => "none",
        Unknown => "unknown",
    }
}

wire_enum! {
    /// Status-check rollup of the head commit; `None` when it has no checks.
    Ci { Success => "success", Failure => "failure", Pending => "pending", None => "none", Unknown => "unknown" }
}

/// One successful observation of a PR from GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Status {
    pub title: String,
    pub state: State,
    pub review: Review,
    pub ci: Ci,
    pub head_ref: String,
    /// The PR description as GitHub returns it; empty when there is none.
    pub body: String,
}
