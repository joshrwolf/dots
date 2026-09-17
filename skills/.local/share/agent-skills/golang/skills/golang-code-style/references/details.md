# Code Style Details

## Extract Complex Conditions

Name a condition when the name adds meaning; a count of operands is not a reason
to extract it. Straightforward checks can remain inline:

```go
if x == nil || y == nil || z == nil {
    return errMissingInput
}
```

Preserve short-circuit evaluation. Hoisting a function call can introduce work,
side effects, or a panic on a path that previously skipped it:

```go
if isAdmin || isOwner || expensivePermissionCheck(user, resource) {
    allow()
}
```

Follow the requested form and surrounding style. Avoid inventing temporary boolean
variables when they only restate the expression.

## Value vs Pointer Arguments

This covers **function parameters**, not method receivers (see `samber/cc-skills-golang@golang-structs-interfaces` skill for receiver rules).

Pass small, fixed-size types by value — strings are already a (pointer, length) pair internally:

```go
// Good — value types by value
func FormatUser(name string, age int, createdAt time.Time) string

// Good — pointer for mutation
func PopulateDefaults(cfg *Config)

// Good — pointer when nil is meaningful (optional field update)
func UpdateUser(ctx context.Context, id string, name *string) error

// Bad — pointer for no reason
func Greet(name *string) string
```

**When to use pointers**:

- The function **mutates** the value
- Copying a large struct is a measured cost; a pointer may avoid it
- **Nil is meaningful** (optional/nullable parameter)

**When NOT to use pointers**:

- `string`, `int`, `bool`, `float64`, `time.Time` — pass by value
- Read-only access to small structs — values are a good default
- "Just to save memory" without measuring copies, escape behavior, and allocation costs

**Memory access trade-offs when strong performance is required**:

- **Values** copy their fields. Allocation placement depends on escape analysis, inlining, and the surrounding code; passing by value does not guarantee stack allocation.
- **Pointers** can avoid copying but introduce aliasing and may affect escape analysis, cache locality, and GC work. Dereference costs are not universally negligible.
- There is no portable byte-size cutoff where pointers become faster. Choose ownership and mutation semantics first, then benchmark representative workloads when performance matters.

-> See the `samber/cc-skills-golang@golang-structs-interfaces` skill for pointer vs value **receiver** rules.
