# Definitions

**Definitions are CUE's mechanism for reusable schemas. They create closed structs by default.**

## Syntax

```cue
#TypeName: {
    field: type
}

// Hidden definition (package-private)
_#InternalType: {
    field: type
}
```

## Key Properties

1. **Closed by default**: Reject unknown fields
2. **Not exported**: Don't appear in output
3. **Can contain abstract types**: `string`, `int`, etc. allowed
4. **Reusable**: Can be referenced multiple times

## Basic Example

```cue
#Person: {
    name!: string
    age?: int & >=0
}

alice: #Person & {
    name: "Alice"
    age: 30
}

bob: #Person & {
    name: "Bob"
    // age omitted (optional)
}
```

## Closedness

**Definitions create closed structs:**

```cue
#Config: {
    host: string
    port: int
}

config: #Config & {
    host: "localhost"
    port: 8080
    timeout: 30  // ✗ Error: field not allowed
}
```

**To allow additional fields, explicitly open:**

```cue
#Config: {
    host: string
    port: int
    ...  // Allow any additional fields
}

config: #Config & {
    host: "localhost"
    port: 8080
    timeout: 30  // ✓ OK now
}
```

## Regular Fields vs Optional vs Required

```cue
#Example: {
    // Regular field - must be concrete for export
    regular: string

    // Optional field constraint - only applies if field exists
    optional?: string

    // Required field constraint - must exist for export
    required!: string
}
```

## Hidden Definitions

```cue
// Package-private (not visible to importers)
_#internal: {
    secret: string
}

// Can use within package
config: _#internal & {
    secret: "xyz"
}
```

## Embedding Definitions

```cue
#Base: {
    name: string
    created: string
}

#Extended: {
    #Base  // Embed all fields
    extra: int
}

// Equivalent to:
#Extended: {
    name: string
    created: string
    extra: int
}
```

## Pattern Constraints in Definitions

```cue
#Service: {
    name: string

    // All string-keyed fields must match this
    [string]: {
        enabled: bool | *true
    }
}
```

## Definitions with Defaults

```cue
#Server: {
    host: string | *"localhost"
    port: int & >=1024 & <65536 | *8080
    tls: bool | *false
}

server: #Server
// Result: {host: "localhost", port: 8080, tls: false}

prodServer: #Server & {
    host: "prod.example.com"
    tls: true
}
// Result: {host: "prod.example.com", port: 8080, tls: true}
```

## Recursive Definitions

```cue
#Tree: {
    value: _
    left?: #Tree
    right?: #Tree
}

tree: #Tree & {
    value: 10
    left: {
        value: 5
    }
    right: {
        value: 15
        right: {
            value: 20
        }
    }
}
```

## Disjunction of Definitions

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

// Must match one
config1: #Config & {
    kind: "a"
    fieldA: "value"
}

config2: #Config & {
    kind: "b"
    fieldB: 42
}
```

## Common Patterns

### Discriminated Union

```cue
#Base: {
    kind!: string
}

#OptionA: #Base & {
    kind: "a"
    dataA: string
}

#OptionB: #Base & {
    kind: "b"
    dataB: int
}

#Config: #OptionA | #OptionB
```

### Validation Schema

```cue
#Email: string & =~"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$"

#User: {
    username!: string & =~"^[a-z0-9_]{3,20}$"
    email!: #Email
    age?: int & >=13
}
```

### Schema with Business Rules

```cue
#Product: {
    name!: string
    price!: number & >0

    // Conditional requirements
    if price > 100 {
        approvalRequired!: bool
        approvedBy?: string
    }
}
```

## Definition vs Regular Struct

```cue
// Definition - closed, not exported, reusable
#Schema: {
    field: string
}

// Regular struct - open, exported, concrete needed
data: {
    field: "value"
}
```

## Gotchas

### 1. Unintended Closure

```cue
#Config: {
    port: int
}

// This fails if data has extra fields
data: #Config & {
    port: 8080
    extra: "value"  // ✗ Error
}

// Fix: open the definition
#Config: {
    port: int
    ...
}
```

### 2. Cannot Reopen

```cue
#Closed: {
    a: int
}

// Cannot add fields elsewhere
#Closed: {
    b: int  // ✗ Error: field not allowed
}

// Must open initially
#Open: {
    a: int
    ...
}

#Open: {
    b: int  // ✓ OK
}
```

### 3. Definitions Don't Export

```cue
#Schema: {
    value: int | *10
}

// Exporting #Schema directly exports nothing
// Must instantiate:
config: #Schema
// Now exports: {value: 10}
```

## When to Use Definitions

**Use definitions for:**
- Schemas for validation
- Reusable types
- Enforcing closed structs
- Package-level contracts

**Don't use definitions for:**
- Final concrete data (use regular fields)
- Open-ended collections (use regular structs)
- Values that need to export directly

## Best Practices

1. **Start with definitions for schemas**: `#TypeName` not `typeName`
2. **Explicitly open if needed**: Add `...` if extra fields allowed
3. **Use `!` for required fields**: `field!: type` makes intent clear
4. **Use `?` for truly optional**: `field?: type` for validation-only
5. **Keep definitions abstract**: Use `type | *default` pattern
6. **Name definitions with PascalCase**: `#PersonConfig` not `#person_config`

## Integration with Validation

```cue
#Schema: {
    name!: string
    age!: int & >=0
}

// Validate data
data: #Schema & {
    name: "Alice"
    age: 30
}

// Use with cue vet
// $ cue vet schema.cue data.json
```

## Quick Reference

| Pattern | Meaning |
|---------|---------|
| `#Type: {...}` | Definition (closed) |
| `_#Type: {...}` | Hidden definition |
| `#Type: {field: type}` | Field constraint |
| `#Type: {field!: type}` | Required field |
| `#Type: {field?: type}` | Optional field |
| `#Type: {...}` | Allow extra fields |
| `#A \| #B` | Disjunction of definitions |
