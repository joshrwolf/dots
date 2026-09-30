# Testing database code

Examples use test-owned contexts on Go 1.24+; see `golang-testing` for cleanup ordering.

Use a focused fake for service behavior and a real test database for SQL semantics.

## Service tests with a fake

Keep the fake's signature identical to the consumed interface. Here a missing
record is represented by `found == false`, separate from a query failure.

```go
type userReader interface {
	GetByID(context.Context, int64) (*User, bool, error)
}

type fakeUserReader struct {
	user        *User
	found       bool
	err         error
	requestedID int64
}

func (f *fakeUserReader) GetByID(_ context.Context, id int64) (*User, bool, error) {
	f.requestedID = id
	return f.user, f.found, f.err
}

func TestUserServiceGetUser(t *testing.T) {
	want := &User{ID: 1, Name: "Alice", Email: "alice@example.com"}
	reader := &fakeUserReader{user: want, found: true}
	svc := NewUserService(reader)
	got, err := svc.GetUser(t.Context(), 1)
	if err != nil {
		t.Fatalf("GetUser: %v", err)
	}
	if diff := cmp.Diff(want, got); diff != "" {
		t.Errorf("GetUser mismatch (-want +got):\n%s", diff)
	}
	if reader.requestedID != 1 {
		t.Errorf("requested ID = %d, want 1", reader.requestedID)
	}
}
```

Import `github.com/google/go-cmp/cmp` for the structural diff. Add separate
not-found and query-error cases, checking the service's error contract with
`errors.Is`. Keep the fake small; it does not validate query syntax or schema.

## Query-level tests

Exercise SQL against an isolated test database with the production schema.
Check query results, not-found behavior, driver errors, constraints, and NULL
round-trips using explicit test failures and structural diffs where helpful.

## Integration tests

Use an isolated test database with the production schema applied. Register
connection cleanup immediately; start a transaction per test and roll it back
with `t.Cleanup`. The repository must accept the transaction and issue all its
queries through it. Tests of commits or multiple connections need a separate
schema or database instead of a rollback fixture.

```go
func repositoryTx(t *testing.T) *sqlx.Tx {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Fatal("TEST_DATABASE_URL must point to an isolated test database")
	}
	db, err := sqlx.Connect("postgres", dsn)
	if err != nil {
		t.Fatalf("connect test database: %v", err)
	}
	t.Cleanup(func() {
		if err := db.Close(); err != nil {
			t.Errorf("close database: %v", err)
		}
	})
	tx, err := db.Beginx()
	if err != nil {
		t.Fatalf("begin test transaction: %v", err)
	}
	t.Cleanup(func() {
		if err := tx.Rollback(); err != nil {
			t.Errorf("rollback test transaction: %v", err)
		}
	})
	return tx
}
```

Use `//go:build integration` when that is the repository's integration-test gate,
and run with `go test -tags=integration ./...`. For container-backed fixtures,
register termination immediately after successful startup, wait for readiness,
and fail setup on errors. Avoid fixed sleeps and shared mutable test data.

See `golang-testing` for fixture organization and `golang-go-cmp` for structured
result comparisons.
