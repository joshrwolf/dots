# Values and Unification

**Unification is CUE's core operation. Understanding it is essential.**

## Core Concept

In CUE, you don't assign or override values—you **unify** them. Each declaration adds constraints that narrow possibilities until a concrete value emerges (or bottom if constraints conflict).

## What is Unification?

**Unification** finds the most specific value that satisfies all constraints:

```cue
a: int          // a must be an integer
a: >10          // a must be greater than 10
a: <100         // a must be less than 100
a: 42           // a is 42

// Result: a: 42
// All constraints satisfied
```

## Order Independence

**Critical**: Order doesn't matter. These are equivalent:

```cue
// Version 1
x: int
x: >0
x: 5

// Version 2
x: 5
x: >0
x: int

// Version 3
x: >0
x: int
x: 5

// All produce: x: 5
```

This is proof that CUE isn't doing "assignment"—it's solving constraints.

## The Unification Operator

Explicit unification uses `&`:

```cue
x: int & >10 & <100 & 42
// Same as declaring multiple times

y: {a: 1} & {b: 2}
// Result: {a: 1, b: 2}

z: [1, 2] & [int, int]
// Result: [1, 2]
```

## Types ARE Values

In CUE's lattice, everything is a value at different levels of specificity:

```
    _           (top - any value)
    ↓
   int          (any integer)
    ↓
   >10          (integers > 10)
    ↓
   42           (specific integer)
    ↓
   _|_          (bottom - no value)
```

## Unification Rules

### Basic Types

```cue
// Same concrete value → that value
a: 1 & 1           // 1

// Different concrete values → bottom
b: 1 & 2           // _|_

// Type and instance → instance (if compatible)
c: int & 42        // 42
d: string & "hi"   // "hi"

// Incompatible types → bottom
e: int & "hello"   // _|_
```

### Bounds

```cue
// Multiple bounds combine
a: >5 & <100        // Numbers between 5 and 100
b: >=10 & <=20      // Numbers between 10 and 20 inclusive

// Compatible bounds narrow
c: >5 & >10         // >10 (more restrictive)
d: <100 & <50       // <50 (more restrictive)

// Incompatible bounds → bottom
e: >100 & <10       // _|_ (impossible)
```

### Structs

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

### Lists

Elements unify position-by-position:

```cue
list: [1, 2, 3]
list: [int, int, int]
// Result: [1, 2, 3]

list2: [1, _, 3]
list2: [_, 2, _]
// Result: [1, 2, 3]

// Open lists
list3: [1, ...int]
list3: [int, 2, 3, ...]
// Result: [1, 2, 3, ...int]
```

## Disjunctions

Disjunction (`|`) means "one of these":

```cue
// Type union
value: int | string

// Concrete options (enum)
color: "red" | "green" | "blue"

// For export, must resolve to one
x: 1 | 2        // ✗ Can't export (ambiguous)
x: 1 | 2
x: 1            // ✓ Can export: 1
```

## Defaults in Unification

Defaults (marked with `*`) are selected when unambiguous:

```cue
// Single default
port: int | *8080
// Result: port: 8080

// Default overridden
port: int | *8080
port: 9000
// Result: port: 9000

// Conflicting defaults → no default selected
a: int | *1
b: int | *2
a: b
// Result: a: int, b: int (not concrete)

// Same default → selected
a: int | *1
b: int | *1
a: b
// Result: a: 1, b: 1
```

## Concrete vs Abstract

**Concrete**: Fully specified, ready for export:
```cue
name: "Alice"
age: 30
ports: [8080, 3000]
```

**Abstract**: Has constraints, not fully specified:
```cue
name: string
age: int & >0
ports: [...int]
```

**Semi-concrete**: Mix of both:
```cue
config: {
    host: "localhost"  // Concrete
    port: int          // Abstract
}
```

## Progressive Refinement

Start broad, get specific:

