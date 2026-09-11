# Modules, Packages, and Instances

**Understanding CUE's three-tier organizational structure is critical for monorepo usage and hierarchical configuration.**

## The Three-Tier Hierarchy

```
Module                          (cue.mod directory)
└── Package                     (package name + import path)
    └── Instance                (package evaluated in directory context)
```

## Modules

**A module is the top-level organizational unit** containing everything needed to deterministically evaluate a CUE configuration.

### Creating a Module

```bash
# Initialize with default module path
cue mod init

# Initialize with specific module path
cue mod init example.com/myproject@v0
```

### Module Structure

```
myproject/
├── cue.mod/
│   ├── module.cue           # Module metadata, dependencies
│   ├── pkg/                 # [DEPRECATED] Manual external packages
│   ├── gen/                 # [DEPRECATED] Generated CUE from protobuf/Go
│   └── usr/                 # [DEPRECATED] User constraints for external pkgs
├── schemas/
│   └── config/
│       └── base.cue         # package config
├── environments/
│   ├── dev/
│   │   └── config.cue       # package config (inherits from schemas/)
│   └── prod/
│       └── config.cue       # package config (inherits from schemas/)
└── data.cue                 # package main
```

### Module Path

The module path format: `domain.com/optional/path@version`

- **Required**: Fully-qualified domain name
- **Optional**: Path components
- **Optional**: Major version (defaults to `@v0`)

```
example.com/transport@v0
k8s.io/api/core/v1
```

### Module Root Marker

**The `cue.mod` directory marks the module root**, analogous to `.git` for repos.

## Packages

**A package groups related CUE files** with the same package name.

### Package Declaration

```cue
package config  // At top of file

// Rest of CUE code
```

### Files Without Package Clause

Files without a `package` clause:
- Are standalone files
- Cannot be imported
- Only evaluated directly
- Useful for simple scripts or one-off configurations

### Package Identity

A package is identified by **import path + package name**:

```
k8s.io/api/core/v1:v1       // Full form
k8s.io/api/core/v1          // Short form (name matches basename)
```

### Multiple Packages Per Directory

**CUE allows multiple packages in one directory:**

```
mydir/
├── config.cue        # package config
├── schema.cue        # package config
└── test.cue          # package test
```

To specify which package to evaluate:

```bash
cue eval ./mydir              # Evaluates 'config' if unique
cue eval ./mydir:test         # Explicitly evaluates 'test'
```

### Package Concatenation

**All files in a package are concatenated:**

```cue
// policy.cue
package config

foo: bar/2 - 1
bar!: int
```

```cue
// data.cue
package config

bar: 200
```

**Result: Both files are merged** into single namespace:
```cue
foo: 99
bar: 200
```

Order doesn't matter—declarations unify.

## Instances

**An instance is a package evaluated in directory context.**

### Instance Loading Rules

When evaluating a package in a directory, CUE loads:
1. All files with that package name in **current directory**
2. All files with that package name in **ancestor directories** up to module root

**This is the key to hierarchical policy enforcement.**

### Hierarchy Pattern: Schema → Policy → Data

```
module root/              # SCHEMA: Organization-wide constraints
├── cue.mod/
├── base.cue             # package config: type definitions
│
├── policies/            # POLICY: Department constraints
│   ├── security.cue     # package config: security rules
│   │
│   └── services/        # POLICY: Service-specific rules
│       ├── api.cue      # package config: API rules
│       │
│       └── prod/        # DATA: Concrete instances
│           └── db.cue   # package config: inherits all above
```

When evaluating `prod/db.cue`, CUE loads:
1. `prod/db.cue` (leaf - concrete data)
2. `services/api.cue` (medial - policy)
3. `policies/security.cue` (medial - policy)
4. `base.cue` (root - schema)

**All files unify into single configuration.**

### Parent "Push Out" Pattern

Parents define constraints that children must satisfy:

```cue
// root/schema.cue
package config

#Service: {
    name!: string
    replicas: int & >0 | *1
}
```

```cue
// root/policies/prod.cue
package config

#Service: {
    replicas: >=3  // Prod requires 3+ replicas
}
```

```cue
// root/services/api/prod/service.cue
package config

service: #Service & {
    name: "api"
    replicas: 5  // Must satisfy >=3 from policy
}
```

**Children don't "inherit"—they satisfy constraints pushed from parents.**

## Imports

### Import Syntax

```cue
package mypackage

import (
    "strings"                                      // Builtin
    "list"                                         // Builtin
    "example.com/mymodule/schemas/config"         // Module package
    namedImport "example.com/other/package"       // Named import
)
```

### Builtin Packages

**Builtins don't require a module:**
- Compiled into `cue` command
- No disk storage needed
- Import path doesn't start with domain name

Examples: `strings`, `list`, `encoding/json`, `encoding/yaml`

### Non-Builtin Package Resolution

**For local packages within your module:**

If module path is prefix of import path, CUE resolves relative to module root:

```cue
// Module: example.com/transport
// Import: example.com/transport/schemas/trains:track

// Resolves to: <module_root>/schemas/trains/
```

**Note**: CUE also supports remote module dependencies (fetched from registries) and deprecated `cue.mod/pkg`/`gen`/`usr` directories, but these are beyond the scope of basic monorepo usage.

