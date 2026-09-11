# Bottom Propagation

**Bottom (`_|_`) represents errors in CUE, and it propagates through all downstream computations.**

## What is Bottom?

**Bottom is CUE's error value**, written `_|_`. It occurs when constraints conflict or operations fail.

```cue
a: 1 & 2           // _|_ (conflicting values)
b: "tcp" | "udp"
b: "http"          // _|_ (not in disjunction)
c: [1, 2, 3]
c: [1, 2]          // _|_ (different lengths)
```

## Propagation Rule

**Bottom propagates through all computations:**

```cue
a: 1 & 2              // _|_ (conflict)
b: a + 10             // _|_ (a is bottom)
c: b * 2              // _|_ (b is bottom)
d: {
    nested: c         // _|_ (c is bottom)
    other: 42         // Still 42 (unaffected)
}
```

**One bottom poisons the entire dependency chain.**

## Common Bottom Sources

### 1. Conflicting Concrete Values

```cue
x: 1
x: 2
// x: _|_ conflicting values
```

### 2. Impossible Bounds

```cue
x: >100 & <10
// x: _|_ incompatible bounds
```

### 3. Empty Disjunction

```cue
protocol: "tcp" | "udp"
protocol: "http"
// protocol: _|_ empty disjunction
```

### 4. Type Mismatch

```cue
x: int
x: "string"
// x: _|_ conflicting types
```

### 5. Closed Struct Violations

```cue
#Type: {a: int}
x: #Type & {b: 2}
// x.b: _|_ field not allowed
```

### 6. List Length Conflict

```cue
list: [1, 2]
list: [1, 2, 3]
// list: _|_ incompatible lengths
```

### 7. Index Out of Range

```cue
list: [1, 2, 3]
x: list[10]
// x: _|_ index out of range
```

### 8. Required Field Missing

```cue
#Type: {
    required!: string
}

x: #Type
// Export error: required field not present
```

## Viewing Bottom

### With cue eval

```bash
# Default: hides bottom values
cue eval file.cue

# Show bottom with -i flag
cue eval -i file.cue
```

```cue
// file.cue
a: 1 & 2
b: a + 10
```

Output:
```
a: _|_ // a: conflicting values 2 and 1
b: _|_ // a: conflicting values 2 and 1
```

### With cue export

```bash
# Export fails on any bottom
cue export file.cue
# Error: conflicting values...
```

## Isolation Strategies

### 1. Separate Namespaces

```cue
// Error in one field doesn't affect others
config: {
    good: {
        value: 42
    }
    bad: {
        value: 1 & 2  // _|_
    }
}

// good.value is still 42
// bad.value is _|_
```

### 2. Optional Fields

```cue
#Schema: {
    a?: 0
    b?: 1
}

x: #Schema & {
    b?: 0  // b?: 1 & 0 = _|_
}

// Bottom in optional field means "field cannot exist"
// Struct is not bottom, just b is disallowed
```

**Rule**: Optional field bottom disallows field. Required field bottom breaks struct.

## Debugging Bottom

### Step 1: Find the Source

```bash
cue eval -i file.cue
```

Look for first `_|_` in dependency chain.

### Step 2: Check Error Message

```
a: _|_ // a: conflicting values 2 and 1
```

Error message shows:
- What conflicted
- Where it happened (file:line)

### Step 3: Trace Backwards

```cue
a: 1 & 2              // SOURCE: conflict here
b: a + 10             // PROPAGATED
c: b * 2              // PROPAGATED
```

Fix the source, propagation stops.

## Common Patterns

### Validation with Bottom

```cue
#MustBePositive: int & >0

value: #MustBePositive & -5
// value: _|_ (violates constraint)
```

Bottom here is **intentional**—it shows validation failure.

### Error Function

```cue
if condition {
    _|_  // Explicit error
}

// Or with message
if price < 0 {
    _|_ & {error: "price cannot be negative"}
}
```

### Disallow Field

```cue
#Schema: {
    allowedField?: string
    disallowedField?: _|_  // Cannot be specified
}

x: #Schema & {
    allowedField: "ok"
    disallowedField: "value"  // _|_ field not allowed
}
```

## Bottom vs Null

**They are different:**

```cue
// null is a valid value
x: null  // ✓ OK

// bottom is an error
y: _|_   // Represents error state
```

## Bottom in Lists

```cue
list: [1, 2, 3]
list: [1, 99, 3]

// Result: [1, _|_, 3]
// Second element conflicts

// If exported: error on element 1
```

## Bottom in Structs

```cue
person: {
    name: "Alice"
    age: 1 & 2  // _|_
}

// person.name: "Alice" (unaffected)
// person.age: _|_ (bottom)
// Can't export: age has error
```

## Avoiding Propagation

### Use Optional Fields

```cue
#Schema: {
    // Optional means bottom doesn't break struct
    optional?: int & >0
}

x: #Schema & {
    optional?: -5  // _|_ but doesn't break x
}
// x is valid, just optional field disallowed
```

### Separate Concerns

```cue
// Keep error-prone computations isolated
validation: {
    check1: value > 10  // might be _|_
    check2: value < 100 // might be _|_
}

data: {
    value: 42  // Unaffected by validation errors
}
```

## Quick Reference

| Cause | Example | Result |
|-------|---------|--------|
| Conflicting values | `1 & 2` | `_|_` |
| Impossible bounds | `>100 & <10` | `_|_` |
| Empty disjunction | `("a"\|"b") & "c"` | `_|_` |
| Type mismatch | `int & "text"` | `_|_` |
| Closed struct violation | `#T{a:int} & {b:2}` | `_|_` |
| List length conflict | `[1,2] & [1,2,3]` | `_|_` |
| Index out of range | `[1,2,3][10]` | `_|_` |

## Best Practices

1. **Use `-i` flag to see bottom values**: `cue eval -i`
2. **Fix at source**: Don't work around bottom
3. **Isolate experimental code**: Prevent propagation
4. **Use optional for validation**: `field?: constraint`
5. **Check error messages carefully**: They show conflict location
6. **Remember: bottom is an error**: Not a value to work with

## Key Insight

**Bottom is final and infectious.** Once a value becomes bottom, it cannot be "fixed" by further unification—it spreads to everything that depends on it.

Fix the conflict at the source, not downstream.
