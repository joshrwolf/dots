# Lists

**Lists define sequences of values. They can be closed (fixed length) or open (variable length).**

## Basic Syntax

```cue
// Closed list (exact length)
numbers: [1, 2, 3]

// Open list (variable length)
numbers: [1, 2, 3, ...int]  // At least 3 ints
numbers: [...int]           // Any number of ints
```

## Closed Lists

Fixed length specified every time:

```cue
items: [1, 2, 3]

// Must be exactly 3 elements
items: [int, int, int]

// Different lengths conflict
list1: [1, 2]
list2: [1, 2, 3]
unified: list1 & list2  // ✗ Error: incompatible lengths
```

## Open Lists

Variable length with type constraints:

```cue
// Any number of integers
numbers: [...int]

// At least 2 strings
names: [string, string, ...]

// At least 3 ints, then any number more
values: [1, 2, 3, ...int]

// Mixed types
mixed: [string, int, ...bool]  // string, int, then any bools
```

**Ellipsis MUST be last:**

```cue
// ✗ INVALID
items: [1, ..., 3]

// ✓ VALID
items: [1, 3, ...]
```

## Building Lists Incrementally

```cue
// Start open
list: [...]

// Add type constraint
list: [...int]

// Add specific elements
list: [1, ...]

// Add more specificity
list: [1, 2, ...]

// Result: [1, 2, ...int]
```

## List Elements

Access by index (0-based):

```cue
items: [10, 20, 30]

first: items[0]   // 10
second: items[1]  // 20
last: items[2]    // 30
```

## List Unification

Elements unify position-by-position:

```cue
A: [1, 2, 3]
A: [int, int, int]
// Result: [1, 2, 3]

B: [1, 2, {a: 1}]
B: [int, int, {b: 2}]
// Result: [1, 2, {a: 1, b: 2}]

// Open lists unify
C: [1, 2, ...int]
C: [int, int, 3, 4, ...]
// Result: [1, 2, 3, 4, ...int]
```

## List Constraints

```cue
// Type constraint
items: [...string]

// Length constraint (closed list)
triplet: [_, _, _]  // Exactly 3 elements

// Specific positions with wildcards
pattern: [1, _, 3, ...]  // First is 1, third is 3

// Combined
typed: [string, string, ...int]
```

## List Comprehensions

Generate lists from expressions:

```cue
// Basic
squares: [
    for x in [1, 2, 3, 4, 5]
    {x * x}
]
// Result: [1, 4, 9, 16, 25]

// With filter
evens: [
    for x in [1, 2, 3, 4, 5]
    if x % 2 == 0
    {x}
]
// Result: [2, 4]

// With let
transformed: [
    for x in [1, 2, 3]
    let squared = x * x
    let doubled = squared * 2
    {doubled}
]
// Result: [2, 8, 18]

// Multiple loops
pairs: [
    for x in [1, 2]
    for y in ["a", "b"]
    {"\(x)\(y)"}
]
// Result: ["1a", "1b", "2a", "2b"]
```

## List Functions

Import `list` package for operations:

```cue
import "list"

numbers: [3, 1, 4, 1, 5]

sorted: list.Sort(numbers, list.Ascending)
// [1, 1, 3, 4, 5]

max: list.Max(numbers)  // 5
min: list.Min(numbers)  // 1
avg: list.Avg(numbers)  // 2.8

concat: list.Concat([[1, 2], [3, 4]])
// [1, 2, 3, 4]

unique: list.UniqueItems(numbers)
// true or false

contains: list.Contains(numbers, 4)
// true
```

## Common Patterns

### List of Definitions

```cue
#Item: {
    name: string
    value: int
}

items: [...#Item]

items: [
    {name: "a", value: 1},
    {name: "b", value: 2},
]
```

### Optional Elements with Defaults

```cue
#Config: {
    ports: [...int] | *[8080, 3000]
}

config: #Config
// ports: [8080, 3000]

config2: #Config & {
    ports: [9000]
}
// ports: [9000]
```

### Validate List Elements

```cue
#ValidatedList: [...int & >0 & <100]

valid: #ValidatedList & [10, 20, 30]    // ✓
invalid: #ValidatedList & [10, 200, 30] // ✗ 200 out of range
```

### Accumulate from Comprehension

```cue
data: [
    {name: "a", value: 10},
    {name: "b", value: 20},
    {name: "c", value: 30},
]

names: [
    for item in data
    {item.name}
]
// ["a", "b", "c"]

total: list.Sum([
    for item in data
    {item.value}
])
// 60
```

## Empty Lists

```cue
// Empty closed list
empty: []

// Empty open list with type
empty: [...string]

// Default to empty
items: [...int] | *[]
```

## List Length

```cue
import "list"

items: [1, 2, 3]
count: len(items)  // 3

// Constrain length
exactlyThree: [...int] & list.MaxItems(3) & list.MinItems(3)
atLeastTwo: [...string] & list.MinItems(2)
```

## Gotchas

### 1. Ellipsis Position

```cue
// ✗ WRONG: Ellipsis not at end
items: [1, ..., 3]

// ✓ CORRECT
items: [1, 3, ...]
```

### 2. Closed List Conflicts

```cue
A: [1, 2]
B: [1, 2, 3]
C: A & B  // ✗ Error: incompatible lengths (2 and 3)
```

### 3. Open List Unification

```cue
// These unify by taking the most specific
A: [...]        // Any elements
B: [...int]     // Must be ints
C: [1, ...]     // First is 1, rest ints

// A & B & C = [1, ...int]
```

### 4. Empty List vs Open List

```cue
// Concrete empty closed list
empty1: []

// Abstract open list (no elements yet)
empty2: [...int]

// These are different!
// empty1 unifies with [] only
// empty2 can unify with [1], [1,2], etc.
```

### 5. Cannot Index Open Boundaries

```cue
items: [...int]
// items[100] may or may not exist
// Be careful with indexing variable-length lists
```

## Quick Reference

| Pattern | Meaning |
|---------|---------|
| `[1, 2, 3]` | Closed list (3 elements) |
| `[...T]` | Open list of type T |
| `[T, T, ...]` | At least 2 elements of type T |
| `[1, 2, ...T]` | First two specific, rest type T |
| `[_, _, _]` | Exactly 3 elements (any type) |
| `list[i]` | Access element at index i |
| `len(list)` | List length |
| `list.Sort(x, cmp)` | Sort list |
| `list.Max(x)` | Maximum element |

## List Validation Patterns

```cue
import "list"

#StrictList: {
    items: [...int] &
           list.MinItems(1) &
           list.MaxItems(10) &
           list.UniqueItems()
}

// Each element validated
#ValidatedList: [...int & >0 & <100]

// Sorted requirement
#SortedList: [...int] & list.IsSorted()
```

## Best Practices

1. **Use `[...T]` for variable-length lists**: Not bare `[]`
2. **Constrain element types**: `[...string]` not just `[...]`
3. **Use list functions**: Don't reinvent sorting/filtering
4. **Default to empty carefully**: `[...T] | *[]` for optional lists
5. **Remember ellipsis position**: Always last
6. **Closed for fixed structures**: Like tuples or fixed schemas
7. **Open for collections**: When size varies
