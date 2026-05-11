# Conventions

## Naming

### Packages
- Lowercase, single-word (`http`, `bytes`, `user`)
- No underscores or mixedCaps
- Name by what it provides: `http` not `httputils`
- Avoid generic names: `util`, `common`, `helpers`

### Variables and Functions
- Exported: `MixedCaps` (e.g., `UserID`, `ParseRequest`)
- Unexported: `mixedCaps` (e.g., `userID`, `parseRequest`)
- Acronyms: all caps (`ID`, `HTTP`, `URL`, not `Id`, `Http`)

### Interfaces
- Single method: method name + "er" (`Reader`, `Writer`, `Closer`)
- Multiple methods: descriptive name (`FileSystem`, `ResponseWriter`)

### Receivers
- Use short, consistent names (1-2 letters)
- Same name across all methods
```go
func (s *Server) Start() error { ... }
func (s *Server) Stop() error { ... }
```

## Project Structure

Standard layout:
```
myproject/
├── cmd/
│   └── myapp/
│       └── main.go
├── internal/
│   ├── service/
│   └── repository/
├── pkg/
│   └── shared/
├── go.mod
└── go.sum
```

- `cmd/`: Application entry points (main packages)
- `internal/`: Private code (cannot be imported by other projects)
- `pkg/`: Public libraries (can be imported by external projects)

## Documentation

### Package Comments
```go
// Package http provides HTTP client and server implementations.
package http
```

### Exported Declarations
```go
// Get issues a GET request to the specified URL.
// Returns an error if the request fails.
func Get(url string) (*Response, error)
```

Use complete sentences starting with the name being documented.

## Variable Declaration

```go
// Short declaration (inside functions)
name := "value"

// Var for zero values or explicit type
var count int
var buf bytes.Buffer

// Multiple declarations
var (
    maxRetries = 3
    timeout    = 30 * time.Second
)
```

## Control Structures

### If with Initialization
```go
if err := doSomething(); err != nil {
    return err
}
```

### Switch with Initialization
```go
switch val := getValue(); val {
case "a":
    // ...
case "b":
    // ...
default:
    // ...
}
```

No `break` needed - cases don't fall through by default.

## Allocation

### new vs make
- `new(T)` allocates zeroed memory, returns `*T`
- `make(T)` initializes slices, maps, channels; returns `T`

```go
p := new(int)           // *int, value is 0
s := make([]int, 10)    // []int, len=10, initialized
m := make(map[string]int) // map, initialized and ready
```

### Zero Values
Design types where zero value is useful:
```go
var buf bytes.Buffer  // Ready to use immediately
buf.WriteString("hello")

var mu sync.Mutex     // Ready to use
mu.Lock()
```

## Slices

### Slice Mechanics Gotcha

Slices share underlying arrays:
```go
a := []int{1, 2, 3, 4, 5}
b := a[0:3]  // [1, 2, 3]
b[0] = 99    // Modifies a too!
// a is now [99, 2, 3, 4, 5]
```

To avoid sharing:
```go
b := make([]int, len(a))
copy(b, a)
```

### Appending
```go
s = append(s, elem)           // Add one
s = append(s, elem1, elem2)   // Add multiple
s = append(s, anotherSlice...)  // Append slice
```

## Maps

Zero value is nil - must initialize:
```go
var m map[string]int  // nil - NOT ready
m["key"] = 1          // panic!

m = make(map[string]int)  // Ready to use
m["key"] = 1              // OK
```

Check for key existence:
```go
val, ok := m["key"]
if ok {
    // Key exists
}
```

Delete keys:
```go
delete(m, "key")
```

## Defer

### Defer Order
LIFO (last in, first out):
```go
defer fmt.Println("1")
defer fmt.Println("2")
defer fmt.Println("3")
// Prints: 3, 2, 1
```

### Defer with Cleanup
```go
f, err := os.Open("file")
if err != nil {
    return err
}
defer f.Close()

// Rest of function
```

### Arguments Evaluated Immediately
```go
i := 0
defer fmt.Println(i)  // Prints 0, not 1
i++
```

## Pointer Receivers

Use pointer receivers when:
- Method modifies the receiver
- Receiver is a large struct
- Consistency (if one method needs pointer, all should use pointer)

```go
type Counter struct {
    count int
}

// Pointer - modifies receiver
func (c *Counter) Increment() {
    c.count++
}

// Pointer - consistency with Increment
func (c *Counter) Count() int {
    return c.count
}
```

## String Building

Strings are immutable - concatenation creates new strings.

### Inefficient
```go
s := ""
for i := 0; i < 1000; i++ {
    s += "x"  // Creates 1000 new strings
}
```

### Efficient
```go
var b strings.Builder
for i := 0; i < 1000; i++ {
    b.WriteString("x")
}
s := b.String()
```

## Error Messages

- Lowercase, no punctuation
- Add context, don't just return
```go
// Good
return fmt.Errorf("failed to connect to database: %w", err)

// Bad
return err  // No context
return fmt.Errorf("Failed to connect.")  // Capitalized, punctuation
```

## Variable Shadowing

Be careful with `:=` in nested scopes:
```go
var err error
if condition {
    result, err := doSomething()  // New err (shadowing)
    _ = result
}
// Outer err is still nil!

// Fix - use = instead of :=
var result Result
if condition {
    result, err = doSomething()  // Assigns to outer err
}
```
