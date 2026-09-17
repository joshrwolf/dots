# Library Documentation

→ See `samber/cc-skills-golang@golang-testing` skill for writing effective Example test functions.

## Table of Contents

- [Public vs Private Libraries](#public-vs-private-libraries)
- [Go Playground Demos](#go-playground-demos)
- [Example Test Functions](#example-test-functions)
- [Code Examples in Doc Comments](#code-examples-in-doc-comments)
- [godoc and pkg.go.dev](#godoc-and-pkggodev)
- [Documentation Website](#documentation-website)
  - [Recommended Frameworks](#recommended-frameworks)
  - [Recommended Sections](#recommended-sections)

## Public vs Private Libraries

Use local `go doc` or the project's internal documentation for private packages.
Public playgrounds and package registries are only appropriate for public code.

## Go Playground Demos

For a requested public demo, Go Playground links can accompany doc comments. This lets users try your library without installing anything. Only applicable to public libraries.

Add a `Play:` line in the doc comment:

```go
// Map applies fn to each element of the slice and returns a new slice.
//
// Play: <URL of an existing public example>
//
// Example:
//
//	doubled := Map([]int{1, 2, 3}, func(x int) int { return x * 2 })
//	// doubled: [2, 4, 6]
func Map[T any, U any](s []T, fn func(T) U) []U {
```

---

## Example Test Functions

Example functions provide executable documentation for representative API usage. They appear in godoc and are verified by `go test`:

```go
// In map_example_test.go

package mypackage_test

import (
    "fmt"
    "strings"

    mypackage "github.com/{owner}/{repo}"
)

// ExampleMap demonstrates mapping over a slice.
func ExampleMap() {
    result := mypackage.Map([]int{1, 2, 3}, func(x int) int {
        return x * 2
    })
    fmt.Println(result)
    // Output: [2 4 6]
}

// ExampleMap_strings demonstrates mapping with string transformation.
func ExampleMap_strings() {
    result := mypackage.Map([]string{"hello", "world"}, strings.ToUpper)
    fmt.Println(result)
    // Output: [HELLO WORLD]
}
```

Naming conventions:

- `ExampleFuncName()` — example for a package-level function
- `ExampleTypeName()` — example for a type
- `ExampleTypeName_MethodName()` — example for a method
- `ExampleFuncName_suffix()` — multiple examples for the same function (suffix starts with a lowercase letter)
- `Example()` — example for the whole package

An `// Output:` or `// Unordered output:` comment makes the example execute and checks its output. Without either, it compiles but is not run.

---

## Code Examples in Doc Comments

Be generous with examples in doc comments. Show common use cases, edge cases, and error handling:

```go
// NewClient creates a new HTTP client with the given options.
//
// Example — basic client:
//
//	client := NewClient()
//
// Example — with custom timeout and retries:
//
//	client := NewClient(
//	    WithTimeout(10 * time.Second),
//	    WithRetries(3),
//	    WithRetryBackoff(time.Second),
//	)
//
// Example — with authentication:
//
//	client := NewClient(
//	    WithBearerToken(os.Getenv("API_TOKEN")),
//	)
func NewClient(opts ...Option) *Client {
```

---

## godoc and pkg.go.dev

[pkg.go.dev](https://pkg.go.dev) renders public module versions after indexing them; tagging or importing a module locally does not itself guarantee indexing. This is the primary documentation surface for public Go libraries.

**How godoc renders comments:**

- First sentence of each doc comment appears in the package index
- `// Package foo provides...` appears as the package description
- Code blocks (indented by one tab) render as formatted code
- `# Heading` syntax (Go 1.19+) creates sections
- `[Link text]` creates a link when a corresponding `[Link text]: URL` definition is present
- `[Identifier]` links to other symbols in the package
- `Deprecated:` marker gets special styling

**For private libraries:** pkg.go.dev won't index private modules. Use `go doc` locally or run `pkgsite` on your internal network. Some teams set up a shared pkgsite instance for internal Go modules.

```bash
# View docs for a specific symbol
go doc github.com/{owner}/{repo}.FuncName

# View full package docs
go doc -all github.com/{owner}/{repo}

# Start a local godoc server
go get -tool golang.org/x/pkgsite/cmd/pkgsite@latest
go tool pkgsite -http=localhost:6060
# Then open http://localhost:6060
```

---

## Documentation Website

For larger libraries or frameworks, consider a dedicated documentation website.

### Recommended Frameworks

- **Docusaurus** (React-based) — best for large projects, supports versioning natively
- **MkDocs Material** (Python-based) — simpler setup, great search, clean design

Both can be deployed on Vercel.

### Recommended Sections

Follow the [Diataxis framework](https://diataxis.fr/) for organizing documentation:

| Section | Purpose | Example |
| --- | --- | --- |
| Getting Started | First steps, installation, hello world | "Install and run your first query in 5 minutes" |
| Tutorial | Step-by-step learning | "Build a REST API with authentication" |
| How-to Guides | Task-oriented recipes | "How to configure connection pooling" |
| Reference | Complete API documentation | Auto-generated from godoc |
| Deep dive / internals | Conceptual understanding | "How the scheduler algorithm works" |
