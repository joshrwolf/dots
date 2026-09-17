# Testing GraphQL in Go

## Table of Contents

- [gqlgen — Client Harness](#gqlgen--client-harness)
- [gqlgen — Testing with DataLoaders](#gqlgen--testing-with-dataloaders)
- [gqlgen — Testing Subscriptions](#gqlgen--testing-subscriptions)
- [graph-gophers — gqltesting](#graph-gophers--gqltesting)
- [Testing Error Handling](#testing-error-handling)
- [Testing Auth Directives (gqlgen)](#testing-auth-directives-gqlgen)
- [Table-Driven Tests](#table-driven-tests)

## gqlgen — Client Harness

The `github.com/99designs/gqlgen/client` package drives the full stack (directives, middleware, resolvers) via an `http.Handler`:

```go
func TestCreateUser(t *testing.T) {
	// Build the full handler with real dependencies (use a test DB)
	srv := handler.NewDefaultServer(graph.NewExecutableSchema(graph.Config{
		Resolvers: &graph.Resolver{
			DB: testDB,
		},
	}))

	c := client.New(srv)

	var resp struct {
		CreateUser struct {
			User struct {
				ID    string
				Email string
			}
			Errors []struct{ Message string }
		}
	}

	c.MustPost(`
        mutation CreateUser($email: String!, $name: String!) {
            createUser(input: {email: $email, name: $name}) {
                user { id email }
                errors { message }
            }
        }
    `, &resp,
		client.Var("email", "alice@example.com"),
		client.Var("name", "Alice"),
		client.AddHeader("Authorization", "Bearer test-token"),
	)

	if len(resp.CreateUser.Errors) != 0 {
		t.Fatalf("expected empty value, got %v", resp.CreateUser.Errors)
	}
	if got, want := resp.CreateUser.User.Email, "alice@example.com"; got != want {
		t.Fatalf("got %v, want %v", got, want)
	}
}
```

For unit testing individual resolvers, call resolver methods directly with a constructed `Resolver` and a real `context.Context` — no HTTP overhead.

## gqlgen — Testing with DataLoaders

Wrap the test server with the DataLoader middleware so resolver tests exercise the full batching path:

```go
srv := handler.NewDefaultServer(es)
h := dataloaders.Middleware(testDB, srv)

c := client.New(h)
```

## gqlgen — Testing Subscriptions

Use `client.Subscription` to test subscription resolvers:

```go
sub := c.Subscription(`subscription { messageAdded(room: "general") { content } }`)
defer sub.Close()

// Trigger an event
publishMessage("general", "hello")

var event struct{ MessageAdded struct{ Content string } }
err := sub.Next(&event)
if err := err; err != nil {
	t.Fatalf("unexpected error: %v", err)
}
if got, want := event.MessageAdded.Content, "hello"; got != want {
	t.Fatalf("got %v, want %v", got, want)
}
```

## graph-gophers — gqltesting

```go
func TestUser(t *testing.T) {
    gqltesting.RunTests(t, []*gqltesting.Test{
        {
            Schema: schema,
            Query: `{ user(id: "1") { name email } }`,
            ExpectedResult: `{"user":{"name":"Alice","email":"alice@example.com"}}`,
        },
        {
            Schema:        schema,
            Query:         `{ user(id: "999") { name } }`,
            ExpectedErrors: []*gqlerrors.QueryError{
                {Message: "user not found", Extensions: map[string]any{"code": "NOT_FOUND"}},
            },
        },
    })
}
```

For HTTP-level tests:

```go
func TestRelayHandler(t *testing.T) {
	body := `{"query":"{ user(id: \"1\") { name } }"}`
	req := httptest.NewRequest(http.MethodPost, "/graphql", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()

	relay.Handler{Schema: schema}.ServeHTTP(w, req)

	if got, want := w.Code, http.StatusOK; got != want {
		t.Fatalf("got %v, want %v", got, want)
	}
	if !strings.Contains(w.Body.String(), `"Alice"`) {
		t.Fatalf("output %q does not contain %q", w.Body.String(), `"Alice"`)
	}
}
```

## Testing Error Handling

Verify error extensions reach the client:

```go
var resp struct {
	Errors []struct {
		Message    string
		Extensions struct{ Code string }
	}
}
c.Post(`{ user(id: "999") { name } }`, &resp)
if got, want := resp.Errors[0].Extensions.Code, "NOT_FOUND"; got != want {
	t.Fatalf("got %v, want %v", got, want)
}
```

## Testing Auth Directives (gqlgen)

Test the directive function directly:

```go
func TestHasRoleDirective(t *testing.T) {
	ctx := context.WithValue(context.Background(), userKey, &domain.User{Role: "USER"})
	_, err := HasRole(ctx, nil, func(ctx context.Context) (any, error) {
		return "ok", nil
	}, model.RoleAdmin)
	if err == nil {
		t.Fatalf("expected an error, got nil")
	}

	var gqlErr *gqlerror.Error
	if !(errors.As(err, &gqlErr)) {
		t.Fatalf("condition failed: errors.As(err, &gqlErr)")
	}
	if got, want := gqlErr.Extensions["code"], "FORBIDDEN"; got != want {
		t.Fatalf("got %v, want %v", got, want)
	}
}
```

## Table-Driven Tests

```go
func TestUserQueries(t *testing.T) {
	tests := []struct {
		name     string
		query    string
		vars     map[string]any
		wantCode string
		wantName string
	}{
		{"existing user", `query($id:ID!){user(id:$id){name}}`, map[string]any{"id": "1"}, "", "Alice"},
		{"missing user", `query($id:ID!){user(id:$id){name}}`, map[string]any{"id": "999"}, "NOT_FOUND", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var resp struct {
				User   *struct{ Name string }
				Errors []struct {
					Extensions struct{ Code string }
				}
			}
			c.Post(tt.query, &resp, client.Var("id", tt.vars["id"]))
			if tt.wantCode != "" {
				if got, want := resp.Errors[0].Extensions.Code, tt.wantCode; got != want {
					t.Fatalf("got %v, want %v", got, want)
				}
			} else {
				if got, want := resp.User.Name, tt.wantName; got != want {
					t.Fatalf("got %v, want %v", got, want)
				}
			}
		})
	}
}
```

For testing patterns across the codebase, see the `samber/cc-skills-golang@golang-testing` skill.