```cue
// Step 1: Very general
x: number

// Step 2: Narrow to integers
x: int

// Step 3: Add bounds
x: >0

// Step 4: More specific bounds
x: >0 & <100

// Step 5: Concrete
x: 42

// All steps unify to: x: 42
```

## Unification vs Assignment

### Assignment (Other Languages)

```python
x = 10      # x is 10
x = 20      # x is now 20 (replaced)
```

### Unification (CUE)

```cue
x: 10       // x must be 10
x: 20       // x must also be 20
// Result: _|_ (conflict - can't be both)
```

**The fix:**

```cue
x: int      // x must be int
x: 10       // x is 10
// Result: x: 10 (10 is an int, constraints satisfied)
```

## Common Unification Patterns

### Type Narrowing

```cue
value: number        // Any number
value: int           // Narrows to integer
value: >0            // Positive integer
value: <100          // Positive integer < 100
value: 42            // Specific value
```

### Struct Merging

```cue
config: {
    database: {...}
}

config: database: {
    host: string
}

config: database: {
    host: "localhost"
    port: 5432
}

// Result: config: database: {host: "localhost", port: 5432}
```

### Conditional Unification

```cue
env: "prod"

config: {
    if env == "prod" {
        ssl: true
    }
    if env != "prod" {
        ssl: false
    }
}
```

## Bottom Propagation

Bottom (`_|_`) propagates through computations:

```cue
a: 1 & 2           // _|_ (conflict)
b: a + 10          // _|_ (a is bottom)
c: {
    nested: b      // _|_ (b is bottom)
}
// Entire struct affected by one bottom
```

## Unification Examples

### Success Cases

```cue
// Compatible types
a: int & 42                    // 42

// Struct merging
b: {x: 1} & {y: 2}            // {x: 1, y: 2}

// List unification
c: [1, 2] & [int, int]        // [1, 2]

// Bound combination
d: >5 & <10 & 7               // 7

// Disjunction narrowing
e: (int | string) & int       // int
```

### Failure Cases (Bottom)

```cue
// Conflicting concrete values
a: 1 & 2                      // _|_

// Incompatible types
b: int & "string"             // _|_

// Impossible bounds
c: >100 & <10                 // _|_

// List length mismatch
d: [1, 2] & [1, 2, 3]        // _|_

// Closed struct with extra field
#S: {x: int}
e: #S & {x: 1, y: 2}          // _|_ (y not allowed)
```

## Unification in Practice

### Schema Validation

```cue
#Person: {
    name: string
    age: int & >=0
}

// Unify instance with schema
alice: #Person & {
    name: "Alice"
    age: 30
}
// Validates: alice conforms to #Person
```

### Configuration Layers

```cue
// Base
config: {
    timeout: int | *30
}

// Override
config: {
    timeout: 60
}

// Result: config: {timeout: 60}
```

### Multi-Source Constraints

```cue
// schema.cue
value: int

// policy.cue
value: >0

// business.cue
value: <1000

// config.cue
value: 42

// All unify: value: 42
```

## Key Insights

1. **Unification is commutative**: `A & B` = `B & A`
2. **Order doesn't matter**: Declarations can appear anywhere
3. **Concrete is final**: Can't make concrete values less specific
4. **Bottom propagates**: One error affects downstream
5. **Defaults are special**: Selected only when unambiguous

## Mental Model

Think of unification as **intersection** of possibilities:

```
All integers: ────────────────────
Positive:           ──────────────
Less than 100:  ────────────
Intersection:       ──────────    (0 < x < 100)
Concrete:              █          (42)
```

Each constraint narrows the set of valid values until one remains.

## Quick Reference

| Operation | Result |
|-----------|--------|
| `A & A` | A |
| `A & B` (compatible) | Most specific |
| `A & B` (incompatible) | _|_ |
| `type & value` | value (if compatible) |
| `bound & bound` | Stricter bound |
| `{a:1} & {b:2}` | {a:1, b:2} |
| `[1,2] & [int,int]` | [1,2] |
