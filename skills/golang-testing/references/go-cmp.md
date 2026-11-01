# Structural comparisons in Go tests

Use `github.com/google/go-cmp/cmp` when a nested value needs a readable failure
diff. Keep scalar, error, length, and nil checks explicit with `testing.T`.

```go
if diff := cmp.Diff(want, got); diff != "" {
	t.Errorf("LoadConfig() mismatch (-want +got):\n%s", diff)
}
```

Keep expected values first and actual values second. Include the operation in
the failure message. Report with `t.Fatalf` only if continuing would make later
checks invalid. Avoid a separate `cmp.Equal` call before `cmp.Diff` and avoid
wrapping comparisons in a generic assertion helper.

## Preserve the contract

Start with the default comparison. Add an option only when the tested contract
requires different equality semantics, and keep it local to that test.

- Nil and empty collections differ by default. Use `cmpopts.EquateEmpty()` only
  if that distinction is irrelevant to the API being tested.
- Slice order matters by default. Use `cmpopts.SortSlices` only for unordered
  results, with a deterministic ordering that distinguishes relevant elements.
- Types with an `Equal` method may define their own equality. Consider that
  method before adding a comparer or exposing representation details.
- Unexported fields can cause a panic. Prefer a public projection or a semantic
  comparer; use narrowly scoped `AllowUnexported` only for types you own.
- A nonempty diff is a test failure. Its formatting is diagnostic output;
  do not parse it or store it as a golden contract.

Use go-cmp in tests; it is not a production validation API.

API references: [cmp](https://pkg.go.dev/github.com/google/go-cmp/cmp),
[cmpopts](https://pkg.go.dev/github.com/google/go-cmp/cmp/cmpopts).

## Options

Import `github.com/google/go-cmp/cmp` and, for the options below,
`github.com/google/go-cmp/cmp/cmpopts`.

| Contract | Option | Check before using |
| --- | --- | --- |
| Nil and empty collections are equivalent | `cmpopts.EquateEmpty()` | This changes all matching collections in scope, including nested ones. |
| Results have no ordering guarantee | `cmpopts.SortSlices(less)` | Preserve duplicate counts; do not hide an ordering regression. |
| A generated field is outside the comparison | `cmpopts.IgnoreFields(Record{}, "GeneratedAt")` | Check that field separately if its validity matters. |
| Floating-point results permit rounding error | `cmpopts.EquateApprox(fraction, margin)` | Derive tolerances from the computation, not the failing result. |
| A type has domain-specific equality | `cmp.Comparer(func(x, y T) bool { ... })` | Keep it deterministic, symmetric, and free of side effects. |

### Unexported fields

Compare a public view when the representation is not part of the contract.
`cmpopts.IgnoreUnexported(T{})` ignores direct unexported fields of the specified
type; nested types need their own treatment. Ignoring every meaningful field can
make unequal values compare equal. For an owned type whose internals are the
subject of the test, `cmp.AllowUnexported(T{})` is a deliberate alternative.
Do not broadly export third-party internals to silence a panic.

For protocol buffer messages, use the protobuf package's `protocmp.Transform()`
when that dependency is present, rather than comparing generated internals.
For errors, prefer `errors.Is` or `errors.As` in the test itself.

### Unordered results

```go
func TestUnorderedRecords(t *testing.T) {
	type record struct {
		ID   string
		Name string
	}
	want := []record{{ID: "a", Name: "Alice"}, {ID: "b", Name: "Bob"}}
	got := []record{{ID: "b", Name: "Bob"}, {ID: "a", Name: "Alice"}}
	byID := cmpopts.SortSlices(func(a, b record) bool { return a.ID < b.ID })
	if diff := cmp.Diff(want, got, byID); diff != "" {
		t.Errorf("records mismatch (-want +got):\n%s", diff)
	}
}
```

This assumes IDs are unique. If duplicates are valid, choose a complete ordering
for the relevant fields so equal IDs cannot mask an ambiguous pairing.

Options compose, but overlapping comparers or transformers may be ambiguous.
Use `cmp.FilterPath` or `cmp.FilterValues` to limit an option when multiple values
of the same type need different treatment. Keep the predicate narrowly tied to
the contract and explain the reason at its call site.
