//! Pure bounded Vim-style destination history.
use std::collections::HashSet;

use anyhow::{Result, ensure};
use herdrkit::TerminalId;
use serde::{Deserialize, Serialize};

pub(crate) const CAPACITY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Travel {
    Back,
    Forward,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct History {
    entries: Vec<TerminalId>,
    current: Option<TerminalId>,
}

impl History {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(self.entries.len() <= CAPACITY, "jumplist exceeds capacity");
        let unique: HashSet<_> = self.entries.iter().collect();
        ensure!(
            unique.len() == self.entries.len(),
            "duplicate jumplist destination"
        );
        ensure!(
            self.entries.iter().all(|id| !id.as_str().is_empty()),
            "empty terminal ID"
        );
        ensure!(
            self.current.as_ref().is_none_or(|id| unique.contains(id)),
            "jumplist cursor is missing"
        );
        Ok(())
    }

    pub(crate) fn observe(&mut self, target: TerminalId) {
        if self.current.as_ref() == Some(&target) {
            return;
        }
        let source = self.current.take();
        self.entries
            .retain(|id| Some(id) != source.as_ref() && *id != target);
        self.entries.extend(source);
        self.entries.push(target.clone());
        let excess = self.entries.len().saturating_sub(CAPACITY);
        self.entries.drain(..excess);
        self.current = Some(target);
    }

    pub(crate) fn retain(&mut self, live: &HashSet<&TerminalId>) {
        self.entries.retain(|id| live.contains(id));
        if self.current.as_ref().is_some_and(|id| !live.contains(id)) {
            self.current = None;
        }
    }

    pub(crate) fn target(&self, direction: Travel) -> Option<&TerminalId> {
        let position = self
            .current
            .as_ref()
            .and_then(|id| self.entries.iter().position(|entry| entry == id));
        let index = match (position, direction) {
            (Some(index), Travel::Back) => index.checked_sub(1)?,
            (Some(index), Travel::Forward) => index.checked_add(1)?,
            (None, Travel::Back) => self.entries.len().checked_sub(1)?,
            (None, Travel::Forward) => return None,
        };
        self.entries.get(index)
    }

    pub(crate) fn select(&mut self, target: &TerminalId) -> Result<()> {
        ensure!(
            self.entries.contains(target),
            "traversal target is no longer in history"
        );
        self.current = Some(target.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> TerminalId {
        TerminalId::new(s)
    }
    fn visit(h: &mut History, names: &[&str]) {
        for name in names {
            h.observe(id(name));
        }
    }
    fn travel(h: &mut History, direction: Travel) -> Option<TerminalId> {
        let target = h.target(direction)?.clone();
        h.select(&target).unwrap();
        Some(target)
    }

    #[test]
    fn branching_preserves_forward_destinations_and_renews_departure() {
        let mut h = History::default();
        visit(&mut h, &["A", "B", "C"]);
        assert_eq!(travel(&mut h, Travel::Back), Some(id("B")));
        assert_eq!(travel(&mut h, Travel::Forward), Some(id("C")));
        travel(&mut h, Travel::Back);
        h.observe(id("D"));
        assert_eq!(h.entries, ["A", "C", "B", "D"].map(id));
        travel(&mut h, Travel::Back);
        assert_eq!(travel(&mut h, Travel::Back), Some(id("C")));
        h.observe(id("A"));
        assert_eq!(h.entries, ["B", "D", "C", "A"].map(id));
    }

    #[test]
    fn traversal_and_duplicates_do_not_record_or_wrap() {
        let mut h = History::default();
        assert_eq!(travel(&mut h, Travel::Back), None);
        visit(&mut h, &["A", "B", "C"]);
        let entries = h.entries.clone();
        for name in ["B", "A"] {
            assert_eq!(travel(&mut h, Travel::Back), Some(id(name)));
            h.observe(id(name)); // late hook reading the traversal destination
        }
        assert_eq!(travel(&mut h, Travel::Back), None);
        for name in ["B", "C"] {
            assert_eq!(travel(&mut h, Travel::Forward), Some(id(name)));
        }
        assert_eq!(travel(&mut h, Travel::Forward), None);
        assert_eq!(h.entries, entries);
    }

    #[test]
    fn closing_entries_preserves_a_live_cursor_and_skips_dead_destinations() {
        let mut h = History::default();
        visit(&mut h, &["A", "B", "C", "D"]);
        let live = [id("A"), id("C"), id("D")];
        h.retain(&live.iter().collect());
        assert_eq!(travel(&mut h, Travel::Back), Some(id("C")));
        let live = [id("A"), id("D")];
        h.retain(&live.iter().collect());
        h.observe(id("D"));
        assert_eq!(travel(&mut h, Travel::Back), Some(id("A")));
    }

    #[test]
    fn capacity_and_uniqueness_hold_through_revisits_and_traversal() {
        let mut h = History::default();
        for n in 0..1000 {
            h.observe(id(&format!("t{}", n % 137)));
            if n % 3 == 0 {
                travel(&mut h, Travel::Back);
            }
            h.validate().unwrap();
        }
        assert_eq!(h.entries.len(), CAPACITY);
    }

    #[test]
    fn malformed_checkpoints_are_rejected() {
        for json in [
            r#"{"entries":["a","a"],"current":"a"}"#,
            r#"{"entries":["a"],"current":"b"}"#,
            r#"{"entries":[""],"current":null}"#,
        ] {
            assert!(
                serde_json::from_str::<History>(json)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
    }
}
