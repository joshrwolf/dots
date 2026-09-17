# Comparison options

Import `github.com/google/go-cmp/cmp` and, for the options below,
`github.com/google/go-cmp/cmp/cmpopts`.

| Contract | Option | Check before using |
| --- | --- | --- |
| Nil and empty collections are equivalent | `cmpopts.EquateEmpty()` | This changes all matching collections in scope, including nested ones. |
| Results have no ordering guarantee | `cmpopts.SortSlices(less)` | Preserve duplicate counts; do not hide an ordering regression. |
| A generated field is outside the comparison | `cmpopts.IgnoreFields(Record{}, "GeneratedAt")` | Check that field separately if its validity matters. |
| Floating-point results permit rounding error | `cmpopts.EquateApprox(fraction, margin)` | Derive tolerances from the computation, not the failing result. |
| A type has domain-specific equality | `cmp.Comparer(func(x, y T) bool { ... })` | Keep it deterministic, symmetric, and free of side effects. |

## Unexported fields

Compare a public view when the representation is not part of the contract.
`cmpopts.IgnoreUnexported(T{})` ignores direct unexported fields of the specified
type; nested types need their own treatment. Ignoring every meaningful field can
make unequal values compare equal. For an owned type whose internals are the
subject of the test, `cmp.AllowUnexported(T{})` is a deliberate alternative.
Do not broadly export third-party internals to silence a panic.

For protocol buffer messages, use the protobuf package's `protocmp.Transform()`
when that dependency is present, rather than comparing generated internals.
For errors, prefer `errors.Is` or `errors.As` in the test itself.

## Unordered results

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
