//! Presets: the ways the viewer shows a review's current revision, and how
//! anchors move between a preset's line numbers and the review's.
//!
//! A review's anchors count lines of its base (old side) and head (new side).
//! `uncommitted` shows the HEAD commit against the head, so its old side is
//! the HEAD commit rather than the base; `committed` shows the base against
//! the HEAD commit, so its new side is the HEAD commit rather than the head.
//! Lines on the replaced side move through the zero-context diff between the
//! two, as refresh carries threads, and lines that differ between them have
//! no place on the other.
use std::cell::OnceCell;
use std::collections::BTreeMap;

use serde::Serialize;

use crate::diff::{Change, Patch};
use crate::git::Git;
use crate::{Anchor, ChangedFile, Error, Lines, Preset, Result, Revision, Side};

/// What the viewer diffs for one preset of a review's current revision, and
/// where the review's anchors appear in it.
#[derive(Debug, Serialize)]
pub struct View {
    pub preset: Preset,
    /// The presets that show any change at this revision, in order.
    pub available: Vec<Preset>,
    /// Commit whose tree the view's old side shows.
    pub base: String,
    /// Commit whose tree the view's new side shows.
    pub head: String,
    /// The paths the view diffs.
    pub files: Vec<ChangedFile>,
    /// By thread or draft id, where each anchor valid at the current revision
    /// falls in this view's line numbers; null where the view does not show it.
    pub anchors: BTreeMap<String, Option<Anchor>>,
}

/// One preset over one revision.
pub(crate) struct Frame<'a> {
    git: &'a Git,
    preset: Preset,
    /// Commits the view shows.
    base: String,
    head: String,
    /// The review's whole change, and the preset's own when it differs.
    everything: &'a Change,
    own: Option<Change>,
    /// The review side the preset replaces, the review's object on it, and
    /// the preset's object there.
    replaced: Option<(Side, String, String)>,
    to_view: OnceCell<Patch>,
    from_view: OnceCell<Patch>,
}

/// The presets with changes to show at `revision`.
pub(crate) fn available(git: &Git, revision: &Revision) -> Result<Vec<Preset>> {
    let mut presets = vec![Preset::Everything];
    let committed = git.object(&revision.head_commit, "tree")?;
    if committed != revision.head {
        presets.push(Preset::Uncommitted);
    }
    if git.object(&revision.base, "tree")? != committed {
        presets.push(Preset::Committed);
    }
    Ok(presets)
}

impl<'a> Frame<'a> {
    /// `preset` over `revision`, whose whole change is `everything`.
    pub(crate) fn new(
        git: &'a Git,
        revision: &Revision,
        everything: &'a Change,
        preset: Preset,
        available: &[Preset],
    ) -> Result<Self> {
        if !available.contains(&preset) {
            let names: Vec<&str> = available.iter().map(|preset| preset.name()).collect();
            return Err(Error::Invalid(format!(
                "the {} view shows nothing at this revision; available: {}",
                preset.name(),
                names.join(", ")
            )));
        }
        let commit = revision.head_commit.clone();
        let (base, head, own, replaced) = match preset {
            Preset::Everything => (revision.base.clone(), revision.snapshot.clone(), None, None),
            Preset::Uncommitted => {
                let change = Change::read(git, &commit, &revision.head, 3)?;
                let replaced = (Side::Old, revision.base.clone(), commit.clone());
                (
                    commit,
                    revision.snapshot.clone(),
                    Some(change),
                    Some(replaced),
                )
            }
            Preset::Committed => {
                let change = Change::read(git, &revision.base, &commit, 3)?;
                let replaced = (Side::New, revision.head.clone(), commit.clone());
                (revision.base.clone(), commit, Some(change), Some(replaced))
            }
        };
        Ok(Self {
            git,
            preset,
            base,
            head,
            everything,
            own,
            replaced,
            to_view: OnceCell::new(),
            from_view: OnceCell::new(),
        })
    }

    /// The change this view diffs.
    fn change(&self) -> &Change {
        self.own.as_ref().unwrap_or(self.everything)
    }

    /// Where review `anchor` falls in this view, if it shows it.
    pub(crate) fn place(&self, anchor: &Anchor) -> Result<Option<Anchor>> {
        if !self.change().changes(&anchor.path) {
            return Ok(None);
        }
        let Some(lines) = anchor.lines else {
            return Ok(Some(anchor.clone()));
        };
        match &self.replaced {
            Some((side, review, view)) if *side == lines.side => {
                let patch = cached(&self.to_view, || self.patch(review, view))?;
                Ok(patch
                    .map(&anchor.path, lines.start, lines.end)
                    .map(|(start, end)| Anchor::lines(&anchor.path, lines.side, start, end)))
            }
            _ => Ok(Some(anchor.clone())),
        }
    }

    /// The review anchor for `anchor` in this view's line numbers, or why it
    /// has none.
    pub(crate) fn unplace(&self, anchor: &Anchor) -> Result<Result<Anchor, String>> {
        if !self.change().changes(&anchor.path) {
            return Ok(Err(format!(
                "{} is not changed in the {} view",
                anchor.path,
                self.preset.name()
            )));
        }
        let Some(Lines { side, start, end }) = anchor.lines else {
            return Ok(Ok(anchor.clone()));
        };
        match &self.replaced {
            Some((replaced, review, view)) if *replaced == side => {
                let patch = cached(&self.from_view, || self.patch(view, review))?;
                Ok(patch
                    .map(&anchor.path, start, end)
                    .map(|(start, end)| Anchor::lines(&anchor.path, side, start, end))
                    .ok_or_else(|| {
                        format!(
                            "{}:{start}-{end} of the HEAD commit differs from the {}, so it has no place in the review; comment on it in the everything view",
                            anchor.path,
                            match side {
                                Side::Old => "base",
                                Side::New => "working tree",
                            }
                        )
                    }))
            }
            _ => Ok(Ok(anchor.clone())),
        }
    }

    /// The view, placing each `(id, anchor)`.
    pub(crate) fn view<'b>(
        self,
        available: Vec<Preset>,
        anchors: impl IntoIterator<Item = (&'b str, &'b Anchor)>,
    ) -> Result<View> {
        let mut placed = BTreeMap::new();
        for (id, anchor) in anchors {
            placed.insert(id.to_owned(), self.place(anchor)?);
        }
        Ok(View {
            preset: self.preset,
            available,
            files: self.change().files(),
            base: self.base,
            head: self.head,
            anchors: placed,
        })
    }

    fn patch(&self, from: &str, to: &str) -> Result<Patch> {
        Patch::parse(&self.git.diff(from, to, 0)?)
    }
}

fn cached(cell: &OnceCell<Patch>, read: impl FnOnce() -> Result<Patch>) -> Result<&Patch> {
    if let Some(patch) = cell.get() {
        return Ok(patch);
    }
    let patch = read()?;
    Ok(cell.get_or_init(|| patch))
}
