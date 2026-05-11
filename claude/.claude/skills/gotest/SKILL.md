---
name: gotest
description: Analyze Go code and write or refactor tests. Identifies testable boundaries, writes table-driven subtests through public APIs, and refactors code for testability when needed.
argument-hint: "[scope: file, package, function, or description of what to test]"
---

# gotest

Analyze Go code and write or refactor tests. The user provides a scope (file, package,
function, etc.) as the argument.

## Process

1. **Read the code.** Use the argument to find the relevant files. Read both the
   production code and any existing tests.

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
        wantErr bool
    }{
        {
            name:  "valid order",
            order: Order{UserID: 1, Items: []Item{{Price: 10, Qty: 2}}},
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
            err := p.Process(t.Context(), tt.order)
            if (err != nil) != tt.wantErr {
                t.Fatalf("Process() error = %v, wantErr %v", err, tt.wantErr)
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
