---
name: cue-language
description: Idiomatic CUE reference for agents - covers syntax disambiguation, common gotchas, best practices, and patterns for writing maintainable constraint-based configurations
---

# CUE Language Reference

**Audience**: Agents with basic CUE understanding who need quick reference, idiom enforcement, and gotcha awareness.

## Core Principles (The Non-Negotiables)

1. **Unification is commutative**: Order doesn't matter. `a: 1` then `a: int` ≡ `a: int` then `a: 1`
2. **Default pattern is `type | *value`**: Never write bare `field: 8080`. Always `field: int | *8080`
3. **Definitions close structs**: `#Type` rejects unknown fields unless explicitly opened with `...`
4. **Bottom propagates**: One `_|_` poisons all computations downstream
5. **Stay abstract longest**: Concrete values limit composability

## Quick Syntax Reference

### Field Types (Critical Distinctions)

```cue
// Regular field - must be concrete for export
field: value

// Optional field constraint - only constrains if field appears
field?: type

// Required field constraint - must exist as regular field for export
field!: type

// Hidden field - not in output
_field: value

// Definition - closed struct, reusable schema
#Type: {...}

// Hidden definition - package-private
_#Type: {...}
```

**Key Point**: `field?: string` does NOT mean "optional with no constraints"—it means "if this field appears, it must be string."

### Defaults vs Constraints (Most Common Mistake)

```cue
// ❌ WRONG: Concrete value, can't be overridden
port: 8080

// ❌ WRONG: No default, always manual
port: int

// ✅ CORRECT: Idiomatic pattern
port: int | *8080

// ✅ CORRECT: Constraint with bound and default
port: int & >=1024 & <65536 | *8080
```

### Disjunctions

```cue
// Unordered disjunction (no default)
protocol: "tcp" | "udp" | "sctp"

// Default disjunction (marked with *)
env: "dev" | "staging" | "prod" | *"dev"

// Type union
value: int | string | null

// For export, exactly ONE element must match
x: 1 | 2  // ✗ Can't export (ambiguous)
```

### Lists

```cue
// Closed list (exact length)
items: [1, 2, 3]

// Open list (minimum length, type constraint)
items: [1, 2, 3, ...int]         // At least 3 ints
items: [...int]                  // Any number of ints
items: [string, string, ...int]  // 2 strings, then any ints

// Ellipsis MUST be last
items: [1, ..., 3]  // ✗ INVALID
```

### Structs (Open vs Closed)

```cue
// Open struct (default) - accepts any fields
config: {
    host: string
}

// Closed struct via definition - rejects unknown fields
#Config: {
    host: string
    port: int
}

// Explicitly open definition
#Config: {
    host: string
    ...  // Allow additional fields
}

// Pattern constraint (template) - applies to matching fields
services: [string]: {  // All string-named fields
    port: int | *8080
}
```

### Comprehensions

```cue
// List comprehension
result: [
    for x in list
    if condition
    let y = expr
    {y}
]

// Field comprehension
obj: {
    for k, v in source {
        "\(k)": transform(v)
    }
}

// Multiple clauses (any order)
result: [
    for x in list
    let squared = x * x
    if squared > 10
    for y in [1, 2]
    {squared * y}
]
```

## Critical Gotchas

### 1. Optional Field Bottom

```cue
#Schema: {
    a?: 0
    b?: 1
}

x: #Schema & {a?: 0, b?: 0}
// Result: a? unifies, b? = _|_
// b? with bottom means "field cannot exist"
```

**Rule**: Optional field with bottom disallows the field. Required field with bottom breaks the struct.

### 2. Concrete Too Early

```cue
// ❌ Makes items concrete immediately
config: {items: []}

// Later fails
config: {items: ["apple"]}  // ✗ Conflict

// ✅ Use type constraint
config: {items: [...string]}
```

### 3. Definition Closedness

```cue
#Person: {name: string}

person: #Person & {age: 30}  // ✗ age not allowed

// Fix: explicit open
#Person: {
    name: string
    ...
}
```

### 4. Fighting Unification

```cue
// ❌ Trying to "override"
config: {tier: "free"}
config: {tier: "paid"}  // ✗ Conflict

// ✅ Use default
config: {tier: string | *"free"}
config: {tier: "paid"}  // ✓ Unifies
```

### 5. Reference Scope Surprise

```cue
val: 1
A: {
    val: 2
    B: val    // 2 (inner scope)
}
A: {
    C: val    // 1 (outer scope - different struct)
}
```

**Rule**: References bind to nearest enclosing scope.

### 6. String Interpolation in Definitions

```cue
#Service: {
    name: string
    url: "http://\(name)"  // ✗ name not resolved yet
}

// Fix: use in instance
service: #Service & {
    name: "api"
    url: "http://\(name)"  // ✓ Works here
}
```

## Idiomatic Patterns

### Schema with Sensible Defaults

```cue
#Server: {
    host: string | *"localhost"
    port: int & >=1024 & <=65535 | *8080
    tls: {
        enabled: bool | *false
        if enabled {
            cert!: string
            key!:  string
        }
    }
}
```

### Progressive Refinement

```cue
// Start abstract
config: {...}

// Add structure
config: {
    database: {...}
    api: {...}
}

// Add constraints
config: database: {
    host: string
    port: int
}

// Add defaults
config: database: {
    host: *"localhost" | string
    port: *5432 | int
}
```

### Multi-Aspect Validation

```cue
// schema.cue - structure
name!: string
age!: int

// policy.cue - business rules
age?: >=18

// defaults.cue - sensible defaults
country: string | *"US"
```

### Conditional Fields

```cue
price: number

if price > 100 {
    reason!: string
    approvedBy!: string
}
```

### Template Pattern (Bulk Constraints)

