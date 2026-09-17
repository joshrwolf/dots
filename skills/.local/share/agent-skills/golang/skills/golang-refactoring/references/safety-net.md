# Verifying behavior across Go refactors

Coverage helps locate unexercised code; it does not establish that tests check the
behavior a refactor could change. Characterization tests capture current behavior,
including quirks, when that behavior needs preserving. Use focused tests over the
affected callers and contracts rather than percentage thresholds to decide which
transforms are permitted.

```sh
go test -coverprofile=cover.out ./path/to/affected/...
go tool cover -func=cover.out
go tool cover -html=cover.out
```

Coverage limitations:

- **Go's coverage is statement coverage, not branch coverage** — a line inside an `if` block that ran once counts as fully covered even if the `else` never executed and even if a `switch` only ever hit one `case`. A function reporting 100% can still have an untested branch; treat the percentage as a floor on how much is exercised, not proof that the logic is correct, and read the actual branches in the code you're about to touch rather than trusting the summary.
- Use `-coverpkg` when tests in one package exercise code in another. Select the affected packages explicitly and inspect which appear in the resulting profile; a test run covering one package is not evidence about all its consumers.

## Seams — What to Introduce When There's No Net Yet

- A seam, in Michael Feathers's sense, is a place in the code where you can alter behavior without editing that exact spot.
- A seam allows a focused fake without first performing the larger refactor the test is meant to protect against.
- Two seam types matter in Go:
  - An **object seam** is an interface, or a function-typed field or parameter, injected at the point of construction — a test substitutes a fake implementation through that injection point instead of exercising the real dependency. This is the seam type that matters most in Go, because interfaces are satisfied implicitly: introducing one at the point of use requires touching only the consumer, never the producer package, which means you can add a seam to legacy code without an invasive edit to whatever it depends on.
  - A **link/build-tag seam** swaps an entire implementation at build time via `//go:build` constraints; it's used far more rarely, mostly for platform- or environment-specific substitutions where an interface would be overkill.
- The enabling move for untested code with no seam yet: extract the smallest possible interface — often just one method — at the exact call site where the untested code depends on something external (a database client, the filesystem, a clock), and inject the concrete implementation through a constructor parameter instead of constructing it inline.
  - This single move does two things at once: it breaks a potential import cycle between the consumer and whatever concrete type it depended on, and it opens the door for a fake in a characterization test, without requiring any change to the producer side at all.

```go
// Before — no seam: NewReport constructs its own client, so a test
// exercising Generate has no way to substitute a fake and is stuck
// hitting a real database.
func NewReport(dsn string) *Report {
    db, _ := sql.Open("postgres", dsn)
    return &Report{db: db}
}

func (r *Report) Generate(ctx context.Context, id int) (Summary, error) {
    row := r.db.QueryRowContext(ctx, "SELECT ... WHERE id = $1", id)
    // ...
}

// After — a one-method interface extracted at the point of use;
// the concrete *sql.DB already satisfies it implicitly, so the
// producer package needs no change at all.
type rowQuerier interface {
    QueryRowContext(ctx context.Context, query string, args ...any) *sql.Row
}

func NewReport(db rowQuerier) *Report {
    return &Report{db: db}
}

func (r *Report) Generate(ctx context.Context, id int) (Summary, error) {
    row := r.db.QueryRowContext(ctx, "SELECT ... WHERE id = $1", id)
    // ... unchanged — a characterization test can now inject a fake rowQuerier
}
```

→ See `samber/cc-skills-golang@golang-design-patterns` skill for constructor and dependency-injection patterns this move builds on, and [catalog.md](catalog.md) in this skill for the Sprout/Wrap mechanics that typically pair with a freshly introduced seam.

## Verification Command Reference

Select checks for the affected packages, consumers, and behavior:

```bash
go build ./...                              # fastest gate — compile errors
go vet ./...                                 # correctness checks the compiler doesn't do
go test ./...                                # full test suite
go test -run TestName ./pkg/...              # target one test while iterating
go test -race ./...                          # concurrency changes — see samber/cc-skills-golang@golang-testing for race-detector and testing/synctest mechanics
go test -covermode=atomic -coverpkg=./... -coverprofile=cover.out ./...
go tool cover -func=cover.out                 # per-function and total coverage, ranked
go tool cover -html=cover.out                 # visual red/green source view
go test -bench=. -benchmem -count=10 > new.txt   # capture before AND after with the same command, then:
benchstat old.txt new.txt                     # `~` means no statistically significant difference — the desired result for a behavior-preserving refactor; anything else is a signal to stop and investigate, not noise to shrug off
```

## Cross-References

- → See `samber/cc-skills-golang@golang-testing` skill for general test-writing craft, race-detector mechanics, and `testing/synctest` — this file assumes them as a baseline and only covers when a refactor's safety net needs to reach for them.
- → See `samber/cc-skills-golang@golang-benchmark` skill for interpreting a `benchstat` delta and the full profiling methodology.
- [catalog.md](catalog.md) — the Fowler refactoring catalog mapped to Go, including the Sprout/Wrap entries referenced in the seams section above.
