# Structs

**Structs (also called maps) are CUE's primary composite type.**

## Basic Syntax

```cue
person: {
    name: "Alice"
    age: 30
}
```

## Open vs Closed

**By default, structs are OPEN** (accept any fields):

```cue
config: {
    host: "localhost"
    // Any other fields allowed
}

config: {
    port: 8080  // ✓ OK - struct is open
}
```

**Definitions create CLOSED structs** (reject unknown fields):

```cue
#Config: {
    host: string
    port: int
}

data: #Config & {
    host: "localhost"
    port: 8080
    timeout: 30  // ✗ Error: field not allowed
}
```

**Explicitly close with `close()` or open with `...`**:

```cue
// Close a regular struct
closed: close({
    a: int
})

// Open a definition
#Open: {
    required: string
    ...  // Allow additional fields
}
```

## Field Types

### Regular Field

```cue
field: value  // Must be concrete for export
```

### Optional Field Constraint

```cue
field?: type  // Only constrains if field appears elsewhere
```

**Critical**: `field?: string` does NOT mean "optional with no value"—it means "if this field exists, it must be string."

### Required Field Constraint

```cue
field!: type  // Must be present for export
```

### Example

```cue
#Person: {
    name!: string         // Required
    age?: int & >=0      // Optional, but if present must be >=0
    email: string        // Regular (must be concrete)
}

// Valid
person1: #Person & {
    name: "Alice"
    email: "alice@example.com"
    // age omitted (optional)
}

// Invalid - missing required field
person2: #Person & {
    email: "bob@example.com"
    // ✗ Error: name is required
}
```

## Pattern Constraints (Templates)

Apply constraints to multiple fields matching a pattern:

```cue
services: {
    // Apply to ALL string-named fields
    [string]: {
        replicas: int | *1
        enabled: bool | *true
    }
}

services: {
    api: {replicas: 3}
    web: {}  // Gets defaults: {replicas: 1, enabled: true}
}
```

**With aliases:**

```cue
jobs: {
    [Name=string]: {
        name: Name  // Use field name inside constraint
        command: string | *"exec \(Name)"
    }
}

jobs: {
    nginx: {}
    // Result: {name: "nginx", command: "exec nginx"}
}
```

## Embedding

Anonymous embedding includes all fields:

```cue
#Base: {
    created: string
    updated: string
}

#Extended: {
    #Base  // Embed all fields from #Base
    extra: int
}

// Equivalent to:
#Extended: {
    created: string
    updated: string
    extra: int
}
```

## Struct Unification

Fields unify recursively:

```cue
A: {
    a: int
    nested: {
        x: 1
    }
}

A: {
    b: string
    nested: {
        y: 2
    }
}

// Result:
A: {
    a: int
    b: string
    nested: {
        x: 1
        y: 2
    }
}
```

## Dynamic Fields

Fields computed from expressions use parentheses:

```cue
key: "myField"

struct: {
    (key): "value"  // Dynamic field name
}

// Result: {myField: "value"}
```

## Conditional Fields

```cue
price: number

// Fields added conditionally
if price > 100 {
    discountApplied: bool
    discountAmount: number
}
```

## Optional Field with Bottom Gotcha

```cue
#Schema: {
    a?: 0
    b?: 1
}

x: #Schema & {
    a?: 0  // Unifies: 0 & 0 = 0
    b?: 0  // Conflict: 1 & 0 = _|_
}

// Result: x has field a, but b is disallowed
// Optional field with _|_ means "field cannot exist"
```

**Rule**: Optional field bottom disallows the field. Required field bottom breaks the struct.

## Struct Validation Patterns

### Closed Validation Schema

```cue
#User: {
    username!: string & =~"^[a-z0-9_]{3,20}$"
    email!: string & =~"@"
    age?: int & >=13
}

// Rejects unknown fields
user: #User & {
    username: "alice"
    email: "alice@example.com"
    age: 25
}
```

### Open Validation Schema

```cue
#Config: {
    required!: string
    ...  // Allow any other fields
}

// Accepts additional fields
config: #Config & {
    required: "value"
    optional1: 123
    optional2: "anything"
}
```

### Multi-Level Constraints

```cue
#Database: {
    host!: string
    port!: int
}

#Database: {
    host: string | *"localhost"
    port: int & >=1024 & <65536 | *5432
}

#Database: {
    host: !="prod-db"  // Policy: can't use prod
}

// All constraints unify
db: #Database & {
    host: "test-db"
}
```

## Struct Instance Checking

```cue
// Check if value is instance of struct
A: {a: int}
A: {b: string}

// A is instance of both {a: int} and {b: string}
// Result: A: {a: int, b: string}
```

## Struct Operations

### len()

```cue
config: {
    a: 1
    b: 2
}

count: len(config)  // 2
```

### close()

```cue
open: {a: int}
closed: close(open)

// closed rejects additional fields
```

## Common Patterns

### Configuration with Layers

```cue
// Base layer - structure
#Config: {
    database: {
        host!: string
        port!: int
    }
}

// Defaults layer
#Config: {
    database: {
        host: string | *"localhost"
        port: int | *5432
    }
}

// Instance
config: #Config & {
    database: {
        host: "prod-db"
    }
}
```

### Service Registry

```cue
services: {
    [name=string]: {
        name: name
        port: int & >=1024 & <65536
        replicas: int & >0 | *1
    }
}

services: {
    api: {port: 8080, replicas: 3}
    web: {port: 3000}  // replicas: 1 (default)
}
```

### Discriminated Union

```cue
#Config: {
    type!: string
}

#TypeA: #Config & {
    type: "a"
    fieldA: string
}

#TypeB: #Config & {
    type: "b"
    fieldB: int
}

#AnyConfig: #TypeA | #TypeB

config: #AnyConfig & {
    type: "a"
    fieldA: "value"
}
```

## Gotchas

### 1. Open vs Closed Default

```cue
// Regular struct - OPEN
x: {a: int}
x: {b: int}  // ✓ OK

// Definition - CLOSED
#X: {a: int}
#X: {b: int}  // ✗ Error: field not allowed
```

### 2. Cannot Reopen

```cue
#Closed: {a: int}
// Cannot add fields later - already closed
```

### 3. Pattern Constraint Scope

```cue
services: {
    [string]: {
        port: int
    }
}

// This applies to ALL services, including existing ones
services: api: {host: "localhost"}
// api now must have port: int
```

### 4. Embedding vs Unification

```cue
// Embedding
#A: {
    #B  // Includes all fields from #B
    c: int
}

// Unification
#A: #B & {
    c: int
}

// Often equivalent, but embedding is clearer for schemas
```

## Quick Reference

| Syntax | Meaning |
|--------|---------|
| `{field: value}` | Regular struct (open) |
| `#Type: {...}` | Definition (closed) |
| `field: value` | Regular field |
| `field?: type` | Optional constraint |
| `field!: type` | Required constraint |
| `[string]: type` | Pattern constraint |
| `[Name=string]: {...}` | Pattern with alias |
| `{...}` | Explicitly open |
| `close({...})` | Explicitly close |
| `#Base` | Embed definition |

## Best Practices

1. **Use definitions for schemas**: Create `#Type` not `type`
2. **Be explicit about open vs closed**: Add `...` or use definitions
3. **Use `!` for required fields**: Makes intent clear
4. **Pattern constraints for bulk operations**: Apply rules to many fields
5. **Keep base layers abstract**: Use `type | *default` in schemas
