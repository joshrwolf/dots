# Bounds

**Bounds constrain values using comparison operators.**

## Syntax

Bounds use comparison operators without left operand:

```cue
// Numeric bounds
age: >0
port: >=1024
timeout: <300
score: <=100
version: !=0

// Combined bounds
age: >0 & <=120
port: >=1024 & <65536
```

## Numeric Bounds

### Basic Comparisons

```cue
positive: >0           // Greater than
nonNegative: >=0       // Greater than or equal
limited: <100          // Less than
capped: <=100          // Less than or equal
notZero: !=0           // Not equal
```

### Combining Bounds

```cue
// Range constraint
score: >=0 & <=100

// Multiple constraints
age: int & >0 & <150

// With default
port: int & >=1024 & <65536 | *8080
```

### Bound Unification

```cue
// More restrictive bound wins
a: >5
a: >10
// Result: a: >10

b: <100
b: <50
// Result: b: <50

// Compatible bounds combine
c: >5 & <100
// Result: c: >5 & <100

// Incompatible → bottom
d: >100 & <10
// Result: d: _|_
```

## String Bounds

**Strings compared lexically, byte-by-byte:**

```cue
// Alphabetical bounds
name: >="A"
name: <"Z"
// Must start with A-Y

// Combined
initial: >="A" & <"B"
// Strings starting with 'A'

// Exact comparison
notEmpty: !=""
```

### String Bound Examples

```cue
// Must be alphabetically after "apple"
fruit: >"apple"
fruit: "banana"  // ✓ "banana" > "apple"

// Lexical range
letter: >="A" & <="Z"
letter: "M"  // ✓

// Not equal
status: !="pending"
status: "active"  // ✓
```

## Bytes Bounds

Same as strings but for byte sequences:

```cue
data: bytes & >='\x00' & <='\xff'
```

## Null Bounds

```cue
// Not null
value: !=null

// This excludes null from type union
notNull: (int | string | null) & !=null
// Simplifies to: int | string
```

## Combining with Types

```cue
// Type + bound + default
age: int & >=0 & <=120 | *25

// Type union + bound
value: (int | float) & >0

// String type + pattern + bound
name: string & =~"^[A-Z]" & !=""
```

## Predefined Bounds

CUE provides predefined bound identifiers:

```cue
uint8: >=0 & <=255
uint16: >=0 & <=65535
uint32: >=0 & <=4294967295

int8: >=-128 & <=127
int16: >=-32768 & <=32767
int32: >=-2147483648 & <=2147483647
```

Usage:

```cue
import "math"

age: uint8       // Same as: int & >=0 & <=255
temp: int16
```

## Bound Semantics

### Type Inference

Bounds infer types:

```cue
x: >5.0    // Type: float (bound has decimal)
y: >5      // Type: int | float (bound is int)
```

### Bound Domains

```cue
// Integer bounds work on ints and floats
a: int & >10
b: float & >10.0
c: number & >10  // Works on both

// String bounds work on strings only
s: string & >"A"
```

## Common Patterns

### Port Numbers

```cue
#Port: int & >=1024 & <65536 | *8080
```

### Percentages

```cue
#Percentage: number & >=0 & <=100 | *0
```

### Positive Integers

```cue
#PositiveInt: int & >0
```

### Non-Empty String

```cue
#NonEmptyString: string & !=""
```

### Age Validation

```cue
#Age: int & >=0 & <=120
```

### Score with Decimals

```cue
#Score: number & >=0.0 & <=100.0
```

## Bounds in Schemas

```cue
#Config: {
    timeout: int & >0 & <300 | *30
    retries: int & >=0 & <=10 | *3
    priority: int & >=1 & <=5 | *3
}
```

## Gotchas

### 1. Impossible Bounds

```cue
// Conflicting bounds → bottom
x: >100 & <10
// Result: x: _|_
```

### 2. Bound Type Mismatch

```cue
// String value doesn't satisfy numeric bound
x: >10
x: "hello"
// Result: x: _|_
```

### 3. Inclusive vs Exclusive

```cue
// Inclusive
a: >=10  // Includes 10

// Exclusive
b: >10   // Excludes 10

// Range inclusive both sides
c: >=10 & <=20
```

### 4. Float vs Int Bounds

```cue
// Int bound allows floats
x: >5
x: 5.5  // ✓ OK

// Float bound
y: >5.0
y: 6    // ✓ OK (int compatible with float)
```

## Bounds vs Regex

For strings, choose appropriately:

```cue
// Bound for lexical comparison
initial: >="A" & <"Z"

// Regex for pattern matching
name: =~"^[A-Z][a-z]+"

// Combine both
validName: string & >="A" & =~"^[A-Z][a-z]{2,}$"
```

## Quick Reference

| Operator | Meaning | Types |
|----------|---------|-------|
| `>` | Greater than | number, string, bytes |
| `>=` | Greater or equal | number, string, bytes |
| `<` | Less than | number, string, bytes |
| `<=` | Less or equal | number, string, bytes |
| `!=` | Not equal | all types including null |

## Best Practices

1. **Combine with types**: `int & >0` not just `>0`
2. **Add bounds to schemas**: Better validation
3. **Use with defaults**: `int & >0 | *10`
4. **Prefer specific bounds**: `>=1024 & <65536` over just `>0`
5. **Document constraints**: Why this bound exists
6. **Use predefined bounds**: `uint8` clearer than `int & >=0 & <=255`
