---
name: idiomatic-rust
description: "Write, review, and refactor Rust using Google's official Rust Style Guide as the canonical baseline. Use for Rust code, API design, module organization, error handling, and Rust lint or style policy."
---

# Idiomatic Rust

Use [Google's Rust Style Guide](https://google.github.io/styleguide/rustguide.html)
as the canonical style baseline. Read the current guide when applying this
skill; follow its links to the Rust Style Guide and Rust API Guidelines where
relevant. Link to upstream sections when explaining a style decision rather
than maintaining a parallel rulebook. If upstream is unavailable, say so;
do not present remembered guidance as freshly verified.

## Apply the guide in context

Read the repository's instructions, manifests, lint configuration, and affected
callers before proposing changes. Explicit project requirements govern local
constraints; surface conflicts with the guide instead of silently bypassing
checks or inventing exceptions.

Apply the guide's qualifications as carefully as its rules:

- Distinguish reusable library APIs from application implementation by their
  consumers and contracts, not by whether the code lives in `lib.rs`.
- Apply sections and exceptions tagged `[chrome]` or `[Fuchsia]` only to
  those platforms. The general rules beside them still apply, including the
  general import rules.
- Favor simplicity in application code. Generality, shared ownership, traits,
  macros, and crate boundaries need a concrete benefit, not speculative reuse.
- Distinguish expected absence, recoverable failure, and violated invariants.
  Do not turn an internal defect into a successful empty/default result merely
  to avoid a panic. A stricter project panic policy must still preserve that
  distinction.
- Apply API evolution guidance to the actual compatibility boundary. When all
  consumers can change together, do not add compatibility machinery merely
  because an item is `pub`.

## Refactor coherently

Trace changes through definitions, callers, tests, serialized contracts,
documentation, and tooling. Propose one consistent end state across those
surfaces rather than isolated fixes. Respect the project's compatibility
requirements; where breaking changes are allowed, update consumers together.

Separate mechanical style changes from changes to failure behavior or external
contracts so each can be verified. Use the project's formatter, lint, and test
commands. Enforce suitable rules with tooling, but do not mistake a clean lint
run for compliance with design guidance or correctness.
