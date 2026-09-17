# Type Assertions & Type Switches

## Safe Type Assertion

Use comma-ok when a mismatch should be handled. The single-value form panics on mismatch and is suitable only for an established invariant whose violation should panic:

```go
// Recoverable mismatch
s, ok := val.(string)
if !ok {
    // handle
}

// Requires an established string invariant; panics if it is violated
s := val.(string)
```

## Type Switch

Discover the dynamic type of an interface value:

```go
switch v := val.(type) {
case string:
    fmt.Println(v)
case int:
    fmt.Println(v * 2)
case io.Reader:
    io.Copy(os.Stdout, v)
default:
    fmt.Printf("unexpected type %T\n", v)
}
```

Cases are evaluated in order, so put concrete types before interface types — a concrete type listed after an interface it satisfies is unreachable.

A nil interface matches `case nil` when present, otherwise `default`. A typed nil pointer in an interface matches its pointer type (or a satisfied interface), not `case nil`.

## Optional Behavior with Type Assertions

Check if a value supports additional capabilities without requiring them upfront. This keeps the declared parameter type minimal while still exploiting richer implementations:

```go
type Flusher interface {
    Flush() error
}

func writeData(w io.Writer, data []byte) error {
    if _, err := w.Write(data); err != nil {
        return err
    }
    // Flush only if the writer supports it
    if f, ok := w.(Flusher); ok {
        return f.Flush()
    }
    return nil
}
```

This pattern is used extensively in the standard library (e.g., `http.Flusher`, `io.ReaderFrom`, `io.WriterTo`).

## Asserting to an Interface, not a Concrete Type

Assert to the smallest interface that carries the behavior you need rather than to a concrete type. Asserting to `*os.File` binds the code to one implementation; asserting to `interface{ Sync() error }` accepts every type that can do the job.

## Errors

Error inspection has dedicated helpers — `errors.As` walks the wrap chain, a plain type assertion does not. A bare `err.(*MyError)` misses an error that was wrapped with `%w` anywhere up the stack.

→ See `samber/cc-skills-golang@golang-error-handling` skill for error wrapping and inspection.
