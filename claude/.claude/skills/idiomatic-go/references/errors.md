# Error Handling

## Basic Pattern

Always check and wrap errors:
```go
result, err := doSomething()
if err != nil {
    return fmt.Errorf("failed to process user %d: %w", userID, err)
}
```

## Error Wrapping

Use `%w` to wrap errors for stack tracing:
```go
if err != nil {
    return fmt.Errorf("context about what failed: %w", err)
}
```

Check for specific errors:
```go
if errors.Is(err, sql.ErrNoRows) {
    // Handle not found
}
```

Check error types:
```go
var pathErr *fs.PathError
if errors.As(err, &pathErr) {
    fmt.Printf("Failed on path: %s\n", pathErr.Path)
}
```

## Sentinel Errors

Define package-level errors for known conditions:
```go
var (
    ErrNotFound      = errors.New("not found")
    ErrUnauthorized  = errors.New("unauthorized")
    ErrInvalidInput  = errors.New("invalid input")
)
```

Use `errors.Is` to check:
```go
if errors.Is(err, ErrNotFound) {
    // Handle not found case
}
```

## Custom Error Types

Create custom types for structured error data:
```go
type ValidationError struct {
    Field   string
    Value   any
    Issue   string
}

func (e *ValidationError) Error() string {
    return fmt.Sprintf("validation failed on %s: %s", e.Field, e.Issue)
}
```

Use `errors.AsType` (Go 1.26) for type-safe matching:
```go
if valErr, ok := errors.AsType[*ValidationError](err); ok {
    fmt.Printf("Invalid field: %s\n", valErr.Field)
}
```

## Error Best Practices

- **Always check errors** - Don't ignore them
- **Wrap with context** - Use `%w` to preserve original error
- **Don't log and return** - Either handle it or pass it up (not both)
- **Use sentinel errors** for known conditions
- **Use custom types** when you need structured data
- **Use `errors.AsType[T]`** instead of `errors.As` (Go 1.26)
