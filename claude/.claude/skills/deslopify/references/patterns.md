# slop patterns — catalog, examples, detectors

Load this once at the start of a run. Grep finds suspects; reading in context
convicts. A pattern match is a *candidate*, never an automatic delete.

---

## Comment slop

The discriminator: a comment must describe permanent **state** and earn its place
with a non-obvious invariant, constraint, gotcha, bug workaround, or genuine
surprise. Everything below fails that test.

### Restatement — comment paraphrases the code

```go
// increment the counter
counter++

// loop over each user and send an email
for _, u := range users { send(u) }
```

→ delete both. The code says it.

### Narration — the model thinking out loud

```go
// First, validate the input
// Now we build the request
// Here we handle the error case
```

→ delete. These describe the act of writing, not the code.

### History / journal — orients to a past state

```go
// we now use a buffered channel instead of the old mutex
// previously this returned an error; no longer needed
// refactored to extract this helper
// removed the legacy fallback as part of the v2 migration
```

→ delete, or replace with the *reason the current code is the way it is*:
`// buffered so producers never block on a slow consumer`.

### Pointers out of the code

```go
// implements the retry strategy from the reliability RFC (docs/adr/0007)
// see PR #1423 for context
// TODO(JIRA-456): clean this up
```

→ put the fact in the comment (`// retries 5xx with jittered backoff, gives up
after 30s`) or delete. References rot when the doc moves.

### Padded doc comments

```go
// GetUser gets a user.
// This function takes an id and returns a user.
func GetUser(id string) (*User, error)
```

→ trim to what a caller needs, or cut:
`// GetUser returns the user, or ErrNotFound if no row matches id.`

---

## grep detectors

First-pass surfacing only. Tune paths/extensions to the scope.

```bash
# narration comments
rg -n -i '//\s*(now|here|first|then|next|let'\''s|we\s)' --type-add 'src:*.{go,ts,tsx,js,py,rs}' -tsrc

# history / journal comments
rg -n -i '\b(previously|no longer|used to|we now|refactored|renamed to|as part of|legacy|the old|removed the)\b' -g '!*.md'

# pointers out of the code
rg -n -i '(see (pr|ticket|issue|plan|adr|rfc)|#[0-9]{2,}|TODO\(|FIXME\()'

# type escape hatches
rg -n '\bas any\b|@ts-ignore|@ts-nocheck|#\s*type:\s*ignore|#\s*noqa(?!:)'

# emoji (any non-ASCII in source)
rg -n '[^\x00-\x7F]' --type-add 'src:*.{go,ts,tsx,js,py,rs}' -tsrc

# bare restatement candidates: a comment immediately above one line of code
# (no good regex — eyeball comment lines whose words match the next identifier)
```

---

## Structural slop

### Defensive over-engineering

Guards/try-catch on inputs that arrive already-validated from a trusted caller, or
that are abnormal for the surrounding file. The tell: the rest of the file trusts
its inputs and this one block doesn't.

```ts
function area(r: Rect) {
  if (!r) throw new Error("r is required");        // caller always passes a Rect
  if (typeof r.w !== "number") throw new Error();  // types already guarantee it
  return r.w * r.h;
}
```

→ `return r.w * r.h`.

### Type escape hatches

`as any`, `@ts-ignore`, `# type: ignore`, `# noqa` added to silence the checker
instead of fixing the type. → fix the type, or justify with a real comment.

### Needless abstraction

Single-use helper, a wrapper that only forwards args, a new util duplicating an
existing one.

```ts
function getUserName(u: User): string { return u.name; }  // called once
```

→ inline `u.name`. Duplication is cheaper than the wrong abstraction.

### Back-compat cruft

```ts
export { handleRequest as processRequest };  // no caller uses the alias
export const VERSION_2 = VERSION;            // shim nobody needs
```

→ remove, fix any call sites to the canonical name.

### Inline imports

```ts
async function load() {
  const { parse } = await import("./parser");  // not lazy by necessity
}
```

→ hoist to a top-level import unless lazy-loading is genuinely required.

### Magic numbers / over-verbose names

```ts
setTimeout(fn, 10000);
const theListOfAllActiveUserAccounts: User[] = users;
```

→ `const VISIBILITY_TIMEOUT_MS = 10_000`; `const active: User[]` (type carries it).

---

## Prose slop

For READMEs, PR bodies, docs, and comment text. State the thing plainly; cut the
performance.

- **Negative parallelism (#1 tell)** — "It's not X, it's Y", "not because X but
  because Y", "X — not Y". → state Y.
- **Negative listing / countdown** — "Not a tool. Not a framework. A platform."
  "Not ten. Not fifty. Five hundred." → cut the windup.
- **Rule of three / tricolon padding** — "fast, simple, and reliable" where one
  adjective would do.
- **Self-posed rhetorical questions** — "The result? Faster builds." → "Builds are
  faster."
- **Filler transitions** — "It's worth noting", "Importantly", "Notably",
  "Moreover", "Furthermore", "Additionally". → delete.
- **Superficial participles** — trailing "-ing" clause adding nothing:
  "…, highlighting its importance", "…, underscoring the need". → delete.
- **False ranges** — "from startups to enterprises", "from X to Y" with no real
  spectrum. → name the actual thing.
- **False agency** — inanimate subject doing a human verb: "the data tells us",
  "the config decides". → name the actor.
- **Manufactured drama** — "Here's the thing", "Think of it as", "Imagine a world
  where", stakes inflation. → cut.
- **Vocabulary tells** — delve, tapestry, testament, pivotal, intricate,
  meticulous, bolster, underscore, seamless, leverage (verb), utilize, foster,
  robust (outside engineering). → plain words.
- **Copula avoidance** — "serves as / stands as / represents" instead of "is".
- **Formatting tells** — em-dash spam (humans use a few, AI uses dozens), `→`
  unicode arrows, bold-lead bullets, emoji bullets, smart/curly quotes inserted
  into plain text, "🧵 Thread:" openers.
