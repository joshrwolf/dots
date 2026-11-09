//! The sidebar's file tree: directories nest and collapse, and a cursor
//! selects one of the rows shown.

use std::collections::HashSet;
use std::ops::Range;

use super::FileView;

/// How a tree row's guides are drawn: for each ancestor column, whether a
/// line continues past this row, and whether the row is its parent's last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Guides {
    /// Bit `k` set: the guide in column `k` continues below this row.
    pub(crate) rails: u64,
    pub(crate) last: bool,
}

impl Guides {
    /// Guides for the children of a row at `depth` drawn with `self`.
    fn child(self, depth: u16, last: bool) -> Self {
        let rails = if depth == 0 || last || depth > 64 {
            self.rails
        } else {
            self.rails | 1 << (depth - 1)
        };
        Self { rails, last: false }
    }

    /// Whether column `k` of a row continues a guide from above.
    pub(crate) fn rail(self, k: u16) -> bool {
        k < 64 && self.rails & 1 << k != 0
    }
}

/// One row of the file tree, `depth` levels in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListItem {
    Dir {
        /// The row's text: one directory, or a collapsed chain of them.
        label: String,
        /// Full path with a trailing slash; the collapse key.
        path: String,
        depth: u16,
        /// Indexes of the files inside, which are contiguous.
        files: Range<usize>,
        guides: Guides,
    },
    File {
        index: usize,
        depth: u16,
        guides: Guides,
    },
}

impl ListItem {
    pub(crate) fn depth(&self) -> u16 {
        match self {
            ListItem::Dir { depth, .. } | ListItem::File { depth, .. } => *depth,
        }
    }

    pub(crate) fn guides(&self) -> Guides {
        match self {
            ListItem::Dir { guides, .. } | ListItem::File { guides, .. } => *guides,
        }
    }

    /// The first file the row stands for.
    pub(crate) fn first_file(&self) -> usize {
        match self {
            ListItem::Dir { files, .. } => files.start,
            ListItem::File { index, .. } => *index,
        }
    }
}

/// The file tree of a stream, the rows left showing outside collapsed
/// directories, and the selected one of them.
#[derive(Debug, Default)]
pub(crate) struct Sidebar {
    /// The whole file tree.
    tree: Vec<ListItem>,
    /// Its rows outside collapsed directories, as the sidebar shows them.
    list: Vec<ListItem>,
    /// Paths of collapsed directories.
    collapsed: HashSet<String>,
    /// The selected row, an index into `list` while it has rows.
    cursor: usize,
}

impl Sidebar {
    /// Starts over for another stream: nothing collapsed or selected.
    pub(crate) fn reset(&mut self) {
        self.collapsed.clear();
        self.cursor = 0;
    }

    /// Builds the tree of the `files` shown, keeping collapsed directories.
    pub(crate) fn set_files(&mut self, files: &[FileView]) {
        self.tree = list_items(files);
        self.list = visible(&self.tree, &self.collapsed);
        self.cursor = self.cursor.min(self.list.len().saturating_sub(1));
    }

