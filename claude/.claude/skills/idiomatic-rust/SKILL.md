---
name: idiomatic-rust
description: "Idiomatic Rust: lint posture and the #[expect] escape hatch, grapheme-correct text handling, serde traps that fail silently, error design that reads well under anyhow, validating at construction, and the lib-plus-thin-bin testability split. Use when writing, reviewing, or refactoring any Rust."
---

# Idiomatic Rust

Everything here was paid for by a real bug or a real review finding. It is not
a survey of the language; for design vocabulary and structure,
the `idiomatic-go` principles that are not Go-specific apply unchanged —
converge on the codebase's design center, do not extract single-use helpers.

Where the work is a herdr plugin, load `herdr-plugin` as well: that skill owns
the herdr contract, this one owns the language.

## Lints, and the one escape hatch

The posture worth adopting: deny `unwrap_used`, `expect_used`, `panic`,
`panic_in_result_fn`, `indexing_slicing`, `todo`, `unimplemented` and
`dbg_macro`; warn `pedantic` as a group with `priority = -1` so individual
`allow` lines below it still win. This is load-bearing rather than taste
wherever a panic strands the user — a TUI that dies leaves their terminal
wedged with no way out.

**When a denial is wrong for one item, use `#[expect]` with a reason — never
widen the workspace list.**

```rust
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors the wire shape; collapsing these needs a translation layer that can only lose information"
)]
```

`expect` fails the build when the lint stops firing, so a local exemption
cannot outlive its reason. An `allow` is a permanent hole.

Two things learned the hard way about the lint set itself:

- **`indexing_slicing` is worth the friction.** It pushes you to `slice.get(i)`
  and to building strings by pushing rather than slicing at a byte offset,
  which is exactly where the multibyte bugs are.
- **`as_conversions` is not.** It forbids every `as`, which is unlivable next
  to `u16` terminal geometry, and `cast_possible_truncation` — already in
  `pedantic` — is the lint that catches actual data loss.
- **Never add `unused_crate_dependencies`.** It is evaluated per compilation
  target, so in a lib-plus-bin crate a dependency the binary uses fails the lib
  target. `cargo-machete` reads the manifest and gets this right.

`clippy.toml` at the workspace root is mandatory once those denials exist, or
test code cannot be written:

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
```

## Text is graphemes

Three units for "how long is this string", and picking the wrong one fails
differently:

| unit | use it for | how it fails |
|---|---|---|
| bytes (`len`) | buffers | nonsense for anything user-facing |
| codepoints (`chars`) | almost nothing | one-cell `é` counts 2, two-cell 👨‍👩‍👧 counts 6 |
| display width (`unicode-width`) | how many terminal cells it occupies | — |
| graphemes (`unicode-segmentation`) | where you may cut, and what an index means | — |

So: **measure with `unicode-width`, cut with `unicode-segmentation`, and never
iterate `chars()` for anything positional.**

```rust
// Wrong: splits a base character from its combining mark, and mis-measures
// every wide glyph.
for ch in text.chars() { … }

// Right.
use unicode_segmentation::UnicodeSegmentation as _;
for grapheme in text.graphemes(true) {
    let cells = unicode_width::UnicodeWidthStr::width(grapheme);
    …
}
```

**A library's index unit is part of its contract — go read it.** `nucleo`'s
`Utf32String::from` collects `chars::graphemes()` for non-ASCII input, so a
match index means "the nth cluster", not "the nth codepoint". Walking `chars()`
to apply those indices put every highlight one place left of its text for each
extra codepoint ahead of it. No lint and no panic finds that; it is purely
visual, and only a non-ASCII test sees it.

Which means: **any test over text handling needs a non-ASCII case.** A
combining mark (`"cafe\u{301}"`) and a ZWJ emoji sequence
(`"\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"`) between them catch the
mis-measure and the mis-cut.

## serde traps

Each of these compiles and then misbehaves at runtime.

**Never `#[serde(flatten)]` an externally tagged enum.** Modelling a response
as `{ id, #[serde(flatten)] body: Result | Error }` fails with `missing field
result` on a response that plainly has one: flatten buffers the whole map and
then cannot pick a variant. Use `Option` fields and match on the pair.

