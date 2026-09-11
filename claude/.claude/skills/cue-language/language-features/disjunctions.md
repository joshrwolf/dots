# Disjunctions

**Disjunctions define "one of" constraints using the `|` operator.**

## Basic Syntax

```cue
// Value must be one of these
protocol: "tcp" | "udp" | "sctp"

// Type union
value: int | string | null

// Schema alternatives
config: #OptionA | #OptionB
```

## Disjunction Types

### Unordered Disjunction

No preference marked:

```cue
protocol: "tcp" | "udp" | "sctp"

// Must resolve to exactly one for export
protocol: "tcp"  // ✓ Resolves to "tcp"
```

### Default Disjunction

One element marked with `*`:

```cue
env: "dev" | "staging" | "prod" | *"dev"

// If no other constraint, defaults to "dev"
config: {
    env: "dev" | "staging" | "prod" | *"dev"
}
// Result: env: "dev"

// Override default
config: {
    env: "dev" | "staging" | "prod" | *"dev"
}
config: {
    env: "prod"
}
// Result: env: "prod"
```

## Export Rules

**For export, disjunction must resolve to exactly ONE element:**

```cue
// ✗ Can't export: ambiguous
x: 1 | 2

// ✓ Can export: resolved
x: 1 | 2
x: 1
// Result: x: 1

// ✓ Can export: default selected
y: int | *1
// Result: y: 1
```

## Disjunction Unification

### Narrowing Disjunctions

```cue
// Unification narrows options
a: int | string
a: int
// Result: a: int

b: "a" | "b" | "c"
b: "a" | "b"
// Result: b: "a" | "b"

c: "a" | "b"
c: "a"
// Result: c: "a"
```

### Empty Disjunction = Bottom

```cue
// No common elements → _|_
x: "tcp" | "udp"
x: "http" | "https"
// Result: x: _|_ (empty disjunction)
```

## Type Unions

```cue
// Value can be multiple types
value: int | string | null

// Unify with concrete type
value: 42
// Result: value: 42 (int branch selected)

// Nullable pattern
optional: string | null | *null
```

## Schema Alternatives

```cue
#TypeA: {
    kind: "a"
    fieldA: string
}

#TypeB: {
    kind: "b"
    fieldB: int
}

#Config: #TypeA | #TypeB

// Must match one schema completely
config1: #Config & {
    kind: "a"
    fieldA: "value"
}

config2: #Config & {
    kind: "b"
    fieldB: 42
}
```

## Discriminated Unions

Use a discriminator field to select schema:

```cue
#Base: {
    type!: string
}

#Server: #Base & {
    type: "server"
    host: string
    port: int
}

#Client: #Base & {
    type: "client"
    endpoint: string
}

#Config: #Server | #Client

serverConfig: #Config & {
    type: "server"
    host: "localhost"
    port: 8080
}
```

## Default Selection Rules

CUE selects defaults only when unambiguous:

```cue
// Single default → selected
a: int | *1
// Result: a: 1

// Default overridden
b: int | *1
b: 2
// Result: b: 2

// Conflicting defaults → none selected
c: int | *1
d: int | *2
c: d
// Result: c: int, d: int (remain abstract)

// Same defaults → selected
e: int | *1
f: int | *1
e: f
// Result: e: 1, f: 1
```

## Disjunction Simplification

CUE simplifies disjunctions when possible:

```cue
// Duplicate elements removed
x: 1 | 1 | 2
// Simplifies to: x: 1 | 2

// Type subsumption
y: int | 42
// Simplifies to: y: int (42 is an int)

// Bound subsumption
z: >5 | >10
// Simplifies to: z: >5 (>10 is subset of >5)
```

## Null Coalescing

Default disjunction with expressions:

```cue
pet: {
    species?: string
}

// Use species if set, otherwise "cat"
display: *pet.species | "cat"

// If pet.species is null or undefined → "cat"
// If pet.species is "dog" → "dog"
```

## Enum Pattern

```cue
#LogLevel: "debug" | "info" | "warn" | "error"

#Config: {
    logLevel: #LogLevel | *"info"
}

config: #Config & {
    logLevel: "debug"
}
```

## Sum Types

```cue
#StringOrInt: string | int

#Result: {
    value: #StringOrInt
}

result1: #Result & {value: "text"}
result2: #Result & {value: 42}
```

## Common Patterns

### Optional with Multiple Types

```cue
#Config: {
    timeout?: int | string  // Can be duration int or "30s" string
}
```

### Fallback Chain

```cue
value: *option1 | option2 | option3

// Tries option1 (default), falls back if conflicts
```

### Validation Alternatives

```cue
#Email: string & =~"^.+@.+\\..+$"
#Phone: string & =~"^\\+?[0-9]{10,}$"

#Contact: #Email | #Phone

contact: #Contact & "+12125551234"  // Matches #Phone
```

## Gotchas

### 1. Export Requires Resolution

```cue
// ✗ Can't export
x: 1 | 2

// Must resolve first
x: 1 | 2
x: 1  // Now x: 1 can export
```

### 2. All Elements Must Fail for Bottom

```cue
x: "a" | "b" | "c"
x: "d"
// Result: _|_ (none match)

y: int | string
y: "hello"
// Result: y: "hello" (string matches)
```

### 3. Default Conflicts

```cue
// Both have different defaults → neither selected
a: int | *1
b: int | *2
a: b
// Result: both remain int (abstract)
```

### 4. Disjunction of Constraints

```cue
// This is NOT "greater than 5 OR less than 10"
// It's "value in set {>5} OR value in set {<10}"
x: >5 | <10
x: 7
// Result: x: 7 (satisfies >5)

// For "AND", use &
y: >5 & <10
y: 7
// Result: y: 7 (satisfies both)
```

## Quick Reference

| Pattern | Meaning |
|---------|---------|
| `A \| B` | A or B |
| `A \| B \| C` | One of A, B, or C |
| `int \| string` | Type union |
| `*A \| B` | A is default, B is alternative |
| `A \| *B` | B is default, A is alternative |
| `#T1 \| #T2` | Schema alternatives |
| `"a" \| "b"` | Enum (value alternatives) |

## Best Practices

1. **Use for enums**: `"tcp" | "udp" | "sctp"` clear and type-safe
2. **Mark defaults explicitly**: `*"default" | "other"` not just `"default" | "other"`
3. **Type unions for flexibility**: `int | string` when both valid
4. **Discriminated unions for schemas**: Use `type` or `kind` field
5. **Remember export rules**: Must resolve to one element
6. **Simplify when possible**: Let CUE simplify, don't manually
