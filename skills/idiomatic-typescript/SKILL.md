---
name: idiomatic-typescript
description: "Write, review, and refactor TypeScript using Google's TypeScript Style Guide as the baseline and Effective TypeScript for type and API design. Use for TypeScript code, module boundaries, error handling, and compiler or lint policy."
---

# Idiomatic TypeScript

Use [Google's TypeScript Style Guide](https://google.github.io/styleguide/tsguide.html)
as the style baseline and [Effective TypeScript](https://effectivetypescript.com/)
for type modeling and API design judgment. Read current upstream guidance for
the issue at hand; cite relevant sections rather than maintaining a parallel
rulebook. Distinguish material actually consulted from book content you have
not accessed. If a source is unavailable, say so.

## Apply the guidance in context

Read project instructions, compiler and lint configuration, package manifests,
runtime requirements, and affected callers first. Surface conflicts with the
baseline rather than silently bypassing checks.

Google's guide includes internal build-tool and runtime constraints. Verify their
applicability to the project's actual compiler, module system, host, and runtime;
do not import Google-only tooling or older-runtime restrictions by default.
Use current [TypeScript documentation](https://www.typescriptlang.org/docs/)
to verify language and compiler behavior and
[typescript-eslint documentation](https://typescript-eslint.io/)
when choosing automated enforcement.

Distinguish reusable public APIs from application internals by their actual
consumers, not by an export keyword. Preserve runtime validation at untrusted
boundaries: a static type or assertion does not validate external data.
Separate expected absence, operational failure, and broken invariants; do not
conceal defects behind successful defaults.

## Refactor coherently

Trace changes through definitions, consumers, tests, serialized contracts,
documentation, and tooling. Prefer a consistent end state to isolated fixes.
Respect real compatibility boundaries; when coordinated breaking changes are
allowed, update consumers together rather than adding speculative shims.

Keep mechanical formatting separate from semantic changes. Use the project's
documented checks and test failure and lifecycle behavior, not just happy paths.
Compiler strictness, type-aware linting, and formatting are complementary;
passing them does not establish design quality or runtime correctness.