```rust
// Wrong — and the error message accuses the server.
struct Response { id: String, #[serde(flatten)] body: Body }

// Right.
struct Response {
    id: String,
    #[serde(default)] result: Option<Value>,
    #[serde(default)] error: Option<ErrorBody>,
}
```

**A unit struct serialises as `null`, not `{}`.** For an empty request body an
API expects as an object, write `struct Ping {}`.

**`#[serde(other)]` on a string enum buys forward compatibility** — an unknown
variant deserialises to your catch-all instead of failing the whole response.

**`skip_serializing_if = "Option::is_none"`** so an absent optional is omitted
rather than sent as an explicit `null`.

**A raw string cannot hold `"#`.** `r#"… "#0969da" …"#` terminates early; use
`r##"…"##` for any fixture carrying a hex colour.

**Newtypes over the wire are free.** `#[serde(transparent)]` keeps the JSON a
bare string, and a `Borrow<str>` impl keeps `HashMap<&NewId, _>` probeable with
a `&str`, so joining two responses on an id still needs no cloning.

## Errors

`thiserror` in libraries, `anyhow` in binaries. Beyond that:

**Never restate the `source` in the `Display`.** `anyhow`'s `{:#}` walks the
chain, so a message that includes its own source prints it twice.

```rust
// Wrong: "could not reach the server at /x: Connection refused: Connection refused"
#[error("could not reach the server at {path}: {source}")]

// Right.
#[error("could not reach the server at {path}")]
Connect { path: PathBuf, #[source] source: io::Error },
```

**Name the operation in every variant.** "socket i/o failed" is a bug report
you cannot act on; "session.snapshot failed on the socket" points at the code
to fix. Carry the method, the path, the id — whatever identifies *which* call.

**`#[non_exhaustive]` on a public error enum** is cheap now and impossible
later.

**Never let a swallowed failure look like a negative answer.** `.ok()` on a
query turns "could not read what is in this pane" into "nothing interesting is
in this pane", and the fallback then looks like correct behaviour forever. If
falling back is right, print why:

```rust
let info = match client.pane_process_info(&pane) {
    Ok(info) => Some(info),
    Err(error) => {
        eprintln!("could not read {pane}: {error}");
        None
    }
};
```

## Make the mistake unrepresentable, then reject the rest at construction

In order of preference:

1. **A type that cannot be wrong.** Three kinds of opaque id all typed `String`
   means `pane_neighbor(tab_id, …)` compiles, reaches the server and returns
   `not_found` at a keypress. Newtypes make it a compile error.
2. **A required argument instead of a builder step.** `Cell::new(text, colour)`
   carries "paint every cell explicitly" in the signature; `.fg()` as an
   optional step carries it in a comment nobody reads.
3. **An index that cannot point at the wrong thing.** A cursor over a
   `Vec<usize>` of *selectable* row positions cannot land on a header, so no
   guard is needed anywhere else.
4. **Validation in the constructor, returning an error.** What is left goes
   here — and prefer it over documenting the invariant.

That last one is the difference between a platform and a pile of code. An
opt-in `check_my_layout()` helper with prose saying "a plugin should assert
this is empty" is a convention; the same logic in `build() -> Result<_, Error>`
is enforcement. Anything that would otherwise render *silently* wrong — an item
in an undeclared group vanishing, a cell count disagreeing with its columns —
belongs in the constructor.

## Compute once, not per frame

**A method that is both a data source and a render hook is a smell.** When
`cells(&theme) -> Vec<Cell>` was called once per item to build search text *and*
once per visible row per frame to draw, every implementation cloned every
string in every arm, and nothing owned the mapping between the two.

Materialising at construction fixed the bug and the churn together:

```rust
struct Prepared {
    cells: Vec<Cell>,          // owned once
    haystack: Utf32String,     // pre-encoded once
    bases: Vec<Option<usize>>, // where each searchable cell starts in it
}
```

All three come out of one pass, so they cannot disagree — which is what made
the off-by-one impossible rather than merely fixed. The generic parameter then
drops out of rendering entirely.

