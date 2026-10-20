---
name: golang-go-cmp
description: Compare structured values in Go tests with google/go-cmp. Use for readable diffs, semantic equality, comparison options, or unexported-field panics.
license: MIT
---

# Structural comparisons in Go tests

Use `github.com/google/go-cmp/cmp` when a nested value needs a readable failure
diff. Keep scalar, error, length, and nil checks explicit with `testing.T`;
`golang-testing` covers those checks and test structure.

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

For option selection and examples, read [Comparison options](references/options.md).
Use go-cmp in tests; it is not a production validation API.

API references: [cmp](https://pkg.go.dev/github.com/google/go-cmp/cmp),
[cmpopts](https://pkg.go.dev/github.com/google/go-cmp/cmp/cmpopts).
