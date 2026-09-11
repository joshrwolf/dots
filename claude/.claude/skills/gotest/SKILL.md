---
name: gotest
description: Write or modify Go tests the right way. MUST be loaded before writing, modifying, or refactoring ANY Go test — covers table-driven subtests through public APIs, fakes over mocks, go-cmp (never testify), testable-by-design refactoring, and when NOT to write a test. Triggers on any work touching `*_test.go`, `Test*`/`Benchmark*`/`Fuzz*` functions, or "write/add/fix/update tests".
argument-hint: "[scope: file, package, function, or description of what to test]"
---

# gotest

Analyze Go code and write or refactor tests. The user provides a scope (file, package,
function, etc.) as the argument.

## Process

1. **Read the code.** Use the argument to find the relevant files. Read both the
   production code and any existing tests. When tests already exist, extend the
   existing table and match its style and helpers — don't create a parallel
   `TestX2` or a second table for the same function. Skip code that isn't worth
   testing: trivial getters/setters, generated code, and thin pass-throughs with
   no logic.

2. **Identify testable boundaries.** Find the exported functions and methods that
   should be tested. For each, determine:
   - What codepaths exist (success, error cases, edge cases, boundary conditions)
   - Whether the function is testable as-is through its public signature
   - What dependencies it has and how they're injected

3. **If the code is hard to test**, refactor it first:
   - Functions that call external systems directly → accept interfaces for those dependencies
   - Functions with too many responsibilities → break into focused units
   - Functions depending on global state → inject dependencies through parameters or struct fields
   - Results only observable through side effects → return values instead

   Load the `idiomatic-go` skill for design guidance. Any refactoring should follow
   the type-oriented design philosophy — don't just extract helper functions.

4. **Write table-driven tests** for each public function. Each test should:
   - Live in a `Test*` function targeting one exported function or method
   - Use a table of `struct` cases with named fields (`name`, inputs, expected outputs, `wantErr`)
   - Iterate with `t.Run(tt.name, func(t *testing.T) { ... })`
   - Cover the meaningful codepaths: happy path, error cases, edge cases, zero values
   - Use `cmp.Diff` for complex comparisons, not testify
   - Use `t.Context()` when the function takes a context
   - Use fakes (simple interface implementations) for dependencies, not mock frameworks

5. **Run the tests** with `go test -race -v ./path/to/package` to verify they pass.

## What good tests look like

```go
func TestProcessor_Process(t *testing.T) {
    tests := []struct {
        name    string
        order   Order
        want    Receipt
        wantErr bool
    }{
        {
            name:  "valid order",
            order: Order{UserID: 1, Items: []Item{{Price: 10, Qty: 2}}},
            want:  Receipt{UserID: 1, Total: 20},
        },
        {
            name:    "empty items",
            order:   Order{UserID: 1},
            wantErr: true,
        },
        {
            name:    "zero user",
            order:   Order{Items: []Item{{Price: 10, Qty: 1}}},
            wantErr: true,
        },
    }

    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            p := NewProcessor(&fakeRepo{}, slog.Default())
            got, err := p.Process(t.Context(), tt.order)
            if (err != nil) != tt.wantErr {
                t.Fatalf("Process() error = %v, wantErr %v", err, tt.wantErr)
            }
            if tt.wantErr {
                return
            }
            if diff := cmp.Diff(tt.want, got); diff != "" {
                t.Errorf("Process() mismatch (-want +got):\n%s", diff)
            }
        })
    }
}
```

## What to avoid

- **Testing private functions directly** — test the exported function that calls them.
  If a private function is complex enough to need its own tests, it should probably be
  exported (possibly in its own package).
- **Mock frameworks** — if you need one, the API boundary is wrong. Accept interfaces,
  write simple fakes.
- **Tests coupled to implementation** — if renaming an internal function breaks tests,
  those tests are testing implementation, not behavior.
- **testify** — never. Use standard library assertions and go-cmp.
- **Duplicating existing tests** — extend the existing table instead of adding a
  parallel `Test*` for a function that's already covered.
- **Testing trivial code** — getters/setters with no logic, generated code, and
  pass-throughs. A test that can only fail if the compiler is broken is noise.
- **Reflexive `Example*` functions** — examples are documentation, not coverage.
  Write one only for an exported API clearly intended for external consumers
  where the usage is non-obvious and the rendered `go doc` output earns its keep.
  Don't add them to packages meant for internal use, and never to pad coverage.
