# Comprehensions

**Comprehensions generate lists and fields programmatically using `for`, `if`, and `let` clauses.**

## List Comprehensions

Generate lists from expressions:

```cue
// Basic list comprehension
squares: [
    for x in [1, 2, 3, 4, 5]
    {x * x}
]
// Result: [1, 4, 9, 16, 25]
```

### With Filters

```cue
evens: [
    for x in [1, 2, 3, 4, 5, 6]
    if x % 2 == 0
    {x}
]
// Result: [2, 4, 6]
```

### With Let Bindings

```cue
transformed: [
    for x in [1, 2, 3]
    let squared = x * x
    let doubled = squared * 2
    {doubled}
]
// Result: [2, 8, 18]
```

### Multiple Loops (Cartesian Product)

```cue
pairs: [
    for x in [1, 2]
    for y in ["a", "b"]
    {"\(x)\(y)"}
]
// Result: ["1a", "1b", "2a", "2b"]
```

### Combining Clauses

**Clauses can be in any order:**

```cue
result: [
    for x in [1, 2, 3, 4, 5]
    let squared = x * x
    if squared > 10
    for y in [1, 2]
    {squared * y}
]
// Filters first, then multiplies by each y
```

## Field Comprehensions

Generate struct fields dynamically:

```cue
import "strings"

#censusData: [
    {name: "Kinshasa", pop: 16_315_534},
    {name: "Lagos", pop: 15_300_000},
    {name: "Cairo", pop: 10_100_166},
]

city: {
    for index, value in #censusData
    let lower = strings.ToLower(value.name) {
        "\(lower)": {
            population: value.pop
            name: value.name
            position: index + 1
        }
    }
}

// Result:
// city: {
//     kinshasa: {population: 16315534, name: "Kinshasa", position: 1}
//     lagos: {population: 15300000, name: "Lagos", position: 2}
//     cairo: {population: 10100166, name: "Cairo", position: 3}
// }
```

### Dynamic Field Names

```cue
ports: {
    for service, port in {api: 8080, web: 3000, db: 5432} {
        "\(service)_port": port
        "\(service)_url": "http://localhost:\(port)"
    }
}

// Result:
// ports: {
//     api_port: 8080
//     api_url: "http://localhost:8080"
//     web_port: 3000
//     web_url: "http://localhost:3000"
//     ...
// }
```

## Comprehension Clauses

### for Clause

```cue
// Iterate over list (index optional)
for item in list {...}
for index, item in list {...}

// Iterate over struct (key required)
for key, value in struct {...}
```

### if Clause

```cue
// Filter based on condition
for x in list
if x > 10
{...}
```

### let Clause

```cue
// Bind intermediate values
for x in list
let squared = x * x
{squared}
```

## Conditional Fields

Field comprehensions enable conditional field inclusion:

```cue
price: number

// Add fields only if condition true
if price > 100 {
    discount: number
    reason!: string
}
```

## Nested Comprehensions

```cue
matrix: [
    for row in [1, 2, 3]
    [
        for col in [1, 2, 3]
        {row * col}
    ]
]
// Result: [[1,2,3], [2,4,6], [3,6,9]]
```

## List Builtins with Comprehensions

Comprehensions work well with `list` package functions:

```cue
import "list"

numbers: [3, 1, 4, 1, 5, 9, 2, 6]

// Sum of filtered values
sumEven: list.Sum([
    for x in numbers
    if x % 2 == 0
    {x}
])
// Result: 12

// Max of transformed values
maxSquared: list.Max([
    for x in numbers
    {x * x}
])
// Result: 81

// Sort comprehension result
sortedFiltered: list.Sort([
    for x in numbers
    if x > 2
    {x}
], list.Ascending)
// Result: [3, 4, 5, 6, 9]

// Concatenate nested lists
flattened: list.Concat([
    for x in [1, 2, 3]
    [[x, x * 2]]
])
// Result: [1, 2, 2, 4, 3, 6]
```

## Common Patterns

### Transform List

```cue
input: [{name: "alice", age: 30}, {name: "bob", age: 25}]

names: [
    for person in input
    {person.name}
]
// Result: ["alice", "bob"]
```

### Filter and Transform

```cue
numbers: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

largeSquares: [
    for x in numbers
    let squared = x * x
    if squared > 50
    {squared}
]
// Result: [64, 81, 100]
```

### Build Lookup Map

