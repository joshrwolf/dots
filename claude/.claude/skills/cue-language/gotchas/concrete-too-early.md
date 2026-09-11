# Concrete Too Early

**Making values concrete prematurely is one of the most common CUE mistakes.**

## The Problem

Once a value becomes concrete, it can only unify with that exact value:

```cue
// Makes config concrete immediately
config: {
    items: []
    port: 8080
}

// Later, trying to add/change
config: {
    items: ["apple"]  // ✗ Conflict: [] vs ["apple"]
    port: 9000        // ✗ Conflict: 8080 vs 9000
}
```

## Why It Matters

**Concrete values kill composability:**

```cue
// base.cue - WRONG: concrete values
database: {
    host: "localhost"
    port: 5432
}

// prod.cue - Can't override
database: {
    host: "prod-db.example.com"  // ✗ Conflict
}
```

## The Solution

**Stay abstract with constraints and defaults:**

```cue
// base.cue - RIGHT: constraints with defaults
database: {
    host: string | *"localhost"
    port: int & >=1024 & <65536 | *5432
}

// prod.cue - Can override
database: {
    host: "prod-db.example.com"  // ✓ Unifies
}
```

## Common Scenarios

### Scenario 1: Empty Lists

```cue
// ❌ WRONG: Concrete empty list
config: {
    tags: []
}

// Can't add tags later
config: {
    tags: ["production"]  // ✗ Conflict
}
```

```cue
// ✅ RIGHT: Type constraint
config: {
    tags: [...string]  // or [...string] | *[]
}

// Can add tags
config: {
    tags: ["production"]  // ✓ Unifies
}
```

### Scenario 2: Empty Structs

```cue
// ❌ WRONG: Concrete empty struct
metadata: {}

// Can't add fields later
metadata: {
    version: "1.0"  // ✗ Conflict
}
```

```cue
// ✅ RIGHT: Open struct or definition
metadata: {...}  // Open struct with no fields yet

// or
#Metadata: {...}  // Definition that allows fields
metadata: #Metadata
```

### Scenario 3: Default Values

```cue
// ❌ WRONG: Concrete value
#Service: {
    replicas: 1
}

service: #Service & {
    replicas: 3  // ✗ Conflict
}
```

```cue
// ✅ RIGHT: Default disjunction
#Service: {
    replicas: int & >0 | *1
}

service: #Service & {
    replicas: 3  // ✓ Unifies
}
```

### Scenario 4: Nullable Fields

```cue
// ❌ WRONG: Concrete null
config: {
    timeout: null
}

// Can't set actual value
config: {
    timeout: 30  // ✗ Conflict
}
```

```cue
// ✅ RIGHT: Nullable with default
config: {
    timeout: int | null | *null
}

// Can override
config: {
    timeout: 30  // ✓ Unifies
}
```

## Detection Signs

You've made values concrete too early if:

- [ ] Getting "conflicting values" errors when layering configs
- [ ] Can't override defaults in derived schemas
- [ ] Empty collections (`[]`, `{}`) that need to grow
- [ ] Bare concrete values in base/shared configs
- [ ] No `type |` pattern in reusable schemas

## The Rule

**In schemas and base configs:**
- ❌ Don't: `port: 8080`
- ✅ Do: `port: int | *8080`

**In final configurations:**
- ✅ OK: `port: 8080` (final concrete value)

## Layers Pattern

```cue
// Layer 1: Pure constraints (most abstract)
#Config: {
    port: int & >=1024 & <65536
    host: string
}

// Layer 2: Constraints with defaults (still abstract)
#DefaultConfig: #Config & {
    port: int | *8080
    host: string | *"localhost"
}

// Layer 3: Environment specifics (some concrete)
#ProdConfig: #DefaultConfig & {
    host: string  // Don't make concrete yet
}

// Layer 4: Final instance (fully concrete)
prodDB: #ProdConfig & {
    host: "prod-db.example.com"
    port: 5432
}
```

## When Concrete is OK

**Final configurations** (leaf nodes in your config tree):

```cue
// This is the END of refinement - concrete is fine
deploymentConfig: {
    replicas: 3
    image: "myapp:v1.2.3"
    port: 8080
}
```

**Test data:**

```cue
testData: {
    input: [1, 2, 3]
    expected: 6
}
```

**Constants:**

```cue
// True constants (never meant to vary)
API_VERSION: "v1"
MAX_RETRIES: 3
```

## Progressive Refinement Example

```cue
// Too concrete
config: {
    database: {
        host: "localhost"
        port: 5432
        ssl: false
    }
}
```

**Refactored for composability:**

```cue
// Step 1: Abstract schema
#DatabaseConfig: {
    host!: string
    port!: int & >=1024 & <65536
    ssl!: bool
}

// Step 2: Add defaults
#DatabaseConfig: {
    host: string | *"localhost"
    port: int | *5432
    ssl: bool | *false
}

// Step 3: Environment-specific (still abstract)
#DevDatabase: #DatabaseConfig & {
    ssl: false  // Dev doesn't need SSL
}

#ProdDatabase: #DatabaseConfig & {
    ssl: true  // Prod requires SSL
}

// Step 4: Final concrete instances
devDB: #DevDatabase & {
    host: "localhost"
}

prodDB: #ProdDatabase & {
    host: "prod-db.example.com"
    port: 5432
}
```

## Empty Values Gotcha

```cue
// These are DIFFERENT

// 1. Concrete empty (closed)
a: []
a: [1]  // ✗ Conflict

// 2. Abstract constraint (can grow)
b: [...int]
b: [1]  // ✓ Unifies

// 3. Abstract with empty default
c: [...int] | *[]
c: [1]  // ✓ Unifies (overrides default)
```

## Null Pattern

```cue
// ❌ WRONG: Concrete null
field: null

// ✅ RIGHT: Nullable with default
field: T | null | *null

// ✅ RIGHT: Optional nullable
field?: T | null
```

## Quick Fix Checklist

For each concrete value in your schema, ask:

1. **Is this the final value?** If no → make it `type | *value`
2. **Could this vary by environment?** If yes → use constraint
3. **Is this in a reusable schema?** If yes → use constraint
4. **Do I need to override this later?** If yes → use constraint
5. **Is this an empty collection?** If yes → use `[...T]` or `{...}`

## Anti-Pattern Examples

```cue
// ❌ All of these are too concrete for schemas

#Config: {
    timeout: 30              // Should be: int | *30
    endpoints: []            // Should be: [...string]
    metadata: {}             // Should be: {...} or specific fields
    enabled: true            // Should be: bool | *true
    items: ["default"]       // Should be: [...string] | *["default"]
}
```

## Best Practice

**Stay abstract until the last possible moment:**

```cue
// 80% of your CUE should look like this
field: <type> | *<default>

// 20% should be final concrete values
actualValue: 42
```

## Key Insight

**Concrete is final. Abstract is composable.**

If you might need to change, constrain, or extend a value—keep it abstract.
