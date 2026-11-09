//! What drawing reads besides the diffs: file texts, syntax colour and
//! within-line emphasis, kept by content id within byte budgets. Entries
//! outlive streams, so a worktree shown again or a file back to earlier
//! contents is drawn without reading or computing again.

use std::sync::Arc;

use super::{Body, FileTexts, FileView};
use crate::diff::{HunkEmphasis, Text};
use crate::highlight::{Key, Lines};
use crate::lru::Lru;
use crate::repo::Reader;

/// Bytes of file text kept for drawing; the rest is reread on demand.
const TEXT_BUDGET: usize = 64 * 1024 * 1024;
/// Bytes of syntax colour kept.
const HIGHLIGHT_BUDGET: usize = 32 * 1024 * 1024;
const EMPHASIS_BUDGET: usize = 4 * 1024 * 1024;

/// A hunk's within-line emphasis is keyed by the contents it was computed from.
pub(crate) type EmphasisKey = (Option<gix::ObjectId>, Option<gix::ObjectId>, u32);

/// What [`Cache::load_texts`] found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Loaded {
    /// Both texts are cached, read now, or the file has no text diff.
    Ready,
    /// The working-tree file changed since its diff, so it needs a new one.
    Stale,
    Unreadable(String),
}

#[derive(Debug)]
pub(crate) struct Cache {
    texts: Lru<gix::ObjectId, Arc<Text>>,
    highlights: Lru<Key, Option<Arc<Lines>>>,
    emphasis: Lru<EmphasisKey, Option<Arc<HunkEmphasis>>>,
    /// Rereads texts the cache dropped, from the stream shown.
    reader: Option<Reader>,
    empty: Arc<Text>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            texts: Lru::new(TEXT_BUDGET),
            highlights: Lru::new(HIGHLIGHT_BUDGET),
            emphasis: Lru::new(EMPHASIS_BUDGET),
            reader: None,
            empty: Arc::new(Text::new(Arc::from(""))),
        }
    }
}

impl Cache {
    /// Where dropped texts are read again from.
    pub(crate) fn set_reader(&mut self, reader: Option<Reader>) {
        self.reader = reader;
    }

    /// Both texts of `f` once its diff is computed, if they are cached; a
    /// missing side is the empty text.
    pub(crate) fn texts(&self, f: &FileView) -> Option<FileTexts> {
        let Body::Text(diff) = &f.body else {
            return None;
        };
        let side = |id: Option<gix::ObjectId>| match id {
            Some(id) => self.texts.peek(&id).cloned().map(|t| (Some(id), t)),
            None => Some((None, Arc::clone(&self.empty))),
        };
        Some(FileTexts {
            old: side(f.change.old)?,
            new: side(diff.new_id)?,
        })
    }

    /// Keeps a file's texts.
    pub(crate) fn keep_texts(&mut self, texts: FileTexts) {
        for (id, text) in [texts.old, texts.new] {
            if let Some(id) = id {
                let size = text.bytes();
                self.texts.insert(id, text, size);
            }
        }
    }

    /// Reads `f`'s texts unless they are cached.
    pub(crate) fn load_texts(&mut self, f: &FileView) -> Loaded {
        let Body::Text(diff) = &f.body else {
            return Loaded::Ready;
        };
        let wanted = [f.change.old, diff.new_id];
        let mut missing = false;
        for id in wanted.into_iter().flatten() {
            missing |= self.texts.get(&id).is_none();
        }
        let Some(reader) = self.reader.as_mut().filter(|_| missing) else {
            return Loaded::Ready;
        };
        match reader.contents(&f.change) {
            Ok(c) if c.new_id == wanted[1] => {
                for (id, bytes) in [(f.change.old, c.old), (c.new_id, c.new)] {
                    if let Some(id) = id {
                        let text = Text::from_bytes(&bytes);
                        let size = text.bytes();
                        self.texts.insert(id, Arc::new(text), size);
                    }
                }
                Loaded::Ready
            }
            Ok(_) => Loaded::Stale,
            Err(e) => Loaded::Unreadable(format!("{e:#}")),
        }
    }

    /// The syntax colour of a text, once highlighted.
    pub(crate) fn highlight(&self, key: &Key) -> Option<&Lines> {
        self.highlights.peek(key)?.as_deref()
    }

    pub(crate) fn has_highlight(&self, key: &Key) -> bool {
        self.highlights.contains(key)
    }

    /// Keeps a text's syntax colour; `None` when it could not be coloured.
    pub(crate) fn keep_highlight(&mut self, key: Key, lines: Option<Arc<Lines>>) {
        let size = lines.as_ref().map_or(0, |l| l.bytes());
        self.highlights.insert(key, lines, size);
    }

    /// A hunk's within-line emphasis, once computed.
    pub(crate) fn emphasis(&self, key: &EmphasisKey) -> Option<&HunkEmphasis> {
        self.emphasis.peek(key)?.as_deref()
    }

    pub(crate) fn has_emphasis(&self, key: &EmphasisKey) -> bool {
        self.emphasis.contains(key)
    }

    /// Keeps a hunk's emphasis; `None` when it has none.
    pub(crate) fn keep_emphasis(&mut self, key: EmphasisKey, emphasis: Option<HunkEmphasis>) {
        let size = emphasis
            .as_ref()
            .map_or(0, |e| (e.old.len() + e.new.len()) * 8)
            + 64;
        self.emphasis.insert(key, emphasis.map(Arc::new), size);
    }

    /// Bytes held by the text, colour and emphasis caches.
    #[cfg(feature = "dev")]
    pub(crate) fn bytes(&self) -> [usize; 3] {
        [
            self.texts.bytes(),
            self.highlights.bytes(),
            self.emphasis.bytes(),
        ]
    }
}
