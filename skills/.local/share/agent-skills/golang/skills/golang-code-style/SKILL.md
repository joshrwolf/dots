---
name: golang-code-style
description: "Review Go code for clarity and style. Use when writing or reviewing control flow, declarations, formatting choices, or project coding conventions."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.0"
  openclaw:
    emoji: "🎨"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent
paths:
  - "**/*.go"
---

> Apply repository instructions and required project skills first. These guidelines are defaults where the project leaves a choice.

# Go Code Style

Style rules that require human judgment — linters handle formatting, this skill handles clarity. For naming see `samber/cc-skills-golang@golang-naming` skill; for design patterns see `samber/cc-skills-golang@golang-design-patterns` skill; for struct/interface design see `samber/cc-skills-golang@golang-structs-interfaces` skill.

> "Clear is better than clever." — Go Proverbs

Follow applicable project instructions and formatting tools before these defaults. Explain meaningful constraints in comments, not routine stylistic choices.

## Line Length & Breaking

Wrap at semantic boundaries when it improves readability, following project conventions and explicit requests. Argument count and line length alone do not require wrapping. A call with long arguments can benefit from this layout:

```go
// Good — each argument on its own line, closing paren separate
mux.HandleFunc("/api/users", func(w http.ResponseWriter, r *http.Request) {
    handleUsers(
        w,
        r,
        serviceName,
        cfg,
        logger,
        authMiddleware,
    )
})
```

Group related options when that improves the API, not merely to shorten a line. Use the project formatter for mechanical formatting.

## Variable Declarations

SHOULD use `:=` for non-zero values, `var` for zero-value initialization. The form signals intent: `var` means "this starts at zero."

```go
var count int              // zero value, set later
name := "default"          // non-zero, := is appropriate
var buf bytes.Buffer       // zero value is ready to use
```

### Slice & Map Initialization

Use nil slices when the zero value fits the operation. Initialize a map before
writing to it; read-only use does not require allocating an empty map. Preserve
API contracts: when an empty JSON array is required, produce a non-nil empty slice
at that boundary. Do not change existing null/empty behavior as a style cleanup.

```go
var users []User                       // zero value; ready for append
users = append(users, user)
lookup := make(map[string]int, len(items)) // allocate before writes
results := make([]User, 0, len(ids))    // known expected capacity
```

When using `make` and expected capacity is known, supply it.

Do not preallocate speculatively — `make([]T, 0, 1000)` wastes memory when the common case is 10 items.

### Composite Literals

Composite literals MUST use field names — positional fields break when the type adds or reorders fields:

```go
srv := &http.Server{
    Addr:         ":8080",
    ReadTimeout:  5 * time.Second,
    WriteTimeout: 10 * time.Second,
}
```

## Control Flow

### Reduce Nesting

Errors and edge cases MUST be handled first (early return). Keep the happy path at minimal indentation:

```go
func process(data []byte) (*Result, error) {
    if len(data) == 0 {
        return nil, errors.New("empty data")
    }

    parsed, err := parse(data)
    if err != nil {
        return nil, fmt.Errorf("parsing: %w", err)
    }

    return transform(parsed), nil
}
```

### Eliminate Unnecessary `else`

When the `if` body ends with `return`/`break`/`continue`, the `else` MUST be dropped. Use default-then-override for simple assignments — assign a default, then override with independent conditions or a `switch`:

```go
// Good — default-then-override with switch (cleanest for mutually exclusive overrides)
level := slog.LevelInfo
switch {
case debug:
    level = slog.LevelDebug
case verbose:
    level = slog.LevelWarn
}

// Bad — else-if chain hides that there's a default
if debug {
    level = slog.LevelDebug
} else if verbose {
    level = slog.LevelWarn
} else {
    level = slog.LevelInfo
}
```

### Complex Conditions & Init Scope

Extract a condition when its name explains a domain concept or makes the logic easier to follow. Keep simple checks inline; do not introduce single-use variables merely to meet an operand threshold. Preserve evaluation order and short-circuit behavior, including nil guards and side effects. [Details](./references/details.md)

```go
// Good — named booleans make intent clear
isAdmin := user.Role == RoleAdmin
isOwner := resource.OwnerID == user.ID
isPublicVerified := resource.IsPublic && user.IsVerified
if isAdmin || isOwner || isPublicVerified || permissions.Contains(PermOverride) {
    allow()
}
```

Scope variables to `if` blocks when only needed for the check:

