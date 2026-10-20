---
name: golang-error-handling
description: "Design and review Go error handling. Use for wrapping, errors.Is/As, error types, panic boundaries, and deciding where to handle or log a failure."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.1"
  openclaw:
    emoji: "⚠"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent
paths:
  - "**/*.go"
---

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-error-handling` skill takes precedence.

# Go Error Handling Best Practices

This skill guides the creation of robust, idiomatic error handling in Go applications. Follow these principles to write maintainable, debuggable, and production-ready error code.

## Best Practices Summary

1. **Handle errors according to the API contract** — do not discard meaningful failures; intentional ignores need a reason. Some APIs return useful data together with an error, such as `io.Reader.Read`
2. **Add useful context without repeating it at every layer**; return an error unchanged when it already describes the operation
3. **Start error strings lowercase**, except proper nouns and acronyms; avoid trailing punctuation
4. **Choose wrapping by the error contract**: `%w` exposes the cause to callers; `%v` preserves its text but prevents unwrapping. Neither redacts sensitive text
5. **MUST use `errors.Is` for sentinel matching and `errors.As`/`errors.AsType` for typed chain inspection** instead of direct comparison or bare type assertions. For Go 1.26+, prefer `errors.AsType[T](err)` when `T` implements `error`; use `errors.As(err, &target)` for Go <1.26 or for non-error interface targets.
6. **SHOULD use `errors.Join`** (Go 1.20+) to combine independent errors
7. **Avoid duplicate error reports** — normally return to the layer owning reporting. Retry diagnostics or explicitly requested local logging can coexist with propagation
8. **Use sentinel errors** for expected conditions, custom types for carrying data
9. **NEVER use `panic` for expected error conditions** — reserve it for violated invariants or explicit panic-based APIs
10. **SHOULD use `slog`** (Go 1.21+) for structured error logging — not `fmt.Println` or `log.Printf`
11. **Use existing error types and logging conventions**; `samber/oops` guidance applies when the project uses that library
12. **Log HTTP requests** with structured middleware capturing method, path, status, and duration
13. **Use log levels** to indicate error severity
14. **Never expose technical errors to users** — translate internal errors to user-friendly messages, log technical details separately
15. **Keep log grouping low-cardinality** — at logging/APM boundaries, keep message templates stable and attach IDs, paths, line numbers, and counts as structured attributes. Error values may include useful operational context, but avoid putting high-cardinality data into the stable log message used for grouping.

## Detailed Reference

- **[Error Creation](./references/error-creation.md)** — How to create errors that tell the story: error messages normally start lowercase, preserve proper nouns/acronyms, and omit trailing punctuation, and describe what happened without prescribing action. Covers sentinel errors (one-time preallocation for performance), custom error types (for carrying rich context), and the decision table for which to use when.

- **[Error Wrapping and Inspection](./references/error-wrapping.md)** — Choosing whether callers should be able to inspect an underlying error. How to inspect chains with `errors.Is`, `errors.As`, and Go 1.26+ `errors.AsType` for type-safe error handling, and `errors.Join` for combining independent errors.

- **[Error Handling Patterns and Logging](./references/error-handling.md)** — Reporting ownership and avoiding duplicate logs. Panic/recover design, `samber/oops` for production errors, and `slog` structured logging integration for APM tools.

## Organizing Error Handling Audits

For a requested error-handling audit, organize findings by category:

- Error creation — validate `errors.New`/`fmt.Errorf` usage, low-cardinality messages, custom types
- Error wrapping — audit `%w` vs `%v`, verify `errors.Is`/`errors.As` patterns
- Reporting ownership — find duplicate reports, swallowed failures, and unjustified discarded errors (`_`)
- Panic/recover — audit `panic` usage, check whether recovery can safely contain the failure and report it
- Structured logging — verify `slog` usage at error sites, check for PII in error messages

## Cross-References

- → See `samber/cc-skills-golang@golang-samber-oops` for full samber/oops API, builder patterns, and logger integration
- → See `samber/cc-skills-golang@golang-observability` for structured logging setup, log levels, and request logging middleware
- → See `samber/cc-skills-golang@golang-safety` for nil interface trap and nil error comparison pitfalls
- → See `samber/cc-skills-golang@golang-naming` for error naming conventions (ErrNotFound, PathError)
- → See `samber/cc-skills-golang@golang-continuous-integration` skill for automated AI-driven code review in CI using these guidelines

## References

- [lmittmann/tint](https://github.com/lmittmann/tint)
- [samber/oops](https://github.com/samber/oops)
- [samber/slog-multi](https://github.com/samber/slog-multi)
- [samber/slog-sampling](https://github.com/samber/slog-sampling)
- [samber/slog-formatter](https://github.com/samber/slog-formatter)
- [samber/slog-http](https://github.com/samber/slog-http)
- [samber/slog-sentry](https://github.com/samber/slog-sentry)
- [log/slog package](https://pkg.go.dev/log/slog)
