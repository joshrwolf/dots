//! Identifiers herdr hands out.
//!
//! Three kinds of opaque string flow through every API call, and `pane.neighbor`
//! against a tab id is a call that compiles, reaches the server, and comes back
//! `not_found` at a keypress. Distinct types make that a compile error.

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Identifies a ", $what, ".")]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        // Lets a `HashMap<&WorkspaceId, _>` be probed with a `&str`, so joining
        // two API responses on an id needs no cloning.
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }
    };
}

id!(WorkspaceId, "workspace");
id!(TabId, "tab");
id!(PaneId, "pane");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn an_id_round_trips_as_a_bare_string() {
        let json = "\"w2:p1\"";
        let pane: PaneId = serde_json::from_str(json).unwrap();
        assert_eq!(pane.as_str(), "w2:p1");
        assert_eq!(serde_json::to_string(&pane).unwrap(), json);
    }

    #[test]
    fn a_map_keyed_by_id_can_be_probed_with_a_str() {
        let mut map = HashMap::new();
        map.insert(WorkspaceId::new("w2"), "dots");
        assert_eq!(map.get("w2"), Some(&"dots"));
    }
}
