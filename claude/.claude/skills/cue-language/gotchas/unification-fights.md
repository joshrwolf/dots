# Fighting Unification

**The most common mistake when coming from other languages.**

## The Problem

In most languages, you "override" or "reassign" values. CUE doesn't work this way—it **unifies** constraints. Fighting this leads to `_|_`.

## The Symptom

```cue
// Trying to "override"
config: {tier: "free"}
config: {tier: "paid"}

// Result: _|_ conflicting values "paid" and "free"
```

## Why This Happens

In CUE, both declarations must be simultaneously true:
- `tier` must be `"free"` (first declaration)
- `tier` must be `"paid"` (second declaration)

These constraints conflict → bottom (`_|_`).

## The Solution

**Don't fight unification—work with it using defaults:**

```cue
// Base configuration
config: {tier: string | *"free"}

// Override
config: {tier: "paid"}

// Result: tier: "paid" ✓
```

The first declaration says "tier is a string, defaults to 'free'". The second says "tier is 'paid'". These unify: `string & "paid"` → `"paid"`.

## Common Scenarios

### Scenario 1: Configuration Layers

```cue
// ❌ WRONG: Concrete values
// base.cue
database: {
    host: "localhost"
    port: 5432
}

// prod.cue
database: {
    host: "prod-db.example.com"  // ✗ Conflict with "localhost"
}
```

```cue
// ✅ RIGHT: Defaults
// base.cue
database: {
    host: string | *"localhost"
    port: int | *5432
}

// prod.cue
database: {
    host: "prod-db.example.com"  // ✓ Unifies
}
```

### Scenario 2: Schema Inheritance

```cue
// ❌ WRONG: Can't "override" in schemas
#Base: {
    timeout: 30
}

#Derived: #Base & {
    timeout: 60  // ✗ Conflict
}
```

```cue
// ✅ RIGHT: Use constraints with defaults
#Base: {
    timeout: int & >0 | *30
}

#Derived: #Base & {
    timeout: 60  // ✓ Unifies with constraint
}
```

### Scenario 3: Environment-Specific Values

```cue
// ❌ WRONG: Concrete per environment
// common.cue
replicas: 3

// prod.cue
replicas: 10  // ✗ Conflict
```

```cue
// ✅ RIGHT: Use disjunction or constraint
// common.cue
replicas: int & >0 | *3

// prod.cue
replicas: 10  // ✓ Unifies
```

## Distinguishing Override vs Refinement

### Override (Doesn't Exist in CUE)

In other languages:
```
x = 1      // x is 1
x = 2      // x is now 2 (replaced)
```

### Refinement (How CUE Works)

In CUE:
```cue
x: int     // x is any integer
x: >0      // x is positive integer (more specific)
x: 42      // x is 42 (most specific)
```

Each declaration **narrows** the possibilities. You can't go backward (make it less specific).

## When You Actually Want Multiple Values

Use a list:
```cue
// ❌ WRONG: Trying to have multiple values
value: 1
value: 2

// ✅ RIGHT: Use a list
values: [1, 2]
```

Or a struct with different fields:
```cue
// ❌ WRONG
config: {host: "a"}
config: {host: "b"}

// ✅ RIGHT
config: {
    primary: "a"
    secondary: "b"
}
```

## Disjunction is NOT Override

```cue
// This is a constraint (value must be one of these)
protocol: "tcp" | "udp" | "sctp"

// This unifies by picking the matching option
protocol: "tcp"  // Result: "tcp"

// This is NOT "overriding tcp with udp"
// It's "value must be tcp AND udp" → _|_
protocol: "tcp"
protocol: "udp"  // ✗ Conflict
```

## The Mental Shift

### Old Way (Imperative)
```
1. Set x to 10
2. Later, change x to 20
3. x is now 20
```

### CUE Way (Declarative)
```
1. Declare: x must be an integer
2. Declare: x must be greater than 5
3. Declare: x must be less than 100
4. Declare: x is 42
5. All constraints satisfied: x is 42
```

## Advanced: Conditional "Overrides"

If you really need different values in different contexts, use conditionals:

```cue
env: "dev" | "prod"

database: {
    if env == "dev" {
        host: "localhost"
    }
    if env == "prod" {
        host: "prod-db.example.com"
    }
}
```

But this is still refinement—the field only exists when the condition is true.

## Detection Checklist

You're fighting unification if:
- [ ] You're trying to "change" a value you already declared
- [ ] You're getting "conflicting values" errors
- [ ] You have concrete values in multiple files/layers
- [ ] You're thinking "why can't I just override this?"

## Fix Checklist

To stop fighting:
- [ ] Use `type | *default` pattern instead of concrete values
- [ ] Think "constraints" not "assignments"
- [ ] Keep base layers abstract, final layers concrete
- [ ] Use conditionals for truly different values
- [ ] Remember: order doesn't matter (proves it's not override)

## Real-World Pattern

```cue
// 1. Abstract schema (constraints only)
#Config: {
    database: {
        host: string
        port: int & >=1024 & <65536
        ssl: bool
    }
}

// 2. Defaults layer (still abstract)
#ConfigWithDefaults: #Config & {
    database: {
        host: string | *"localhost"
        port: int | *5432
        ssl: bool | *false
    }
}

// 3. Environment-specific refinement
#ProdConfig: #ConfigWithDefaults & {
    database: {
        ssl: true  // Override default
    }
}

// 4. Final concrete values
prodDB: #ProdConfig & {
    database: {
        host: "prod-db.example.com"
        port: 5432
    }
}
```

Each layer refines, never overrides.

## Key Insight

**CUE doesn't have assignment. It has progressive constraint satisfaction.**

If you're thinking "override", you're thinking wrong. Think "refine" or "narrow."
