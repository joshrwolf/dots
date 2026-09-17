# Fakes and test fixtures

Implement the smallest consumed interface with a focused fake. Configure its
result directly and record inputs only when they are part of the contract.
Keep each fake local to a test file until multiple tests need it. Avoid creating
interfaces solely to mock implementation details.

```go
type userStore interface {
	GetUser(context.Context, string) (*User, error)
}

type fakeUserStore struct {
	user         *User
	err          error
	requestedIDs []string
}

func (f *fakeUserStore) GetUser(_ context.Context, id string) (*User, error) {
	f.requestedIDs = append(f.requestedIDs, id)
	return f.user, f.err
}

func TestServiceGetUser(t *testing.T) {
	want := &User{ID: "1", Name: "Alice"}
	store := &fakeUserStore{user: want}
	service := NewService(store)
	got, err := service.GetUser(context.Background(), "1")
	if err != nil {
		t.Fatalf("GetUser: %v", err)
	}
	if diff := cmp.Diff(want, got); diff != "" {
		t.Errorf("GetUser mismatch (-want +got):\n%s", diff)
	}
	if diff := cmp.Diff([]string{"1"}, store.requestedIDs); diff != "" {
		t.Errorf("requested IDs mismatch (-want +got):\n%s", diff)
	}
}
```

The example uses `github.com/google/go-cmp/cmp`. For an error case, configure
`err: ErrNotFound` and check `errors.Is(err, ErrNotFound)`, then check the result
is nil if that is the API contract. For concurrent calls, synchronize recorded
state and wait for the calls to complete before inspecting it.

Return fresh fixture values per test instead of sharing mutable global pointers.
Use fixed IDs and times unless randomness or time is the behavior under test.
Inject a `func() time.Time` for a simple clock dependency. For timer/goroutine
coordination, use `testing/synctest` when the project's Go version supports it;
avoid sleeps as evidence that asynchronous work finished.
