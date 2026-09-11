# Defaults vs Constraints

**The single most important pattern in idiomatic CUE.**

## The Problem

Coming from other languages, you might write:

```cue
port: 8080
```

This creates a **concrete value** that cannot be changed without causing `_|_`. This is almost never what you want.

## The Solution

**Always use the default disjunction pattern:**

```cue
port: int | *8080
```

This means "must be an int, defaults to 8080 if nothing else specified."

## The Pattern

```cue
field: <constraint> | *<default>
```

Where:
- `<constraint>` is a type or bound (e.g., `int`, `string`, `>=0`, `=~"pattern"`)
- `*<default>` is the marked default value

## Examples

### Basic Types

```cue
// ❌ WRONG
host: "localhost"

// ✅ CORRECT
host: string | *"localhost"

// ❌ WRONG
enabled: true

// ✅ CORRECT
enabled: bool | *true
```

### With Bounds

```cue
// ❌ WRONG
port: 8080

// ✅ BETTER
port: int | *8080

// ✅ BEST
port: int & >=1024 & <65536 | *8080
```

### Multiple Constraints

```cue
// Combine bounds, patterns, and defaults
version: string & =~"^v[0-9]+\\.[0-9]+\\.[0-9]+$" | *"v1.0.0"

// Multiple bounds
score: number & >=0 & <=100 | *50.0

// Enum with default
env: "dev" | "staging" | "prod" | *"dev"
```

## Why This Matters

### Composability

```cue
// config1.cue
port: int | *8080

// config2.cue
port: int | *3000

// Result: port remains int (no default picked - conflict)
```

vs

```cue
// config1.cue
port: 8080

// config2.cue
port: 3000

// Result: _|_ (conflicting concrete values)
```

With defaults, CUE refuses to pick arbitrarily. With concrete values, you get an error.

### Refinement

```cue
// Base schema
#Service: {
    port: int | *8080
}

// Specific service
api: #Service & {
    // Inherits default 8080
}

web: #Service & {
    port: 3000  // Overrides default
}
```

## Common Mistakes

### Mistake 1: Default Without Constraint

```cue
// ❌ BAD: No type constraint
port: *8080

// ✅ GOOD: Type constraint with default
port: int | *8080
```

Without the type constraint, `port` could unify with anything.

### Mistake 2: Using Optional Fields as Defaults

```cue
// ❌ WRONG: This is NOT a default
port?: 8080

// ✅ CORRECT: This is a default
port: int | *8080
```

`port?: 8080` means "if port field appears, it must be 8080." It's a constraint, not a default.

### Mistake 3: Concrete in Shared Schemas

```cue
// ❌ BAD: Everyone gets the same concrete value
#Config: {
    timeout: 30
}

// ✅ GOOD: Everyone can override the default
#Config: {
    timeout: int | *30
}
```

## Default Selection Rules

CUE only picks a default when unambiguous:

```cue
// Unambiguous: picks default
a: int | *1
// Result: a: 1

// Ambiguous: no default picked
a: int | *1
b: int | *2
a: b
// Result: a: int, b: int (defaults conflict)

// Unambiguous after unification
a: int | *1
b: int | *1
a: b
// Result: a: 1, b: 1 (same default)
```

## Nested Defaults

```cue
#Server: {
    host: string | *"localhost"
    port: int & >=1024 & <65536 | *8080
    tls: {
        enabled: bool | *false
        cert?: string
        key?: string
    }
}

// Using defaults
server: #Server
// Result:
// server: {
//     host: "localhost"
//     port: 8080
//     tls: {enabled: false}
// }

// Overriding some defaults
server: #Server & {
    port: 9000
    tls: enabled: true
}
```

## Conditional Defaults

Sometimes you want defaults to depend on other values:

```cue
#Config: {
    env: "dev" | "prod" | *"dev"

    // Default depends on env
    if env == "dev" {
        debug: bool | *true
    }
    if env == "prod" {
        debug: bool | *false
    }
}
```

## Edge Cases

### Empty Values

```cue
// Empty string default
name: string | *""

// Empty list default
items: [...string] | *[]

// Empty struct default
config: {...} | *{}
```

### Null as Default

```cue
// Nullable with null default
value: int | null | *null

// vs Optional field (different meaning)
value?: int
```

## Quick Rules

1. **Never use bare concrete values in schemas**
2. **Always pair defaults with type constraints**
3. **Put most specific constraint first, default last**
4. **Use `|` for default disjunction, not `&`**
5. **Remember: concrete values can't be "overridden", only unified**

## Idiom Checklist

- [ ] All schema fields use `type | *default` pattern?
- [ ] Concrete values only in final configurations, not reusable schemas?
- [ ] Defaults are compatible with their constraints?
- [ ] Multiple defaults don't conflict when unified?