```go
if err := validate(input); err != nil {
    return err
}
```

### Switch Over If-Else Chains

When comparing the same variable multiple times, prefer `switch`:

```go
switch status {
case StatusActive:
    activate()
case StatusInactive:
    deactivate()
default:
    panic(fmt.Sprintf("unexpected status: %d", status))
}
```

## Function Design

- Keep a cohesive operation readable in one place. Length alone does not justify splitting a function or method.
- Extract a helper or method when it has a meaningful contract that lets callers understand the step without reading its implementation. Reuse, encapsulated complexity, or a resource lifetime can justify a single-use helper; merely shortening its caller or naming a sequence of steps does not. Apply the same standard to receiver methods and free functions.
- Choose parameters for the operation's actual inputs. Group related values when they form a meaningful concept, not to satisfy a parameter-count limit. See `golang-structs-interfaces` for types that own state and behavior, and `golang-design-patterns` for constructor configuration.
- **Parameter order**: `context.Context` first, then inputs, then output destinations.
- Naked returns help in very short functions (1-3 lines) where return values are obvious, but become confusing when readers must scroll to find what's returned — name returns explicitly in longer functions.

```go
func FetchUser(ctx context.Context, id string) (*User, error)
func SendEmail(ctx context.Context, msg EmailMessage) error  // grouped into struct
```

### Prefer `range` for Iteration

SHOULD use `range` over index-based loops. Use `range n` (Go 1.22+) for simple counting.

```go
for _, user := range users {
    process(user)
}
```

## Value vs Pointer Arguments

Pass small types (`string`, `int`, `bool`, `time.Time`) by value. Use pointers when mutating, when avoiding a measured large-struct copy cost, or when nil is meaningful. [Details](./references/details.md)

## Code Organization Within Files

- **Group related declarations**: keep a type, its constructor, methods, and closely related supporting types together so readers can follow the implementation locally.
- **File placement**: extend the existing file that owns the behavior. A new type or helper does not automatically need a new file. Split only for a distinct logical responsibility or a technical constraint; file size alone is not a smell. See `golang-project-layout` for file boundaries and `golang-testing` for test placement.
- **Blank imports** (`_ "pkg"`) register side effects (init functions). Restricting them to `main` and test packages makes side effects visible at the application root, not hidden in library code
- **Dot imports** pollute the namespace and make it impossible to tell where a name comes from — never use in library code
- **Unexport aggressively** — you can always export later; unexporting is a breaking change. → See `samber/cc-skills-golang@golang-gopls` skill to unexport safely — semantic rename checks resolved references and visible interface conversions; review dynamic assertions, reflection, and external consumers before changing visibility.

## String Handling

Use `strconv` for simple conversions (faster), `fmt.Sprintf` for complex formatting. Use `%q` in error messages to make string boundaries visible. Use `strings.Builder` for loops, `+` for simple concatenation.

## Type Conversions

Prefer explicit, narrow conversions. Use generics over `any` when a concrete type will do:

```go
func Contains[T comparable](slice []T, target T) bool  // not []any
```

## Philosophy

- **"A little copying is better than a little dependency"**
- **Use `slices` and `maps` standard packages**; use explicit loops for transformations not covered by the standard library
- **"Reflection is never clear"** — avoid `reflect` unless necessary
- **Don't abstract prematurely** — extract when the pattern is stable
- **Minimize public surface** — every exported name is a commitment

## Organizing Code Style Reviews

For a requested style review, organize findings by relevant concern: control flow, function design, declarations, strings, and code organization.

## Enforce with Linters

Many rules are enforced automatically: `gofmt`, `gofumpt`, `goimports`, `gocritic`, `revive`, `wsl_v5`. → See the `samber/cc-skills-golang@golang-lint` skill.

## Cross-References

- → See the `samber/cc-skills-golang@golang-naming` skill for identifier naming conventions
- → See the `samber/cc-skills-golang@golang-structs-interfaces` skill for pointer vs value receivers, interface design
- → See the `samber/cc-skills-golang@golang-design-patterns` skill for functional options, builders, constructors
- → See the `samber/cc-skills-golang@golang-lint` skill for automated formatting enforcement
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for automated AI-driven code review in CI using these guidelines
- → See `samber/cc-skills-golang@golang-refactoring` skill for mechanically applying guard-clause conversion, function extraction, and options-struct migration safely across many call sites once a review surfaces violations at scale