### Import Path Construction

Within module `example.com/transport`:

```
root/
├── cue.mod/
│   └── module.cue        # module: "example.com/transport"
└── schemas/
    └── trains/
        └── track.cue     # package track
```

Import as:

```cue
import "example.com/transport/schemas/trains:track"
// or short form if package name matches dir:
import "example.com/transport/schemas/trains"
```

## Monorepo Patterns

### Pattern 1: Shared Schemas at Root

```
monorepo/
├── cue.mod/
├── schemas/
│   └── base.cue          # package shared: #Config, #Service
├── team-a/
│   └── service.cue       # package teama: imports shared
└── team-b/
    └── service.cue       # package teamb: imports shared
```

Each team imports root schemas but has separate packages.

### Pattern 2: Hierarchical Policy Enforcement

```
monorepo/
├── cue.mod/
├── global-policy.cue     # package app: global constraints
├── dev/
│   ├── env-policy.cue    # package app: dev-specific policies
│   └── services/
│       └── api.cue       # package app: inherits global + dev
└── prod/
    ├── env-policy.cue    # package app: prod-specific policies
    └── services/
        └── api.cue       # package app: inherits global + prod
```

**Same package name throughout** means instances automatically inherit ancestor constraints.

### Pattern 3: Multi-Package Monorepo

```
monorepo/
├── cue.mod/
│   └── module.cue        # module: "company.com/platform"
├── shared/
│   └── types.cue         # package types
├── services/
│   ├── api/
│   │   └── config.cue    # package api
│   └── worker/
│       └── config.cue    # package worker
└── infra/
    └── k8s.cue          # package infra
```

Each service is its own package, all import from shared.

## Evaluation Commands

```bash
# Evaluate package in current directory
cue eval

# Evaluate specific directory's package
cue eval ./path/to/dir

# Evaluate specific package when multiple exist
cue eval ./path/to/dir:packagename

# Export (requires concrete values)
cue export ./path/to/dir

# Evaluate multiple packages
cue eval ./pkg1 ./pkg2

# Evaluate recursively
cue eval ./...
```

## Key Insights

### 1. Instance = Package + Directory Context

The same package evaluated in different directories produces different instances:

```
pkg/base.cue          # package config: base constraints
dev/config.cue        # package config: gets base + local
prod/config.cue       # package config: gets base + local
```

### 2. No Explicit Inheritance

Children don't declare what they inherit. Parents "push" constraints down:

```cue
// Parent doesn't know about children
// Children don't import parent
// CUE automatically merges based on package name + directory
```

### 3. Package vs Module Imports

```cue
// Within same module - relative to module root
import "mymodule.com/pkg/subpkg"

// External module - from dependency
import "github.com/someone/module/pkg"

// Builtin - no module needed
import "strings"
```

### 4. Order Independence Enables Hierarchy

Because unification is commutative, it doesn't matter whether:
- Child declarations come before/after parent
- Constraints are in same file or different files
- Files are processed in any particular order

## Common Patterns

### Shared Schema, Per-Environment Data

```cue
// schemas/service.cue
package config

#Service: {
    name!: string
    replicas: int | *1
}
```

```cue
// dev/services.cue
package config

service: #Service & {
    name: "api"
    // replicas: 1 (default)
}
```

```cue
// prod/services.cue
package config

service: #Service & {
    name: "api"
    replicas: 10
}
```

### Policy Layering

```cue
// global.cue
package app

#Config: {
    timeout: int & >0 & <300
}
```

```cue
// prod/policy.cue
package app

#Config: {
    timeout: >=30  // Prod requires longer timeout
}
```

### Team Boundaries with Packages

```
monorepo/
├── cue.mod/
├── platform/
│   └── api.cue           # package platform
├── team-red/
│   └── service.cue       # package teamred
└── team-blue/
    └── service.cue       # package teamblue
```

Different packages = isolated namespaces.

## Debugging Tips

### Check What Gets Loaded

```bash
# See which files CUE loads
cue eval -v ./path

# Export with trace
cue eval -t ./path
```

### Verify Package Name

```bash
# List packages in directory
cue eval ./dir
# Error: "multiple packages"

# Specify package
cue eval ./dir:config
```

### Check Module Root

```bash
# From any subdir, find module root
find . -name "cue.mod" -type d
```

## Quick Reference

| Concept | Definition | Marker |
|---------|------------|--------|
| **Module** | Top-level unit with all dependencies | `cue.mod/` directory |
| **Package** | Grouped files with same package name | `package <name>` clause |
| **Instance** | Package evaluated in directory context | Package + directory path |
| **Import Path** | Unique package identifier | `domain.com/path:name` |
| **Builtin** | Standard library package | No domain in import |
| **Module Path** | Module identifier | In `module.cue` |

## Best Practices

1. **Use module for any non-trivial project**
2. **Same package name for hierarchical policy** (root → medial → leaf)
3. **Different packages for team boundaries** (isolation)
4. **Root = schema, medial = policy, leaf = data**
5. **Avoid deprecated `pkg`/`gen`/`usr` directories**
6. **Use builtin packages freely** (no module needed)
7. **Module path should reflect ownership** (domain you control)
