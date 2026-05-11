# Interfaces

## Design Principles

### Small Interfaces

Keep interfaces focused:
```go
type Reader interface {
    Read(p []byte) (n int, err error)
}

type Writer interface {
    Write(p []byte) (n int, err error)
}

type Closer interface {
    Close() error
}
```

### Accept Interfaces, Return Structs

```go
// Accept interface - flexible
func ProcessData(r io.Reader) (*Result, error) {
    // Can accept *os.File, *bytes.Buffer, etc.
}

// Return concrete type - clear
func NewProcessor() *Processor {
    return &Processor{}
}
```

### Interface Naming

Method name + "er":
```go
type Reader interface { Read() }
type Writer interface { Write() }
type Formatter interface { Format() }
type Stringer interface { String() }
```

## Composition

Combine small interfaces:
```go
type ReadWriter interface {
    Reader
    Writer
}

type ReadWriteCloser interface {
    Reader
    Writer
    Closer
}
```

## Embedding

### Struct Embedding

Promote methods from embedded types:
```go
type Server struct {
    *http.Server  // Embedded
    logger Logger
}

// Server now has all http.Server methods
s := &Server{
    Server: &http.Server{Addr: ":8080"},
}
s.ListenAndServe()  // From embedded http.Server
```

### Interface Embedding

```go
type Animal interface {
    Eat()
    Sleep()
}

type Dog interface {
    Animal  // Embedded
    Bark()
}
```

## Implementation

Interfaces are implicit:
```go
type Writer interface {
    Write(p []byte) (int, error)
}

type MyWriter struct{}

// MyWriter implements Writer automatically
func (m MyWriter) Write(p []byte) (int, error) {
    return len(p), nil
}
```

## Type Assertions

### Checking Type
```go
var i interface{} = "hello"

s, ok := i.(string)
if ok {
    fmt.Println(s)
}
```

### Type Switch
```go
func describe(i interface{}) {
    switch v := i.(type) {
    case int:
        fmt.Printf("Integer: %d\n", v)
    case string:
        fmt.Printf("String: %s\n", v)
    case bool:
        fmt.Printf("Boolean: %t\n", v)
    default:
        fmt.Printf("Unknown type: %T\n", v)
    }
}
```

## Empty Interface

`interface{}` or `any` accepts anything:
```go
func Print(v any) {
    fmt.Println(v)
}

Print(42)
Print("hello")
Print(true)
```

## Common Gotcha: Nil Interface

An interface with a nil concrete value is NOT nil:

```go
func returnsError() error {
    var err *MyError = nil
    return err  // Returns non-nil interface!
}

func main() {
    err := returnsError()
    if err != nil {
        fmt.Println("Error!")  // This prints!
    }
}
```

Fix by returning nil interface:
```go
func returnsError() error {
    var err *MyError = nil
    if err != nil {
        return err
    }
    return nil  // Correct - returns nil interface
}
```

Always use comma-ok for type assertions:
```go
s, ok := i.(string)
if ok {
    // Use s
}
```

## Best Practices

- Define interfaces where they're used, not where types are defined
- Keep interfaces small (1-3 methods ideal)
- Don't force abstractions - add interfaces when you need flexibility
- Use `any` instead of `interface{}` (Go 1.18+)
- Watch out for nil interface gotcha when returning interface types
