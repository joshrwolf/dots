# Error Wrapping and Inspection

## Table of Contents

- [Error Wrapping with `%w`](#error-wrapping-with-w)
  - [`%w` vs `%v`: controlling exposure](#w-vs-v-controlling-exposure)
- [Inspecting Errors: `errors.Is` and `errors.As`](#inspecting-errors-errorsis-and-errorsas)
  - [`errors.Is` — match against a sentinel value](#errorsis--match-against-a-sentinel-value)
  - [`errors.As / errors.AsType` — extract a typed error from the chain](#errorsas--errorsastype--extract-a-typed-error-from-the-chain)
- [Combining Errors with `errors.Join`](#combining-errors-with-errorsjoin)
  - [Use case: validating multiple fields](#use-case-validating-multiple-fields)
  - [Use case: parallel operations with independent failures](#use-case-parallel-operations-with-independent-failures)
  - [`errors.Is` works through joined errors](#errorsis-works-through-joined-errors)

## Error Wrapping with `%w`

Wrapping preserves the original error in a chain that callers can inspect with `errors.Is` and `errors.As`. Add context when it distinguishes the operation; forwarding an already informative error unchanged is valid.

```go
// ✓ Good — wraps with context, preserves the chain
func (s *UserService) GetUser(id string) (*User, error) {
    user, err := s.repo.FindByID(id)
    if err != nil {
        return nil, fmt.Errorf("getting user %s: %w", id, err)
    }
    return user, nil
}
```

### `%w` vs `%v`: controlling exposure

Use `%w` when the cause is part of the caller-visible contract, including public APIs. Translate implementation-specific failures to domain errors when callers should not depend on them. `%v` prevents unwrapping but retains the complete error text; it is not redaction.

```go
// The database cause is exposed to callers of this internal method
func (r *repo) fetch(id string) error {
    return fmt.Errorf("querying database: %w", err)
}

// This API intentionally exposes text but not the backend error type
func (s *PublicService) GetItem(id string) error {
    err := s.repo.fetch(id)
    if err != nil {
        return fmt.Errorf("item unavailable: %v", err) // text still contains backend details
    }
    return nil
}
```

When clients need a stable domain condition, return or wrap that sentinel instead:

```go
var ErrNotFound = errors.New("item not found")

func lookupError(id string, err error) error {
    if errors.Is(err, sql.ErrNoRows) {
        return fmt.Errorf("item %q: %w", id, ErrNotFound)
    }
    return err
}
```

Here callers can match `ErrNotFound` without depending on `sql.ErrNoRows`.
The fallback still exposes other errors intentionally; decide that policy for the
actual API. HTTP responses need a separate choice of client-safe text.
See [Go's error API guidance](https://go.dev/blog/go1.13-errors#whether-to-wrap).

## Inspecting Errors: `errors.Is` and `errors.As`

### `errors.Is` — match against a sentinel value

```go
// ✗ Bad — direct comparison breaks on wrapped errors
if err == sql.ErrNoRows {

// ✓ Good — traverses the entire error chain
if errors.Is(err, sql.ErrNoRows) {
    return nil, ErrNotFound
}
```

### `errors.As / errors.AsType` — extract a typed error from the chain

```go
// ✗ Bad — type assertion breaks on wrapped errors
if ve, ok := err.(*ValidationError); ok {

// ✓ Good — traverses the entire error chain
var ve *ValidationError
if errors.As(err, &ve) {
    log.Printf("validation failed on field %s: %s", ve.Field, ve.Msg)
}

// ✓ Better (Go 1.26+) — same behavior, simpler syntax
if ve, ok := errors.AsType[*ValidationError](err); ok {
    log.Printf("validation failed on field %s: %s", ve.Field, ve.Msg)
}
```

## Combining Errors with `errors.Join`

`errors.Join` (Go 1.20+) combines multiple independent errors into one. The combined error works with `errors.Is` and `errors.As` — each inner error is inspectable.

### Use case: validating multiple fields

```go
func validateUser(u User) error {
    var errs []error

    if u.Name == "" {
        errs = append(errs, errors.New("name is required"))
    }
    if u.Email == "" {
        errs = append(errs, errors.New("email is required"))
    }

    return errors.Join(errs...) // returns nil if errs is empty
}
```

### Use case: parallel operations with independent failures

```go
func closeAll(closers ...io.Closer) error {
    var errs []error
    for _, c := range closers {
        if err := c.Close(); err != nil {
            errs = append(errs, err)
        }
    }
    return errors.Join(errs...)
}
```

### `errors.Is` works through joined errors

```go
err := errors.Join(ErrNotFound, ErrUnauthorized)

errors.Is(err, ErrNotFound)    // true
errors.Is(err, ErrUnauthorized) // true
```
