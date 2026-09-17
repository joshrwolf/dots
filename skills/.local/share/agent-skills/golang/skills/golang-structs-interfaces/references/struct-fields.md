# Struct Fields: Tags and Copy Safety

## Struct Field Tags

Field tags drive every reflection-based encoder and decoder. Exported fields in serialized structs MUST have field tags — without one, the encoder falls back to the Go field name, so renaming a field silently changes the wire format:

```go
type Order struct {
    ID        string    `json:"id"         db:"id"`
    UserID    string    `json:"user_id"    db:"user_id"`
    Total     float64   `json:"total"      db:"total"`
    Items     []Item    `json:"items"      db:"-"`
    CreatedAt time.Time `json:"created_at" db:"created_at"`
    DeletedAt time.Time `json:"-"          db:"deleted_at"`
    Internal  string    `json:"-"          db:"-"`
}
```

| Directive               | Meaning                                     |
| ----------------------- | ------------------------------------------- |
| `json:"name"`           | Field name in JSON output                   |
| `json:"name,omitempty"` | Omit empty values (v1 rules)                    |
| `json:"name,omitzero"`  | Omit field if zero (Go 1.24+, type-aware)   |
| `json:"-"`              | Always exclude from JSON                    |
| `json:"-,"`             | Field literally named `-`                   |
| `json:",string"`        | Encode number/bool as JSON string           |
| `db:"column"`           | Database column mapping (sqlx, etc.)        |
| `yaml:"name"`           | YAML field name                             |
| `xml:"name,attr"`       | XML attribute                               |
| `validate:"required"`   | Struct validation (go-playground/validator) |

Notes that bite in production:

- **Unexported fields are never serialized**, tag or not. A tag on an unexported field is dead weight and `go vet` flags the malformed ones only.
- **In `encoding/json` v1, `omitempty` uses its documented empty-value rules.** It drops `0`, `""`, and `false`, so an explicit "zero" value becomes indistinguishable from "absent". For an always-present boolean, use no omission tag. To distinguish absent from explicit false, use `*bool` with `omitempty`: nil is omitted, a pointer to false is emitted. `omitzero` also omits plain false; it does not track presence. It can omit zero `time.Time` using `IsZero`. Preserve the chosen JSON API and wire contract when changing tags.
- **Tag syntax is unchecked at compile time.** A missing backtick or a `json: "id"` with a stray space produces a silently ignored tag. `go vet`'s `structtag` analyzer catches malformed tags — run it in CI.

**Diagnose:** 1- `go vet ./...` — the `structtag` analyzer reports malformed or duplicated tags 2- round-trip test: marshal, unmarshal, compare — catches field-name drift the compiler cannot see

## Copy safety and static analysis

Do not copy a used `sync.Mutex` or a struct containing it. Lock state belongs to
that value. Channels and pointers instead copy references to shared state; that
sharing may be intended. Assess ownership and synchronization rather than treating
all reference-bearing structs as non-copyable.

A mutex field already lets `go vet`'s `copylocks` analyzer detect many accidental
copies. Use pointer receivers and parameters for lock-bearing values. A `noCopy`
marker is useful for a documented non-copyable type without a detectable lock:

```go
type noCopy struct{}

func (*noCopy) Lock()   {}
func (*noCopy) Unlock() {}

type Handle struct {
    noCopy noCopy // named field; do not promote Lock/Unlock into the public API
    state  *state // this type's ownership contract forbids copying
}
```

The marker works through method-set analysis, not a magic type name. It is a
static-analysis convention, not a language restriction or runtime enforcement.
The compiler still permits copies, and vet is not an exhaustive proof. Avoid
adding a redundant marker to every struct that already contains a mutex.

Run `go vet` for copy diagnostics and exercise concurrent behavior with `go test
-race`; a passing run does not prove every possible execution safe.

Sources: [sync.Mutex](https://pkg.go.dev/sync#Mutex),
[copylocks analyzer](https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/copylock).
