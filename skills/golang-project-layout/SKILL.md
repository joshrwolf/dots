---
name: golang-project-layout
description: "Structure Go projects and application wiring. Use for new project setup, module paths, cmd/internal package boundaries, multi-module repositories and workspaces, dependency injection and service wiring (including samber/do), and architecture choices such as clean, hexagonal, or DDD."
license: MIT
---

# Go project layout and wiring

Follow the requested architecture and the repository's existing layout. For a new
project, start with the simplest structure that works. A small CLI needs no
layers and no DI framework. Ask only when missing requirements would materially
change the architecture or lifecycle design.

## Layout

- A small command can be a root `main` package. Use `cmd/<name>/` when separating
  several binaries is useful. Entry points parse, wire and run; substantial logic
  lives in testable packages.
- Use `internal/` for packages not meant for outside import. Public packages need
  no `pkg/` wrapper.
- Name packages for their purpose, not `util`, `common` or `models`. A package
  that exists only to hold types or interfaces for others usually belongs with
  its consumer.
- Within a package, extend the file that owns the behavior. Adding a type, helper
  or feature does not by itself justify a new file or package. Add one for a
  distinct responsibility or a technical constraint.
- Module paths resolve to their published source, including any `/vN` major
  version suffix. Keep the repository's real case and path.
- One `go.mod` usually suffices for a monorepo. Use `go.work` for local
  development across modules that are versioned separately.
- Services follow 12-factor conventions: config from the environment, logs to
  stdout, graceful shutdown, and one-off admin commands as their own binaries.
- Reuse the existing build and lint setup. [assets/Makefile](assets/Makefile) is
  a template to use on request, not a required scaffold.

## Wiring

Wire dependencies explicitly at the application root. Use constructors with
concrete parameters, plus consumer-owned interfaces where a real seam exists.
Adopt a container only when it removes a concrete wiring or lifecycle cost. Read
[dependency-injection.md](references/dependency-injection.md) before changing how
a program is wired.

## References

- [directory-layouts.md](references/directory-layouts.md): layout examples for
  commands, services and libraries, with common mistakes.
- [testing-layout.md](references/testing-layout.md): test packages, `testdata/`,
  shared test helpers.
- [workspaces.md](references/workspaces.md): setting up `go.work` across modules.
- [manual-di.md](references/manual-di.md): a complete hand-wired application with
  shutdown order.
- [samber-do.md](references/samber-do.md): only when the project uses samber/do.
- [architecture.md](references/architecture.md): choosing how much architecture a
  project needs, and for the chosen style [clean-architecture.md](references/clean-architecture.md),
  [hexagonal-architecture.md](references/hexagonal-architecture.md) or
  [ddd.md](references/ddd.md).

Moving existing code into a new layout is `golang-refactoring` work.
