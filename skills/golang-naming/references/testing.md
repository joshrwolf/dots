# Test Naming

## Test Functions

Test functions follow `Test` + the name of what is being tested. Use underscores for subcases.

```go
func TestParseToken(t *testing.T) { ... }
func TestServer_Handle(t *testing.T) { ... }           // method test
func TestParseToken_InvalidInput(t *testing.T) { ... }  // subcase
```

## Table-Driven Tests

Table-driven test case names should be descriptive phrases. Ordinary words can be lowercase; acronyms may retain their spelling. Go does not require lowercase subtest names. Use `input` for inputs and `expected` for expected outputs to make the data flow clear:

```go
tests := []struct {
    name         string
    input        string
    expectedCode int
    expectedErr  bool
}{
    {name: "empty input", input: "", expectedCode: 400, expectedErr: true},
    {name: "valid token", input: "abc123", expectedCode: 200},
    {name: "expired token", input: "exp", expectedCode: 401, expectedErr: true},
    {name: "invalid ID", input: "???", expectedCode: 400, expectedErr: true},
}

// Prefer descriptive names over opaque labels such as "case 1".
```

## Test Helpers

Test helper functions that panic on failure conventionally use the `must` prefix: `mustLoadFixture()`, `mustParseURL()`.
