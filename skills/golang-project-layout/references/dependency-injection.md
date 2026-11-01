# Dependency wiring

Read the existing dependency graph and lifecycle needs before changing how a
program is wired.

## Defaults

- Make dependencies explicit. Long-lived services take them in constructors;
  simpler code can take function parameters or a small configuration value.
- Wire the application by hand in `main` (or one wiring function) while that stays
  readable. A count of services alone does not justify a container. See
  [manual-di.md](manual-di.md) for a complete example with shutdown ordering.
- Constructors accept the concrete types they need. A dependency becomes a
  consumer-owned interface when a real seam exists: an alternative implementation,
  a slow or external system to fake in tests, or an import cycle.
- Do not use package-level service lookup, `init` service setup, or a container
  passed around as a service locator. Business code receives resolved
  dependencies so its requirements stay visible.
- Choose lifetime by ownership and concurrency safety. Share pools and other
  concurrency-safe clients; isolate per-request state.
- Initialize required dependencies before reporting ready. Lazy initialization
  suits optional, expensive work.
- Close dependencies in reverse order, after their users stop.

## Containers

Adopt a DI library only when repeated wiring or lifecycle coordination has become
a concrete cost that it removes, and weigh its runtime failure modes: missing
registrations surface at startup or first use rather than at compile time. Keep
container resolution at the application root or in explicit scope factories. In an
existing samber/do project, follow [samber-do.md](samber-do.md). In tests, use a
fresh container per test; `RootScope.Clone` copies registrations, not the state
held in eagerly registered values.
