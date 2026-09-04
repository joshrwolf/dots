# Shared picker contract

Every plugin that asks the user to choose from a list uses
`herdrkit::picker`. The picker runs in the plugin process and returns the same
typed value it was given. Do not serialize entries into lines, shell out to an
external finder, or spawn work per keystroke or cursor movement.

## Entries, cells, and groups

An item implements `Entry`:

```rust
impl Entry for Destination {
    type Group = Kind;

    fn group(&self) -> Kind {
        self.kind()
    }

    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        // Exactly one cell for every column declared by this group.
        vec![Cell::new(self.label(), theme.strong)]
    }

    fn hidden_terms(&self) -> Option<Cow<'_, str>> {
        None
    }
}
```

- `Cell::new` is drawn and searchable.
- `Cell::tag` is drawn but not searchable, for glyphs and chrome.
- `hidden_terms` contributes searchable text that is not drawn.
- Control characters in cell text are sanitized by the shared picker.
- `Column::new(n)` reserves terminal cells; `Column::fill()` shares the
  remainder and may appear anywhere.
- A `Group` owns a label, its columns, and optionally a scope sigil.

Declare every group exactly once and make every entry's cell arity match its
group. `Picker::build` rejects no groups, duplicate groups, undeclared groups,
and arity mismatches. Groups retain declaration order; matching rank applies
within a group so important small groups are not interleaved with large ones.

Group labels are display chrome and not searchable. If users should find a row
by its kind or a status represented only by a glyph, include that word in
`hidden_terms` and test it.

`Group::scoped('/')` hides the group until the query starts with `/`, then
shows only groups with that sigil. The sigil is removed before fuzzy matching.
Use this for a large secondary source that would overwhelm the default view.

## Unicode and matching

All widths are terminal-cell widths and all cuts occur at grapheme boundaries.
Never use byte length, `chars().count()`, or string slicing for layout. Wide
glyphs, combining marks, and emoji sequences otherwise misalign columns or get
split.

The picker uses Nucleo with smart case and normalization. `.match_paths()`
enables path-boundary scoring. Its match indices for non-ASCII strings align
with grapheme clusters, so any index-to-display mapping must also walk
`graphemes(true)`.

Searchable cells are joined with spaces before `hidden_terms`. Nucleo's fzf
atom syntax is supported, including prefix, suffix, exact, and negated atoms.

## Construction and testing

```rust
let chosen = Picker::new(items)
    .groups(groups())
    .prompt("select> ")
    .match_paths()
    .run()?;
```

Use `Picker::build()` in tests to obtain a terminal-free `Model`. Drive it with
`apply(Command::...)` and inspect `matched`, `candidates`, `selected`,
`take_selected`, and `lines(width, height)`. Assert behavior at the narrowest
supported width, non-ASCII alignment, search-only terms, scope filtering,
group order, and selection—not snapshots that merely look plausible.

The shared keyboard mapping follows fzf conventions: arrows and
`ctrl-j`/`ctrl-k` navigate, Enter accepts, Escape and common cancel chords
abort, readline-style chords edit the query, and page keys move by the visible
page. Change key behavior in `herdrkit`, not in one plugin.

## Terminal and theme

Terminal initialization, panic restoration, alternate-screen cleanup, caret
placement, and grapheme-aware editing belong to `herdrkit::picker`. Restore the
terminal on every exit path. A plugin should not install a competing event loop
or terminal lifecycle.

`Theme::load()` resolves Herdr's custom theme and falls back safely. Pass the
theme into `Entry::cells`; do not capture a separate palette in entry values.
Use `Theme::status` for Herdr-consistent agent state glyphs and colors.

For picker panes, prefer fixed numeric cell dimensions. Herdr clamps them to
the terminal, avoiding excessively wide rows on large displays. Ensure the
declared width leaves useful space after all fixed columns; test the rendered
model at that width.

The current picker returns one `Option<T>` and has no preview pane or
multi-select. Add such features once in `herdrkit` before exposing them from a
plugin.
