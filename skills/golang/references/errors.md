# Error API design

## Choosing a representation

| Caller needs | Use |
| --- | --- |
| To match a condition | Sentinel: `var ErrNotFound = errors.New("not found")` |
| Structured data | Error type, with `Unwrap` if it wraps a cause |
| Only a message | `errors.New` or `fmt.Errorf` |
| Several independent failures | `errors.Join` (nil when all inputs are nil) |

```go
type QueryError struct {
	Query string
	Err   error
}

func (e *QueryError) Error() string { return fmt.Sprintf("query %q: %v", e.Query, e.Err) }
func (e *QueryError) Unwrap() error { return e.Err }
```

Context reads as a chain: `creating order: charging card: connection refused`.
Do not repeat the callee's own context or name the function the stack already names.

## Wrapping at an API boundary

`%w` makes the cause part of your contract; callers may start depending on
`sql.ErrNoRows`. When they should depend on a domain condition instead,
translate:

```go
if errors.Is(err, sql.ErrNoRows) {
	return fmt.Errorf("item %q: %w", id, ErrNotFound)
}
```

`%v` blocks unwrapping but keeps the full text, including backend details. Choose
client-safe text separately for HTTP or user-facing responses.

## Reporting ownership

The layer that decides what happens next reports the failure: an HTTP handler,
`main`, a job runner. Lower layers return with context. Logging and returning
duplicates the report. Retry diagnostics and explicitly required audit events
are exceptions.

For log aggregation, keep the logged message stable and attach IDs, paths and
counts as structured attributes; a stable `fmt.Errorf` prefix still embeds the
cause's variable text. Returned errors may carry identifying context when it
helps the caller. `golang-observability` owns logging setup.

## Panic and recover

- Panic for violated invariants and documented panic APIs (`regexp.MustCompile`
  on a constant pattern). Expected failures return errors.
- Recover only at a boundary that can abandon the failed operation and report it.
  Recovery cannot repair shared state, and only works in a deferred call on the
  panicking goroutine; a parent cannot recover a child goroutine's panic.
- A recovery that converts a panic to an error needs a named result, and must
  still return that error, not success.
- `net/http` already recovers handler panics but not goroutines a handler starts.
  Custom middleware must re-panic `http.ErrAbortHandler`, and cannot rewrite a
  response that is already committed.
