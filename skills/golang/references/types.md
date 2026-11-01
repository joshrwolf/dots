# Types, interfaces and struct tags

## Choosing a type

- Concrete types for fixed domains, interfaces for behavior consumers share, and
  type parameters when an operation preserves a relationship across types. `any`
  fits genuinely heterogeneous values and dynamic boundaries.
- Make the zero value useful (`bytes.Buffer`, `sync.Mutex`). For map fields,
  lazily initialize in the mutating method or require the constructor.
- Keep interfaces small and compose larger ones (`io.ReadWriteCloser`). Do not
  split an interface whose consumer really needs the whole contract.
- Return concrete types unless the contract deliberately hides the implementation.
  Canonical method names carry signatures: `String() string`, `Error() string`
  and `Read([]byte) (int, error)`.
- `var _ io.ReadWriter = (*Buffer)(nil)` documents and checks an intended
  implementation when nothing else would catch drift.

## Type assertions and switches

- Use comma-ok when a mismatch is expected or recoverable. A single-value
  assertion panics; use it only for an established invariant.
- In a type switch, list concrete types before interfaces they satisfy. A nil
  interface matches `case nil`; a typed nil pointer matches its type instead.
- Detect optional behavior with an assertion to a small interface
  (`if f, ok := w.(interface{ Flush() error }); ok`), not a concrete type. The
  standard library does this with `http.Flusher`, `io.WriterTo`, `io.ReaderFrom`.
- For errors, assertions miss wrapped values; use `errors.AsType` or `errors.As`.

## Embedding

- Embed to promote the inner type's full API ("is an enhanced X"); use a named
  field for an internal dependency ("has an X"). Embedding a type exports its
  methods as part of yours, including `Lock`/`Unlock` for an embedded mutex.
- Promoted methods run with the inner value as receiver.

## Receivers

- Use pointer receivers when the method mutates, the type holds a mutex or other
  non-copyable state, or copies are costly. Value receivers suit small immutable types.
- Keep a type's receivers consistent unless mixing is deliberate, and remember
  that `T` and `*T` have different method sets when satisfying interfaces.

## Copy safety

Never copy a used `sync.Mutex` or a struct containing one; `go vet` `copylocks`
catches many cases. For a documented non-copyable type without a lock, a named
`noCopy` field with `Lock`/`Unlock` methods enables the same vet check. It is a
static-analysis convention, not enforcement.

## Struct tags

- Tag exported fields of serialized types so renaming a Go field does not change
  the wire format.
- `encoding/json` v1 `omitempty` drops `0`, `""`, `false`, nil and empty
  collections, but never a zero struct. `omitzero` (Go 1.24+) drops zero values,
  including zero structs and `time.Time` via `IsZero`, but keeps empty non-nil
  slices and maps (`[]`, `{}`). Switching tags therefore changes output; do it
  only when that is the intended wire contract. Neither tracks presence:
  use a pointer with `omitempty` when absent and explicit zero must differ.
- Unexported fields are ignored by `encoding/json`; tags on them are a vet warning.
- Malformed tags compile silently; `go vet` `structtag` reports them. A
  marshal/unmarshal round-trip test catches field-name drift.
