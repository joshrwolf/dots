# Control flow and representation pitfalls

## Missing Return After HTTP Error Response

After writing an error with `http.Error()`, execution continues. This can cause double writes, corrupted responses, or executing logic that should have been skipped.

```go
// BAD
func handler(w http.ResponseWriter, r *http.Request) {
    if !authorized(r) {
        http.Error(w, "Forbidden", http.StatusForbidden)
        // BUG: missing return — handler keeps executing
    }
    doSensitiveAction(r)
}

// GOOD
func handler(w http.ResponseWriter, r *http.Request) {
    if !authorized(r) {
        http.Error(w, "Forbidden", http.StatusForbidden)
        return
    }
    doSensitiveAction(r)
}
```

## JSON Pitfalls

### Numbers into `interface{}` become `float64`

When unmarshaling into `map[string]interface{}` or `interface{}`, all JSON numbers become `float64`. Type-asserting to `int` panics. Integers outside the exactly representable float64 range can lose precision.

```go
// BAD
var result map[string]interface{}
json.Unmarshal([]byte(`{"id": 1234567890123456789}`), &result)
id := result["id"].(int) // PANIC: it's float64, not int

// GOOD — use typed struct (preferred)
type Response struct {
    ID int64 `json:"id"`
}

// GOOD — use json.Number when you must use interface{}
dec := json.NewDecoder(bytes.NewReader(data))
dec.UseNumber()
var result map[string]interface{}
if err := dec.Decode(&result); err != nil {
    return err
}
number, ok := result["id"].(json.Number)
if !ok {
    return fmt.Errorf("id must be a JSON number")
}
id, err := number.Int64()
if err != nil {
    return fmt.Errorf("id: %w", err)
}
// Use id in the application.
```

### Unexported fields silently ignored

Fields starting with lowercase are invisible to `encoding/json`. Marshal produces empty output, unmarshal skips them — no error in either case.

```go
// BAD
type User struct {
    name  string `json:"name"`  // unexported — silently ignored!
    email string `json:"email"` // unexported — silently ignored!
}
u := User{name: "Alice", email: "alice@example.com"}
data, _ := json.Marshal(u) // data is "{}" — no error

// GOOD
type User struct {
    Name  string `json:"name"`
    Email string `json:"email"`
}
```

**Detect:** `go vet` warns when unexported fields have JSON struct tags.

## `strings.Trim` vs `strings.TrimPrefix`

`strings.Trim` treats its second argument as a **set of characters** to strip from both ends, not as a substring. This over-trims unexpectedly.

```go
// BAD
s := strings.Trim("application/json", "application/")
// Result: "js" — stripped all chars in set {a,p,l,i,c,t,o,n,/} from both ends!

// GOOD
s := strings.TrimPrefix("application/json", "application/")
// Result: "json"
```

Use `strings.TrimPrefix`/`strings.TrimSuffix` to remove substrings. Only use `strings.Trim` when you intend to strip a set of characters.

## `break` in `select`/`switch` Inside `for` Loop

A bare `break` inside a `select` or `switch` that is inside a `for` loop only exits the `select`/`switch`, not the loop.

```go
// BAD
for {
    select {
    case msg := <-ch:
        if msg == "quit" {
            break // BUG: only breaks the select, loop continues forever
        }
        process(msg)
    }
}

// GOOD — use labeled break
loop:
for {
    select {
    case msg := <-ch:
        if msg == "quit" {
            break loop // breaks the for loop
        }
        process(msg)
    }
}
```

## `os.Exit` Skips Deferred Functions

`os.Exit` terminates the process immediately. No deferred functions run — cleanup, flush, and close operations are skipped. `log.Fatal` calls `os.Exit(1)` internally and has the same problem.

```go
// BAD — deferred cleanup never runs
func main() {
    f, _ := os.Create("data.tmp")
    defer f.Close()       // NEVER RUNS
    defer os.Remove(f.Name()) // NEVER RUNS

    if err := process(); err != nil {
        log.Fatal(err) // calls os.Exit(1) — skips all defers!
    }
}

// GOOD — return from main instead, or restructure so defers run
func main() {
    if err := run(); err != nil {
        fmt.Fprintf(os.Stderr, "error: %v\n", err)
        os.Exit(1) // defers in run() already ran when it returned
    }
}

func run() error {
    f, err := os.Create("data.tmp")
    if err != nil {
        return err
    }
    defer os.Remove(f.Name())
    defer f.Close()
    return process()
}
```

## `time.Time` Comparison: `==` vs `.Equal()`

`time.Time` can include a monotonic clock reading. Two `time.Time` values representing the same instant may not be `==` if one has a monotonic component and the other doesn't (e.g., one from `time.Now()`, the other deserialized from JSON/database).

```go
// BAD — may fail even for the same instant
t1 := time.Now()
data, _ := t1.MarshalJSON()
var t2 time.Time
t2.UnmarshalJSON(data)
fmt.Println(t1 == t2) // false! t1 has monotonic, t2 doesn't

// GOOD — Equal compares instants; here t2 has no monotonic reading
fmt.Println(t1.Equal(t2)) // true

// Strip monotonic explicitly if the storage/comparison contract requires it
t1 = t1.Round(0) // strips monotonic reading
```

## `fallthrough` in `switch` Executes Unconditionally

Unlike C, Go's `switch` cases don't fall through by default. But when you explicitly use `fallthrough`, it executes the **next case body unconditionally** — it does not check the next case's condition.

```go
// Surprising: fallthrough doesn't check the next condition
switch x := 5; {
case x > 10:
    fmt.Println(">10")
    fallthrough
case x > 0:
    fmt.Println(">0")
    fallthrough
case x < 0:
    fmt.Println("<0") // EXECUTES even though 5 is not < 0!
}
// Output: >0, <0

// fallthrough is rarely needed. Prefer listing multiple values:
switch status {
case "active", "enabled":
    enable()
}
```