```cue
users: [
    {id: 1, name: "alice"},
    {id: 2, name: "bob"},
]

userMap: {
    for user in users {
        "\(user.id)": user
    }
}

// Result:
// userMap: {
//     "1": {id: 1, name: "alice"}
//     "2": {id: 2, name: "bob"}
// }
```

### Generate Configuration

```cue
services: ["api", "web", "worker"]

deployments: {
    for service in services {
        "\(service)": {
            name: service
            replicas: int | *1
            image: "myapp/\(service):latest"
        }
    }
}
```

## Accessing Comprehension Results

**Dynamic fields cannot be referenced directly:**

```cue
generated: {
    for name in ["a", "b"] {
        "\(name)": 1
    }
}

// ✗ WRONG: Can't reference directly
x: generated.a  // Error: field not found

// ✓ RIGHT: Use selector or index
x: generated["a"]

// ✓ RIGHT: Use alias
generated: {
    for name in ["a", "b"]
    let alias = name {
        "\(name)": 1
    }
}
```

## Multiple Iterations

```cue
// Process in stages
stage1: [
    for x in [1, 2, 3]
    {x * 2}
]
// [2, 4, 6]

stage2: [
    for x in stage1
    if x > 3
    {x * x}
]
// [16, 36]
```

## Gotchas

### 1. Curly Braces Required

```cue
// ✗ WRONG: Missing braces around result
result: [
    for x in [1, 2, 3]
    x * x  // Invalid syntax
]

// ✓ CORRECT
result: [
    for x in [1, 2, 3]
    {x * x}
]
```

### 2. Let Scope

```cue
// let bindings only visible in comprehension
for x in [1, 2, 3]
let squared = x * x
{squared}

// ✗ squared not accessible here
y: squared  // Error: reference not found
```

### 3. Order of Clauses

Clauses can be in any order, but semantics matter:

```cue
// Filter then square
a: [
    for x in [1, 2, 3, 4, 5]
    if x > 3
    let squared = x * x
    {squared}
]
// [16, 25]

// Square then filter (different result)
b: [
    for x in [1, 2, 3, 4, 5]
    let squared = x * x
    if squared > 10
    {squared}
]
// [16, 25]
```

### 4. Dynamic Field References

```cue
services: {
    for name in ["api", "web"] {
        "\(name)": {port: 8080}
    }
}

// ✗ Can't reference: services.api
// ✓ Must use: services["api"]
```

## Advanced Patterns

### Accumulation with Filtering

```cue
import "list"

data: [
    {type: "a", value: 10},
    {type: "b", value: 20},
    {type: "a", value: 30},
]

totalA: list.Sum([
    for item in data
    if item.type == "a"
    {item.value}
])
// Result: 40
```

### Nested Field Generation

```cue
envs: ["dev", "staging", "prod"]

configs: {
    for env in envs {
        "\(env)": {
            database: {
                host: "\(env)-db.example.com"
                if env == "prod" {
                    replicas: 3
                }
                if env != "prod" {
                    replicas: 1
                }
            }
        }
    }
}
```

### Cross-Product with Filtering

```cue
regions: ["us", "eu", "asia"]
tiers: ["free", "paid"]

services: {
    for region in regions
    for tier in tiers
    // Skip free tier in Asia
    if !(region == "asia" && tier == "free") {
        "\(region)-\(tier)": {
            region: region
            tier: tier
        }
    }
}
```

## Quick Reference

| Clause | Syntax | Purpose |
|--------|--------|---------|
| `for` | `for item in list` | Iterate list |
| `for` | `for index, item in list` | Iterate with index |
| `for` | `for key, value in struct` | Iterate struct |
| `if` | `if condition` | Filter |
| `let` | `let name = expr` | Bind intermediate value |

## Best Practices

1. **Use `let` for complex expressions**: Improves readability
2. **Filter early**: Put `if` before expensive operations
3. **Name loop variables clearly**: `for service in services` not `for x in y`
4. **Access dynamic fields with selectors**: `obj["key"]` not `obj.key`
5. **Use `for index, item` when index needed**: Don't manually track
6. **Combine with list functions**: Use `list.Sum()`, `list.Sort()` etc.

## Comprehensions vs Pattern Constraints

**Different purposes:**

```cue
// Pattern constraint - applies to existing fields
services: {
    [string]: {  // Template for all string fields
        port: int | *8080
    }
}

// Field comprehension - generates new fields
services: {
    for name in ["api", "web"] {  // Creates specific fields
        "\(name)": {port: 8080}
    }
}
```

Pattern constraints template existing fields; comprehensions generate new ones.