    /// The rows shown.
    pub(crate) fn rows(&self) -> &[ListItem] {
        &self.list
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    /// The selected row.
    pub(crate) fn selected(&self) -> Option<&ListItem> {
        self.list.get(self.cursor)
    }

    /// Selects row `row`, if it is shown.
    pub(crate) fn select(&mut self, row: usize) {
        if row < self.list.len() {
            self.cursor = row;
        }
    }

    pub(crate) fn is_collapsed(&self, path: &str) -> bool {
        self.collapsed.contains(path)
    }

    /// The row showing file `index`: its own row or the collapsed
    /// directory that holds it.
    pub(crate) fn row_of_file(&self, index: usize) -> Option<usize> {
        self.list.iter().position(|it| match it {
            ListItem::File { index: i, .. } => *i == index,
            ListItem::Dir { path, files, .. } => {
                files.contains(&index) && self.collapsed.contains(path)
            }
        })
    }

    /// Selects the row showing file `index`.
    pub(crate) fn follow_file(&mut self, index: usize) {
        if let Some(row) = self.row_of_file(index) {
            self.cursor = row;
        }
    }

    /// Collapses or expands the directory at row `row`; false when the row
    /// is a file.
    pub(crate) fn toggle_dir(&mut self, row: usize) -> bool {
        let Some(ListItem::Dir { path, .. }) = self.list.get(row) else {
            return false;
        };
        let path = path.clone();
        self.set_collapsed(&path, !self.collapsed.contains(&path));
        true
    }

    /// Collapses or expands the directory at `path`, keeping its row selected.
    pub(crate) fn set_collapsed(&mut self, path: &str, collapse: bool) {
        if collapse {
            self.collapsed.insert(path.to_owned());
        } else {
            self.collapsed.remove(path);
        }
        self.list = visible(&self.tree, &self.collapsed);
        match self
            .list
            .iter()
            .position(|it| matches!(it, ListItem::Dir { path: p, .. } if p == path))
        {
            Some(row) => self.cursor = row,
            None => self.cursor = self.cursor.min(self.list.len().saturating_sub(1)),
        }
    }

    /// The row of the directory holding row `row`, if any.
    pub(crate) fn parent_row(&self, row: usize) -> Option<usize> {
        let depth = self.list.get(row)?.depth();
        (0..row).rev().find(|&r| self.list[r].depth() < depth)
    }

    /// The first row shown in a list `rows` tall: the cursor stays near
    /// the middle.
    pub(crate) fn list_start(&self, rows: usize) -> usize {
        self.cursor
            .saturating_sub(rows / 2)
            .min(self.list.len().saturating_sub(rows))
    }
}

/// The file list as a nested tree in path order. A directory chain with a
/// single child directory collapses into one row, like `src/diff/src/`.
fn list_items(files: &[FileView]) -> Vec<ListItem> {
    let (index, paths): (Vec<usize>, Vec<&str>) = files
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.hidden)
        .map(|(i, f)| (i, f.change.path.as_str()))
        .unzip();
    let mut items = Vec::new();
    tree(&paths, 0..paths.len(), 0, 0, Guides::default(), &mut items);
    // The tree numbers the files shown; rows name files by stream index.
    for item in &mut items {
        match item {
            ListItem::File { index: i, .. } => *i = index[*i],
            ListItem::Dir { files, .. } => *files = index[files.start]..index[files.end - 1] + 1,
        }
    }
    items
}

/// Lists `paths[range]`, which share their first `prefix` bytes, as
/// children at `depth` below ancestors drawn with `guides`.
fn tree(
    paths: &[&str],
    range: Range<usize>,
    prefix: usize,
    depth: u16,
    guides: Guides,
    out: &mut Vec<ListItem>,
) {
    let mut i = range.start;
    while i < range.end {
        let rest = &paths[i][prefix..];
        let Some(slash) = rest.find('/') else {
            let last = i + 1 == range.end;
            out.push(ListItem::File {
                index: i,
                depth,
                guides: Guides { last, ..guides },
            });
            i += 1;
            continue;
        };
        // Paths in sorted order keep each directory's contents together.
        let head = &rest[..=slash];
        let end = (i..range.end)
            .find(|&j| !paths[j][prefix..].starts_with(head))
            .unwrap_or(range.end);
        let mut label_end = prefix + head.len();
        while let Some(slash) = paths[i][label_end..].find('/') {
            let next = &paths[i][label_end..=label_end + slash];
            if !(i..end).all(|j| paths[j][label_end..].starts_with(next)) {
                break;
            }
            label_end += next.len();
        }
        let last = end == range.end;
        out.push(ListItem::Dir {
            label: paths[i][prefix..label_end].to_owned(),
            path: paths[i][..label_end].to_owned(),
            depth,
            files: i..end,
            guides: Guides { last, ..guides },
        });
        tree(
            paths,
            i..end,
            label_end,
            depth + 1,
            guides.child(depth, last),
            out,
        );
        i = end;
    }
}

