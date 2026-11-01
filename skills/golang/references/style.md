# Style and naming

Project formatters and linters own mechanical formatting. This page covers choices
they leave to judgment. Local conventions win.

## Declarations

- `var x T` signals a meaningful zero value; use `:=` for non-zero initialization.
- Composite literals of other packages' structs use field names.
- Use nil slices when the zero value fits. Allocate a map before writing. Give
  `make` a capacity when the size is known; do not preallocate speculatively.

## Control flow

- Handle errors and edge cases first and return early; keep the happy path flat.
- Drop `else` after `return`, `break` or `continue`. For a default with overrides,
  assign the default then use `switch`.
- Prefer `switch` when comparing one value repeatedly.
- Use a table of records when cases differ only in values; use explicit control
  flow when their behavior differs.
- Name a condition only when the name adds domain meaning. Hoisting a call out of
  a `||` chain changes short-circuit evaluation and can add work or a panic.
- Scope single-use values to the `if` (`if err := f(); err != nil`).

## Functions

- Order parameters as `ctx`, inputs, then output destinations.
- Group parameters into a struct when they form a concept, not to meet a count.
- Pass small values by value. Use pointers for mutation, meaningful nil, or a
  measured copy cost; there is no portable size threshold.
- Naked returns only in very short functions.
- Use `strconv` for simple conversions, `%q` for strings in messages, and
  `strings.Builder` in loops.
- Keep blank imports in `main` or tests; avoid dot imports outside tests.

## Names

| Element | Convention |
| --- | --- |
| Identifiers | MixedCaps; underscores only in test names, generated code, cgo |
| Acronyms | Consistent case: `URL`, `HTTPServer`, `xmlParser` |
| Packages | Short, lowercase, purpose-specific; never `util`, `common`, `helpers` |
| Constructors | `New` for a package's primary type, `NewThing` when there are several |
| Getters | `Name()`, not `GetName()` |
| Receivers | Short, consistent across methods; never `this` or `self` |
| Errors | `ErrNotFound` variables, `PathError` types |
| Options | `WithTimeout`; `Must` prefix for panicking variants; `f` suffix for format functions |
| Import aliases | Only on collision (`mrand "math/rand"`) |

- Do not stutter: `http.Client`, `user.New`, `dbpool.Status` rather than
  `http.HTTPClient`, `user.NewUser`, `dbpool.PoolStatus`.
- Match name length to scope: `i` in a short loop, descriptive names at package level.
- Name by role, not value or representation: `DefaultPort`, `users`, not
  `Port8080`, `userSlice`.
- Use one name per concept across a codebase.
- Choose enum zero values deliberately. Use an `Unknown` zero only when omission
  must be distinguishable; never renumber an established serialized enum as cleanup.
- Subtests take descriptive phrases; casing is free.

## Renames

Rename with gopls (`golang-refactoring`), then check struct tags, templates,
reflection, serialized names and external consumers that the compiler cannot see.
