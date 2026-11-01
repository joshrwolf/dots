# Advanced Usage

## Table of Contents

- [Scopes (Module Tree)](#scopes-module-tree)
- [Explicit Service Aliasing](#explicit-service-aliasing)
- [Struct Injection](#struct-injection)
- [Lifecycle Management](#lifecycle-management)
  - [Health Checks](#health-checks)
  - [Graceful Shutdown](#graceful-shutdown)
- [Debugging](#debugging)
  - [List Services](#list-services)
  - [Explain Injector](#explain-injector)
- [Migration from Manual DI](#migration-from-manual-di)
- [Quick Reference](#quick-reference)
  - [Aliasing](#aliasing)
  - [Lifecycle & Health](#lifecycle--health)
  - [Container Management](#container-management)
  - [Debugging](#debugging-1)

## Scopes (Module Tree)

Scopes SHOULD be used to organize services by module:

```go
root := do.New()

// Register shared services in root
do.Provide(root, func(i do.Injector) (*Database, error) {
    return &Database{}, nil
})

// Create child scope
apiScope := root.Scope("api")

// Services in apiScope can access root services
do.Provide(apiScope, func(i do.Injector) (*userService, error) {
    db := do.MustInvoke[*Database](i) // from root
    return &userService{db: db}, nil
})

// Child scopes are isolated from each other
userScope := root.Scope("user")
```

Organize services by lifecycle and visibility:

```go
root := do.New()

// Global/stateless services in root
do.Provide(root, NewConfig)
do.Provide(root, NewLogger)

// Request-scoped services
requestScope := root.Scope("request")
do.Provide(requestScope, NewRequestContext)
```

## Explicit Service Aliasing

Use an explicit alias when a particular implementation must supply an interface. `InvokeAs` selects the first matching service and is ambiguous when several implementations match:

```go
do.Provide(injector, func(i do.Injector) (*PostgreSQLDatabase, error) {
    return &PostgreSQLDatabase{}, nil
})

do.MustAs[*PostgreSQLDatabase, Database](injector)

// Now both work:
db1 := do.MustInvoke[*PostgreSQLDatabase](injector)
db2 := do.MustInvoke[Database](injector)
```

Use `InvokeAs()` when the first matching implementation is the intended one; aliases or named services make selection explicit.

## Struct Injection

Inject services directly into struct fields using tags:

```go
type App struct {
    Database *Database `do:""`
    Logger   *Logger   `do:"app-logger"`
    Config   *Config   `do:""`
}

app := do.MustInvokeStruct[App](injector)
```

## Lifecycle Management

### Health Checks

Implement `Healthchecker` or `HealthcheckerWithContext` (choose one signature for a type):

```go
func (d *Database) HealthCheck() error {
    return d.conn.Ping()
}

// With context support:
func (d *Database) HealthCheck(ctx context.Context) error {
    return d.conn.PingContext(ctx)
}

// Check health
if err := do.HealthCheck[*Database](injector); err != nil {
    log.Printf("Database unhealthy: %v", err)
}
```

### Graceful Shutdown

Implement one of the four shutdown interfaces (these are alternative signatures, not methods to define together):

```go
// Simple
func (d *Database) Shutdown() { d.conn.Close() }

// With context
func (d *Database) Shutdown(ctx context.Context) { d.conn.Close() }

// With error
func (d *Database) Shutdown() error { return d.conn.Close() }

// With context + error (most flexible)
func (d *Database) Shutdown(ctx context.Context) error { return d.conn.Close() }
```

Shutdown with timeout:

```go
ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
defer cancel()

report := injector.ShutdownWithContext(ctx)
```

## Debugging

### List Services

```go
services := injector.ListProvidedServices()
for _, svc := range services {
    fmt.Printf("%s: %s\n", svc.ScopeName, svc.Service)
}
```

### Explain Injector

```go
explanation := do.ExplainInjector(injector)
fmt.Println(explanation.String())
```

## Migration from Manual DI

Before (manual):

```go
func main() {
    config := &Config{Port: 8080}
    db := NewDatabase(config)
    userRepo := NewUserRepository(db)
    userService := NewUserService(userRepo)
    api := NewAPI(userService)
}
```

After (with do):

```go
func main() {
    injector := do.New()
    do.Provide(injector, func(i do.Injector) (*Config, error) {
        return &Config{Port: 8080}, nil
    })
    do.Provide(injector, func(i do.Injector) (*Database, error) {
        return NewDatabase(do.MustInvoke[*Config](i)), nil
    })
    // Adapt and register the remaining constructors before invoking API.
    api := do.MustInvoke[*API](injector)
}
```

## Quick Reference

### Aliasing

| Function                       | Purpose                 |
| ------------------------------ | ----------------------- |
| `do.As[Initial, Alias]()`      | Create type alias       |
| `do.AsNamed[Initial, Alias]()` | Create named type alias |

### Lifecycle & Health

| Function                         | Purpose                     |
| -------------------------------- | --------------------------- |
| `do.HealthCheck[T]()`            | Check service health        |
| `do.HealthCheckNamed()`          | Check named service health  |
| `do.HealthCheckWithContext[T]()` | Health check with timeout   |
| `do.Shutdown[T]()`               | Gracefully shutdown service |
| `do.ShutdownNamed()`             | Shutdown named service      |
| `do.ShutdownWithContext[T]()`    | Shutdown with timeout       |
| `do.MustShutdown[T]()`           | Shutdown (panic on error)   |

### Container Management

| Function           | Purpose                       |
| ------------------ | ----------------------------- |
| `do.New()`         | Create new root container     |
| `do.NewWithOpts()` | Create container with options |
| `injector.Scope()` | Create child scope            |

### Debugging

| Function                          | Purpose                              |
| --------------------------------- | ------------------------------------ |
| `do.ExplainInjector()`            | Visualize scope tree and services    |
| `do.ExplainService[T]()`          | Get service details and dependencies |
| `do.NameOf[T]()`                  | Get service name (use sparingly)     |
| `injector.ListProvidedServices()` | List all available services          |
| `injector.ListInvokedServices()`  | List invoked services only           |