```cue
services: {
    // Apply to all services
    [string]: {
        replicas: int | *1
        resources: {
            memory: string | *"512Mi"
            cpu: string | *"500m"
        }
    }
}

services: {
    nginx: {replicas: 3}
    api: {replicas: 2}
}
```

## Built-in Functions (Most Useful)

```cue
// Core
close(struct)     // Explicitly close struct
len(x)            // Length of string/list/struct
and([...bool])    // Logical and
or([...bool])     // Logical or

// Strings (import "strings")
strings.HasPrefix(string, prefix)
strings.HasSuffix(string, suffix)
strings.ToLower(string)
strings.Join([...string], sep)
strings.Split(string, sep)

// Lists (import "list")
list.Sort([...], cmp)
list.Concat([[...T]])
list.Avg([...number])
list.Max([...number])
list.Min([...number])

// Encoding (import "encoding/json" or "encoding/yaml")
json.Marshal(x)
json.Unmarshal(string)
yaml.Marshal(x)
yaml.Unmarshal(string)
```

## Tooling Quick Reference

```bash
# Evaluate (show all fields including non-concrete)
cue eval file.cue

# Evaluate with errors visible
cue eval -i file.cue

# Export (only concrete values, JSON by default)
cue export file.cue

# Validate data against schema
cue vet schema.cue data.json

# Format code
cue fmt file.cue

# Show definition
cue def file.cue

# Trim redundant fields
cue trim file.cue
```

## Error Message Patterns

**"conflicting values X and Y"**
→ Two incompatible concrete values unified
→ Check for accidental concrete values that should be defaults

**"field not allowed"**
→ Trying to add field to closed struct
→ Check if struct comes from definition without `...`

**"empty disjunction"**
→ All disjunction options resulted in _|_
→ Value doesn't match any option in enum/union

**"reference X not found"**
→ Scoping issue or typo
→ Check reference scope, consider using absolute path

**"cannot use X (type Y) as Z"**
→ Type mismatch
→ Check constraints and unification order

## File Organization Patterns

```cue
// schema.cue - structural definitions
package config

#Database: {
    host!: string
    port!: int & >=1024 & <65536
}

// defaults.cue - sensible defaults
package config

#Database: {
    host: *"localhost" | string
    port: *5432 | int
}

// policy.cue - business rules
package config

#Database: {
    host: !="prod-db" // Can't use prod in this env
}

// concrete.cue - actual values
package config

database: #Database & {
    host: "test-db"
}
```

## Navigation

### Deep Dives

- **Unification Mechanics** → [fundamentals/values-and-unification.md](fundamentals/values-and-unification.md)
- **Value Lattice Theory** → [fundamentals/value-lattice.md](fundamentals/value-lattice.md)
- **Bottom Propagation** → [fundamentals/bottom-and-errors.md](fundamentals/bottom-and-errors.md)

### Language Features

- **Modules, Packages, Instances** → [language-features/modules-packages-instances.md](language-features/modules-packages-instances.md) ⭐ **Critical for monorepos**
- **Definitions** → [language-features/definitions.md](language-features/definitions.md)
- **Structs** → [language-features/structs.md](language-features/structs.md)
- **Lists** → [language-features/lists.md](language-features/lists.md)
- **Disjunctions** → [language-features/disjunctions.md](language-features/disjunctions.md)
- **Comprehensions** → [language-features/comprehensions.md](language-features/comprehensions.md)
- **Bounds** → [language-features/bounds.md](language-features/bounds.md)
- **Strings** → [language-features/strings.md](language-features/strings.md)
- **References** → [language-features/references.md](language-features/references.md)

### Patterns

- **Defaults vs Constraints** → [patterns/defaults-vs-constraints.md](patterns/defaults-vs-constraints.md)
- **Schema Design** → [patterns/schema-design.md](patterns/schema-design.md)
- **Progressive Refinement** → [patterns/progressive-refinement.md](patterns/progressive-refinement.md)
- **Validation Strategies** → [patterns/validation.md](patterns/validation.md)

### Gotchas

- **Fighting Unification** → [gotchas/unification-fights.md](gotchas/unification-fights.md)
- **Concrete Too Early** → [gotchas/concrete-too-early.md](gotchas/concrete-too-early.md)
- **Open vs Closed** → [gotchas/open-vs-closed.md](gotchas/open-vs-closed.md)
- **Optional Field Gotchas** → [gotchas/optional-fields.md](gotchas/optional-fields.md)

### Built-ins & Tooling

- **Core Functions** → [builtins/core-functions.md](builtins/core-functions.md)
- **Standard Library** → [builtins/standard-library.md](builtins/standard-library.md)
- **CLI Tools** → [tooling/cli-tools.md](tooling/cli-tools.md)
- **Error Messages** → [tooling/error-messages.md](tooling/error-messages.md)

## Quick Checklist for Writing Idiomatic CUE

- [ ] Using `type | *default` pattern instead of bare values?
- [ ] Schemas are definitions (`#Type`) not regular fields?
- [ ] Understood which structs are closed?
- [ ] Avoided making values concrete too early?
- [ ] Using bounds with defaults (`int & >0 | *10`)?
- [ ] Optional fields (`?`) used correctly (not for defaults)?
- [ ] List ellipses (`...`) in correct position (last)?
- [ ] Not fighting unification (using defaults, not overrides)?

## When to Use This Skill

Activate when:
- Writing CUE schemas, definitions, or configurations
- Debugging unification errors or bottom (_|_)
- Enforcing idiomatic patterns (defaults, definitions, etc.)
- Understanding subtle syntax distinctions (optional vs required, open vs closed)
- Designing reusable, composable schemas
- Converting between CUE and other formats
- Validating data against constraints
