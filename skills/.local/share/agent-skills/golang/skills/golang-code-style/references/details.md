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
- The struct is **large** (~128+ bytes) — avoids copying overhead
- **Nil is meaningful** (optional/nullable parameter)

**When NOT to use pointers**:

- `string`, `int`, `bool`, `float64`, `time.Time` — pass by value
- Read-only access to small structs — pass by value (better cache locality)
- "Just to save memory" — value copy is negligible; stack allocation is fast

**Memory access trade-offs when strong performance is required**:

- **Values (no pointer)**: Stack allocation, excellent CPU cache locality for small types, zero indirection cost. Slower only when copying large structs.
- **Pointers**: One extra dereference (negligible on modern CPUs), but risk cache misses if pointed-to data isn't in cache. Essential for large structs (>~128 bytes) where copy cost dominates.
- **Rule of thumb**: For structs <~128 bytes with read-only access, values are typically faster due to cache locality. For mutation or large structs, pointers win. When in doubt, benchmark.

-> See the `samber/cc-skills-golang@golang-structs-interfaces` skill for pointer vs value **receiver** rules.