The general shape: if two things must stay in step, compute them in one place
and hand out the result. Two loops that agree today are two loops that will
disagree.

## lib plus a thin bin

Every binary crate here is `autobins = false` with an explicit `[lib]` and
`[[bin]]`, all logic in the lib, and a `main.rs` that only maps argv onto it.

The payoff is a **pure transform over one `Deserialize` struct.** Put every
input the transform needs into a single value, and the live path fills it from
the network while the test path fills it from a fixture, with the transform
unable to tell the difference:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct Listings { changed: Vec<String>, numstat: Vec<Numstat>, … }

impl Listings {
    pub fn collect(root: &Path) -> Result<Self>   // does the I/O
    pub fn index(&self, root: &Path) -> Vec<File> // pure, tested
}
```

A function that shells out from *inside* the transform is the anti-pattern.
It was the reason one plugin here had zero tests over its entire index while
its sibling had ten.

## Tests

**Hand-author fixtures on the real shape; do not commit a live capture.** Read
the real response to get the shape right, then write the fixture. A capture
only covers the session that happened to be running, so it silently misses the
branches that matter — and in a public repo it carries real paths, names and
titles.

**Prove the test catches the bug.** After fixing something subtle, revert the
fix and watch the new test fail with the exact symptom, then restore it. A
regression test you have never seen fail is a guess. Restore by editing, not
by `mv`-ing a backup over the file: the backup carries its original mtime,
cargo sees nothing newer than the last build, and the "restored" run is still
the reverted binary — which reads as the fix not working.

**A fake tests your code; only the real thing tests your beliefs.** Against a
protocol or a service, probe the real one first and write down what it actually
does, then build the fake to pin the client to that. Done in the other order the
fake faithfully reproduces your assumptions and every test passes — none of the
surprises worth knowing (a replayed backlog, a subscription that fires on a
timer rather than on a change, a deadline measured against the wrong clock) is
reachable from a stub you wrote yourself.

What the fake is *for* is the half the real thing will not do on request: a
frame split across two reads, a deadline landing mid-message, a hang-up, a
rejection. Make it scriptable — a `Vec<Step>` of `Bytes`/`Line`/`Wait`/`Close`
covers all of them, and it must be able to echo a request id or it can only ever
test the failure paths.

Two traps in writing one, both of which report as a bug in the code under test:

- **Do not hang up when the script ends.** Hold the connection until the fake is
  dropped, or the client's next write fails with `BrokenPipe`.
- **Read the request before answering**, or the reply races the client's write.

**Do not report a syscall failure as being about its arguments.** On macOS
`set_read_timeout` fails with `EINVAL` once the peer has hung up — nothing to do
with the duration. Mapping that to an error about the timeout hid a clean
end-of-file that said exactly what happened. When a call fails for a reason it
cannot express, carry on to the operation that *can*:

```rust
// A peer that has hung up rejects this with EINVAL. Read regardless: the read
// answers a hang-up with an end of file, which is the accurate reason.
let _ = self.stream.set_read_timeout(Some(left));
```

**Assert the property, not the incident.** "every rendered row is exactly the
requested width, at every width, for wide content" outlives "this row is 40
cells". Loop over the interesting inputs inside one test.

**Let a failing test correct you.** Twice here a new test failed because *its
premise* was wrong, not the code — the honest fix was to assert the true
property, which was more useful than what I set out to assert.

## Idioms actually reached for

```rust
let Some(x) = opt else { return … };          // early exit without nesting
if let Some(a) = x && let Some(b) = y { … }   // let chains, no nested ifs
opt.as_ref().is_none_or(|v| v.is_empty())     // over map_or(true, …)
std::iter::repeat_n(' ', n)                   // over repeat().take()
text.split_at(byte).0                         // over chars().take().collect()
slice.get(i)                                  // never slice[i]
fn f(pred: impl Fn(&str) -> bool)             // over a boxed closure
Cow::Borrowed(text)                           // when the common path copies nothing
```

`Cow` earns its place on a hot path whose usual answer is "unchanged":
`truncate` allocating on every cell of every row of every frame, for text that
already fits, is pure waste.
