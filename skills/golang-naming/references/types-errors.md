# Types, Constants & Errors

## Table of Contents

- [Interfaces](#interfaces)
  - [Single-Method Interfaces](#single-method-interfaces)
  - [Multi-Method Interfaces](#multi-method-interfaces)
  - [Canonical Method Names](#canonical-method-names)
- [Structs](#structs)
- [Constants](#constants)
  - [Enums (iota)](#enums-iota)
- [Errors](#errors)
  - [Sentinel Errors](#sentinel-errors)
  - [Error Types](#error-types)
  - [Error Strings](#error-strings)

## Interfaces

### Single-Method Interfaces

Name them with the **method name + `-er`** suffix:

```go
type Reader interface {
    Read(p []byte) (n int, err error)
}

type Stringer interface {
    String() string
}

type Closer interface {
    Close() error
}
```

### Multi-Method Interfaces

Use a descriptive **noun** or compose from single-method interfaces:

```go
type ReadWriteCloser interface {
    Reader
    Writer
    Closer
}

type Handler interface {
    ServeHTTP(ResponseWriter, *Request)
}
```

### Canonical Method Names

Honor established names and signatures when implementing standard interfaces: byte-stream Read should match io.Reader, and a textual representation commonly uses String() string. A domain-specific Read or ReadData with different semantics is legal; the name alone does not imply interface implementation.

| Method name | Expected interface |
| ----------- | ------------------ |
| `Read`      | `io.Reader`        |
| `Write`     | `io.Writer`        |
| `Close`     | `io.Closer`        |
| `String`    | `fmt.Stringer`     |
| `Error`     | `error`            |
| `Len`       | `sort.Interface` (also needs Less and Swap) |
| `ServeHTTP` | `http.Handler`     |

## Structs

Name structs with **MixedCaps nouns** describing the entity. Fields follow exported/unexported rules.

```go
type Server struct {
    Addr     string        // exported
    Handler  http.Handler  // exported
    timeout  time.Duration // unexported
}
```

NEVER suffix struct names with `Struct`, `Object`, or `Data` — they add no information.

## Constants

Constants MUST use **MixedCaps**, NEVER `ALL_CAPS`. The name should explain the **role**, not the **value**.

```go
// Good — MixedCaps, name explains purpose
const MaxRetries = 3
const defaultTimeout = 30 * time.Second
const DefaultPort = 8080

// Bad — ALL_CAPS is not idiomatic Go
const MAX_RETRIES = 3
const DEFAULT_TIMEOUT = 30

// Bad — name is the value, not the purpose
const Three = 3
const Port8080 = 8080
```

### Enums (iota)

Prefix enum values with the **type name** to avoid collisions and improve readability at the call site.

```go
type Status int

const (
    StatusUnknown Status = iota // zero value = unknown/invalid
    StatusReady
    StatusRunning
    StatusDone
)

type Color int

const (
    ColorRed Color = iota + 1  // skip zero to catch uninitialized values
    ColorGreen
    ColorBlue
)
```

Choose the zero value deliberately. Use an unspecified sentinel when omission must be distinguishable from a valid selection, and validate it where required. A safe, useful default may occupy zero. Preserve existing numeric values and serialization contracts; do not insert a sentinel into an established enum as a naming cleanup.

## Errors

### Sentinel Errors

Sentinel error variables use the `Err` prefix. A package prefix can identify the origin; it is not required when the context is already clear:

```go
// Good — package prefix identifies origin
var ErrNotFound = errors.New("mypackage: not found")
var ErrPermissionDenied = errors.New("mypackage: permission denied")
var ErrTimeout = errors.New("mypackage: operation timed out")

// Also valid — wrapping or the API contract may already identify the origin
var ErrNotFound = errors.New("not found")
```

### Error Types

Custom error types use the `Error` suffix:

```go
type PathError struct {
    Op   string
    Path string
    Err  error
}

type SyntaxError struct {
    Offset int64
    msg    string
}
```

### Error Strings

Error strings normally start lowercase and omit trailing punctuation so they compose with other context. Preserve proper nouns and acronyms (`ID`, `URL`, `HTTP`), including at the beginning of an error. See [Go Code Review Comments](https://go.dev/wiki/CodeReviewComments#error-strings).

```go
// Good — ordinary words start lowercase; acronyms retain their spelling
errors.New("image: unknown format")
errors.New("mypackage: invalid message ID")
errors.New("mypackage: invalid URL")
fmt.Errorf("decoding config: %w", err)

// Bad — unnecessary capitalization and trailing punctuation
errors.New("Image: Unknown format.")
fmt.Errorf("Failed to decode config: %w.", err)
```