/// The rows of `tree` left showing when the directories in `collapsed`
/// hide their contents.
fn visible(tree: &[ListItem], collapsed: &HashSet<String>) -> Vec<ListItem> {
    let mut out = Vec::with_capacity(tree.len());
    let mut hide_below: Option<u16> = None;
    for item in tree {
        if let Some(d) = hide_below {
            if item.depth() > d {
                continue;
            }
            hide_below = None;
        }
        if let ListItem::Dir { path, depth, .. } = item
            && collapsed.contains(path)
        {
            hide_below = Some(*depth);
        }
        out.push(item.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tree row as text: guides, then `name/` or a file index.
    fn row(item: &ListItem) -> String {
        let g = item.guides();
        let mut s: String = (0..item.depth())
            .map(|k| match () {
                () if k + 1 == item.depth() => {
                    if g.last {
                        "╰ "
                    } else {
                        "├ "
                    }
                }
                () if g.rail(k) => "│ ",
                () => "  ",
            })
            .collect();
        match item {
            ListItem::Dir { label, .. } => s.push_str(label),
            ListItem::File { index, .. } => s.push_str(&index.to_string()),
        }
        s
    }

    const PATHS: [&str; 6] = [
        "docs/containers/a.md",
        "docs/containers/b.md",
        "src/x/deep/y.rs",
        "src/x/z.rs",
        "src/z.rs",
        "top.rs",
    ];

    #[test]
    fn file_tree_nests_with_guides_and_compacts_chains() {
        let mut items = Vec::new();
        tree(&PATHS, 0..PATHS.len(), 0, 0, Guides::default(), &mut items);
        let rows: Vec<String> = items.iter().map(row).collect();
        assert_eq!(
            rows,
            [
                "docs/containers/",
                "├ 0",
                "╰ 1",
                "src/",
                "├ x/",
                "│ ├ deep/",
                "│ │ ╰ 2",
                "│ ╰ 3",
                "╰ 4",
                "5",
            ]
        );
        let ListItem::Dir { path, files, .. } = &items[4] else {
            panic!("x/ is a directory");
        };
        assert_eq!((path.as_str(), files.clone()), ("src/x/", 2..4));
    }

    #[test]
    fn collapsed_directories_hide_their_contents() {
        let mut items = Vec::new();
        tree(&PATHS, 0..PATHS.len(), 0, 0, Guides::default(), &mut items);
        let collapsed: HashSet<String> = ["src/x/".to_owned()].into();
        let rows: Vec<String> = visible(&items, &collapsed).iter().map(row).collect();
        assert_eq!(
            rows,
            ["docs/containers/", "├ 0", "╰ 1", "src/", "├ x/", "╰ 4", "5"]
        );
    }

    #[test]
    fn sidebar_cursor_follows_files_into_collapsed_folders() {
        let files: Vec<_> = PATHS
            .iter()
            .map(|p| {
                (
                    *p, "a
", "b
",
                )
            })
            .collect();
        let mut app = crate::app::fixture::app_with(&files);
        app.layout(80, 40);
        app.sidebar.set_collapsed("src/x/", true);
        assert_eq!(
            app.sidebar.cursor(),
            4,
            "the collapsed folder stays selected"
        );
        app.jump_to_file(3, 40);
        assert_eq!(app.sidebar.cursor(), 4, "src/x/z.rs shows as its folder");
        app.select_row(5, 40);
        assert_eq!(app.top.file, 4);
        assert_eq!(app.sidebar.parent_row(5), Some(3));
        assert!(app.sidebar.toggle_dir(4));
        assert_eq!(app.sidebar.rows().len(), 10);
        let stream = app.stream.as_mut().expect("loaded");
        stream.files[2].reviewed = true;
        assert!(!app.dir_summary(2..4).2);
        app.stream.as_mut().expect("loaded").files[3].reviewed = true;
        assert!(app.dir_summary(2..4).2);
    }
}
