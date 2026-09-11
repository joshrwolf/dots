# Open vs Closed Structs

**Understanding when structs are open or closed is critical to avoiding "field not allowed" errors.**

## The Rules

### Regular Structs are OPEN

```cue
config: {
    host: "localhost"
}

// Can add fields later
config: {
    port: 8080  // ✓ OK - struct is open
}
```

### Definitions are CLOSED

```cue
#Config: {
    host: string
}

// Cannot add unknown fields
data: #Config & {
    host: "localhost"
    port: 8080  // ✗ Error: field not allowed
}
```

## Opening Closed Structs

**Use `...` to explicitly open:**

```cue
#OpenConfig: {
    required: string
    ...  // Allow any additional fields
}

data: #OpenConfig & {
    required: "value"
    extra: 42  // ✓ OK now
}
```

## Closing Open Structs

**Use `close()` builtin:**

```cue
open: {
    a: int
}

closed: close(open)

// Cannot add fields to closed
x: closed & {
    a: 1
    b: 2  // ✗ Error: field not allowed
}
```

**Warning**: `close()` is permanent. Cannot reopen later.

## Detection

### How to Tell if Struct is Closed

1. **From definition**: `#Type` → closed
2. **From `close()`**: `close({...})` → closed
3. **Otherwise**: open

```cue
#Closed: {a: int}        // Closed (definition)
closed: close({a: int})  // Closed (close builtin)
open: {a: int}           // Open (regular struct)
```

## Common Scenarios

### Scenario 1: Definition with Extra Fields

```cue
#Person: {
    name: string
    age: int
}

// ✗ This fails
person: #Person & {
    name: "Alice"
    age: 30
    email: "alice@example.com"  // Not allowed
}

// ✓ Fix: open the definition
#Person: {
    name: string
    age: int
    ...  // Allow extra fields
}
```

### Scenario 2: Schema Composition

```cue
#Base: {
    id: string
}

#Extended: #Base & {
    extra: int  // ✗ Error: #Base is closed
}

// ✓ Fix: open #Base
#Base: {
    id: string
    ...
}
```

### Scenario 3: Incremental Definition

```cue
#Config: {
    host: string
}

// ✗ Can't add fields later
#Config: {
    port: int  // Error: field not allowed
}

// Definitions are closed immediately
```

## Pattern Constraints vs Closedness

**Pattern constraints allow matching fields even in closed structs:**

```cue
#Config: {
    required: string
    [string]: int  // Pattern allows any string field with int value
}

data: #Config & {
    required: "value"
    extra1: 42     // ✓ Matches pattern
    extra2: 99     // ✓ Matches pattern
    bad: "text"    // ✗ Doesn't match pattern (not int)
}
```

## Closedness with Embedding

```cue
#Base: {
    a: int
}

// ✗ #Extended is also closed
#Extended: {
    #Base
    b: int  // Error: field not allowed in #Base
}

// ✓ Fix: open base or embed differently
#Base: {
    a: int
    ...
}

#Extended: {
    #Base
    b: int  // Now OK
}
```

## Openness Inheritance

```cue
#Open: {
    a: int
    ...
}

// Embedding preserves openness
#Child: {
    #Open
    b: int
    // ... not needed, inherited from #Open
}

child: #Child & {
    a: 1
    b: 2
    extra: "allowed"  // ✓ OK - #Child is open via #Open
}
```

## When to Use Each

### Use Open Structs For:

- Data that may have arbitrary fields
- Partial validation (only care about some fields)
- Extensible schemas
- Progressive refinement

### Use Closed Structs For:

- Strict validation (reject unknown fields)
- API contracts
- Type safety
- Preventing typos

## Common Patterns

### Validation Schema (Closed)

```cue
#User: {
    username!: string
    email!: string
    age?: int
}

// Rejects typos and unknown fields
user: #User & {
    username: "alice"
    email: "alice@example.com"
    emial: "typo"  // ✗ Caught: field not allowed
}
```

### Extensible Schema (Open)

```cue
#BaseConfig: {
    version: string
    ...  // Allow custom fields
}

config: #BaseConfig & {
    version: "1.0"
    customField: "allowed"
    anything: true
}
```

### Hybrid: Required + Optional

```cue
#Config: {
    // Required fields
    name!: string
    type!: string

    // Optional fields (via pattern)
    [=~"^metadata_"]: string

    // Open for anything else
    ...
}

config: #Config & {
    name: "service"
    type: "api"
    metadata_author: "alice"  // Matches pattern
    extra: 42                 // Allowed by ...
}
```

## Gotchas

### 1. Cannot Reopen After Close

```cue
#Closed: {a: int}

// ✗ Can't add ... later
#Closed: {...}  // Error: field not allowed
```

Once closed, always closed.

### 2. Close Propagates in Unification

```cue
#Closed: {a: int}
open: {b: int, ...}

combined: #Closed & open
// combined is CLOSED (more restrictive wins)

combined: {c: int}  // ✗ Error: field not allowed
```

### 3. Empty Closed Struct

```cue
#Empty: {}  // Closed, no fields allowed

x: #Empty & {a: 1}  // ✗ Error: field not allowed

// vs

empty: {}  // Open, fields can be added
empty: {a: 1}  // ✓ OK
```

### 4. Definition Composition

```cue
// Both closed
#A: {a: int}
#B: {b: int}

// Union is closed too
#C: #A & #B
// #C has both fields, still closed

#C: {c: int}  // ✗ Error: field not allowed
```

## Top-Level Package Openness

**Top-level of a package is OPEN:**

```cue
package config

// These are top-level regular fields (open)
name: string
age: int

// Can add more fields
extra: "anything"
```

This is why `cue vet` allows extra fields in data by default.

## Testing Closedness

```bash
# This will fail if struct is closed and has extra fields
cue export data.json schema.cue -d '#Schema'
```

## Quick Decision Tree

```
Is it a definition (#Type)?
├─ Yes → CLOSED (unless has ...)
└─ No → Is it close(struct)?
    ├─ Yes → CLOSED
    └─ No → OPEN
```

## Quick Reference

| Pattern | Open/Closed | Can Add Fields? |
|---------|-------------|-----------------|
| `{a: int}` | Open | ✓ Yes |
| `#Type: {a: int}` | Closed | ✗ No |
| `#Type: {a: int, ...}` | Open | ✓ Yes |
| `close({a: int})` | Closed | ✗ No |
| Top-level package | Open | ✓ Yes |

## Best Practices

1. **Default to open for flexibility**: Add `...` to definitions
2. **Close for strict validation**: Use definitions without `...`
3. **Use pattern constraints**: Allow sets of fields in closed structs
4. **Document closedness**: Make intent clear
5. **Test with actual data**: Ensure closedness matches needs
6. **Open base schemas**: Close only at validation boundaries

## Key Insight

**Closedness is about validation strictness:**
- Open = permissive (partial validation)
- Closed = strict (full validation)

Choose based on whether you want to catch unknown fields or allow extension.
