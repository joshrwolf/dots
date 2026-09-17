---
name: golang-documentation
description: "Write or review Go documentation. Use for package and API comments, executable examples, README files, and contributor or release documentation."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.2"
  openclaw:
    emoji: "📝"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch
paths:
  - "**/*.go"
---

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-documentation` skill takes precedence.

# Go Documentation

## Cross-References

- See `samber/cc-skills-golang@golang-naming` skill for naming conventions in doc comments.
- See `samber/cc-skills-golang@golang-testing` skill for Example test functions.
- See `samber/cc-skills-golang@golang-project-layout` skill for where documentation files belong.

## Writing Principles

Apply to every piece of documentation you write or review:

**Concision** — write the shortest version that carries the idea. Remove ornament and hollow transitions. Never drop facts, warnings, or user-requested depth.

**Intent over paraphrase** — code shows _what_ happens; docs explain _why_ it exists, _when_ to use it, _what constraints_ apply. A comment that only restates the signature wastes the reader's time.

**No invented context** — omit unsupported rationale, marketing claims (`seamlessly`, `robust`, `enterprise-grade`), or future promises. Leave gaps visible rather than filling with speculation.

**Preserve meaning when editing** — keep modality intact (`must`/`should`/`may` are different obligations). Preserve conditions, warnings, required actions. A cleaner sentence that changes obligations is wrong.

**Anti-patterns to remove on sight:** pure-paraphrase comments that start with the name but add nothing (the Go convention is to start with the name; useful comments add the API contract), signature restatement, marketing vocabulary, groundless future claims (`future extensibility`, `easy to scale`), hollow transitions (`it's worth noting that`, `in conclusion`), template padding that adds no information.

## Function & Method Doc Comments

Every exported function and method MUST have a doc comment. Document complex internal functions too. Skip test functions.

The comment starts with the function name and a verb phrase. Focus on **why** and **when**, not restating what the code already shows. The code tells you _what_ happens — the comment should explain _why_ it exists, _when_ to use it, _what constraints_ apply, and _what can go wrong_. Explain parameter constraints, error behavior, and usage where they are not apparent from the signature. This longer example illustrates those details; it is not a required comment template:

```go
// CalculateDiscount computes the final price after applying tiered discounts.
// Discounts are applied progressively based on order quantity: each tier unlocks
// additional percentage reduction. Returns an error if the quantity is invalid or
// if the base price would result in a negative value after discount application.
//
// Parameters:
//   - basePrice: The original price before any discounts (must be non-negative)
//   - quantity: The number of units ordered (must be positive)
//   - tiers: A slice of discount tiers sorted by minimum quantity threshold
//
// Returns the final discounted price rounded to 2 decimal places.
// Returns ErrInvalidPrice if basePrice is negative.
// Returns ErrInvalidQuantity if quantity is zero or negative.
//
// Example:
//
//	tiers := []DiscountTier{
//	    {MinQuantity: 10, PercentOff: 5},
//	    {MinQuantity: 50, PercentOff: 15},
//	    {MinQuantity: 100, PercentOff: 25},
//	}
//	finalPrice, err := CalculateDiscount(100.00, 75, tiers)
//	if err != nil {
//	    log.Fatalf("Discount calculation failed: %v", err)
//	}
//	log.Printf("Ordered 75 units at $100 each: final price = $%.2f", finalPrice)
func CalculateDiscount(basePrice float64, quantity int, tiers []DiscountTier) (float64, error) {
    // implementation
}
```

For the full comment format, deprecated markers, interface docs, and file-level comments, see **[Code Comments](./references/code-comments.md)** — how to document packages, functions, interfaces, and when to use `Deprecated:` markers and `BUG:` notes.

## Project documentation

Follow the project's existing documentation structure. For release-note examples
and distribution details, see [Project Docs](./references/project-docs.md).

## Release notes

Write each entry to answer what changed for the reader. Internal refactors without
user-visible impact belong in commit history; a fixed edge case should not become
a broad reliability claim. Use the project's changelog or release-note convention.

## Library-Specific Documentation

Executable `ExampleXxx` functions in `_test.go` files can appear in package
documentation and be verified by `go test`. Use `go doc` to inspect the resolved
package's API comments; see `golang-go-doc`.

See [Library Documentation](./references/library.md) for details.

## Application-Specific Documentation

For Go applications/CLIs:

- **Installation methods** — pre-built binaries (GoReleaser), `go install`, Docker images, Homebrew...
- **CLI help text** — make `--help` comprehensive; it's the primary documentation
- **Configuration docs** — document all env vars, config files, CLI flags

See [Application Documentation](./references/application.md) for details.

## API Documentation

If your project exposes an API:

| API Style    | Format      | Tool                                         |
| ------------ | ----------- | -------------------------------------------- |
| REST/HTTP    | OpenAPI 3.x | Existing project schema workflow |
| Event-driven | AsyncAPI    | Manual or code-gen                           |
| gRPC         | Protobuf    | buf, grpc-gateway                            |

Use the project's schema-first or code-first workflow; neither direction is universally preferred. See [Application Documentation](./references/application.md#api-documentation) for details.

## Delivery Documentation

Document how users get your project:

**Libraries:**

```bash
go get github.com/{owner}/{repo}
```

**Applications:**

```bash
# Pre-built binaries: link the actual release assets and document extraction,
# checksum verification, and installation for each supported platform.

# From source
go install github.com/{owner}/{repo}@latest

# Docker
docker pull {registry}/{owner}/{repo}:latest
```

See [Project Docs](./references/project-docs.md#distribution) for Dockerfile best practices and Homebrew tap setup.
