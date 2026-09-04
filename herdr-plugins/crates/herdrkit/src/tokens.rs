//! Named values a plugin pushes into herdr's own rendering.
//!
//! herdr draws a token wherever the config asks for it by name:
//!
//! ```toml
//! [ui.sidebar.spaces]
//! rows = [["state_icon", "workspace"], ["branch", "git_status", "$ci"]]
//! ```
//!
//! That is the difference between a plugin that reports and a plugin that is
//! part of the UI. A `[[ui.tab_bar_right]]` command owns one shared line and is
//! re-run on a timer whether or not anything changed; tokens are per workspace
//! or per pane, styled by the config, and written when the plugin learns
//! something.
//!
//! # A TTL is the honest way to be stale
//!
//! Report through [`crate::MetadataReporter`] with a TTL. A token that expires
//! on its own cannot outlive the truth it described, which is the whole
//! difficulty with a status
//! indicator: a stale "passing" is worse than a blank column, and the
//! alternative — remembering to clear it on every path that could invalidate
//! it — is a rule nobody keeps. Set the TTL to a small multiple of the refresh
//! interval and a plugin that dies simply fades out.

use std::collections::BTreeMap;

use serde::Serialize;

/// A token name herdr will not accept, or one too many.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum TokenError {
    #[error("token name {name:?} must be 1-32 of A-Z a-z 0-9 _ -")]
    Name { name: String },

    #[error("a source may report {} tokens, not {count}", Tokens::MAX)]
    TooMany { count: usize },
}

/// Tokens for one workspace or pane, from one source.
///
/// A `None` value clears the token rather than drawing it empty, which is the
/// difference between "this workspace has no PR" and "this workspace has a PR whose
/// state I could not determine".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Tokens(BTreeMap<String, Option<String>>);

impl Tokens {
    /// Tokens one source may report at once.
    pub const MAX: usize = 16;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `name` to `value`.
    ///
    /// Validated here rather than at the server because both failures are
    /// silent on screen: herdr rejects the whole call for a bad name, and a
    /// seventeenth token is simply not drawn. Neither looks like an error in
    /// the place the token was supposed to appear.
    pub fn set(mut self, name: &str, value: impl Into<String>) -> Result<Self, TokenError> {
        self.insert(name, Some(value.into()))?;
        Ok(self)
    }

    /// Clears `name`, so herdr stops drawing it.
    pub fn clear(mut self, name: &str) -> Result<Self, TokenError> {
        self.insert(name, None)?;
        Ok(self)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).and_then(|value| value.as_deref())
    }

    fn insert(&mut self, name: &str, value: Option<String>) -> Result<(), TokenError> {
        if !valid_name(name) {
            return Err(TokenError::Name {
                name: name.to_owned(),
            });
        }
        if !self.0.contains_key(name) && self.0.len() >= Self::MAX {
            return Err(TokenError::TooMany {
                count: self.0.len() + 1,
            });
        }
        self.0.insert(name.to_owned(), value);
        Ok(())
    }
}

/// herdr's own pattern: 1-32 of `A-Za-z0-9_-`.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_serialises_as_a_bare_object() {
        let tokens = Tokens::new().set("ci", "passing").unwrap();
        assert_eq!(
            serde_json::to_string(&tokens).unwrap(),
            r#"{"ci":"passing"}"#
        );
    }

    /// A cleared token is an explicit null, which is how herdr removes it. An
    /// omitted key would leave the previous value on screen.
    #[test]
    fn a_cleared_token_is_sent_as_null() {
        let tokens = Tokens::new().clear("ci").unwrap();
        assert_eq!(serde_json::to_string(&tokens).unwrap(), r#"{"ci":null}"#);
    }

    #[test]
    fn a_name_herdr_would_reject_is_refused_here() {
        for name in ["", "$ci", "ci status", "ci.status", "über"] {
            assert_eq!(
                Tokens::new().set(name, "x").err(),
                Some(TokenError::Name {
                    name: name.to_owned()
                }),
                "{name:?} should be refused"
            );
        }
        // The config references a token as `$ci`, but the sigil belongs to the
        // config's syntax and is not part of the name.
        assert!(Tokens::new().set("ci", "x").is_ok());
        assert!(Tokens::new().set("pr_state-2", "x").is_ok());
        assert!(Tokens::new().set(&"n".repeat(32), "x").is_ok());
        assert!(Tokens::new().set(&"n".repeat(33), "x").is_err());
    }

    #[test]
    fn the_seventeenth_token_is_refused_but_overwriting_is_not() {
        let mut tokens = Tokens::new();
        for n in 0..Tokens::MAX {
            tokens = tokens.set(&format!("t{n}"), "x").unwrap();
        }
        assert_eq!(tokens.len(), Tokens::MAX);
        assert_eq!(
            tokens.clone().set("one_more", "x").err(),
            Some(TokenError::TooMany { count: 17 })
        );
        assert!(
            tokens.set("t0", "changed").is_ok(),
            "replacing an existing token does not grow the map"
        );
    }
}
