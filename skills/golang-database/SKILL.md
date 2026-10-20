---
name: golang-database
description: "Implement and review Go database access. Use for queries, scanning, transactions, connection pools, migrations, and database tests."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.3.2"
  openclaw:
    emoji: "🗄"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
    install: []
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent AskUserQuestion
paths:
  - "**/*.go"
---

Inspect existing query patterns and naming conventions before adding database code.

> **Community default.** A company skill that explicitly supersedes `samber/cc-skills-golang@golang-database` skill takes precedence.

# Go Database Best Practices

Go's `database/sql` provides a solid foundation for database access. `sqlx` extends it; `pgx` offers a native PostgreSQL API and an optional `database/sql` adapter. Prefer explicit SQL, while respecting a project's existing library and an explicitly requested ORM.

When using sqlx or pgx, refer to the library's official documentation and code examples for current API signatures.

## Best Practices Summary

1. **Prefer explicit SQL and established repository libraries** — inspect generated ORM queries when an ORM is used; honor explicitly requested ORM work
2. Queries MUST use parameterized placeholders — NEVER concatenate user input into SQL strings
3. Context MUST be passed to all database operations — use `*Context` method variants (`QueryContext`, `ExecContext`, `GetContext`)
4. `sql.ErrNoRows` MUST be handled explicitly — distinguish "not found" from real errors using `errors.Is`
5. Rows MUST be closed after iteration — `defer rows.Close()` immediately after `QueryContext` calls
6. NEVER use `db.Query` for statements that don't return rows — `Query` returns `*Rows` which must be closed; if you forget, the connection remains checked out of the pool. Use `db.Exec` instead
7. **Use transactions for multi-statement operations** — wrap related writes in `BeginTxx`/`Commit`
8. Protect read-modify-write invariants with atomic SQL updates, conditional updates, row locks, or suitable transaction isolation; choose based on the invariant
9. Check the database's configured default isolation (PostgreSQL defaults to READ COMMITTED); stronger isolation may require retrying whole transactions
10. **Handle NULLable columns** with pointer fields (`*string`, `*int`) or `sql.NullXxx` types
11. Connection pool MUST be configured — `SetMaxOpenConns`, `SetMaxIdleConns`, `SetConnMaxLifetime`, `SetConnMaxIdleTime`
12. **Use the repository's migration tooling** and version reviewed migration SQL; assess locks, backfills, compatibility and rollback/data-loss risks
13. **Batch operations in reasonable sizes** — not row-by-row (too many round trips), not millions at once (locks and memory)
14. **Design requested schema changes from actual constraints and access patterns** — inspect data volumes, indexes, locking and rollout requirements; do not apply destructive production DDL without authorization
15. **Make database-side behavior explicit** — document/test triggers, views, procedures and row-level security when they support the design

## Library Choice

| Library | Best for | Struct scanning | PostgreSQL-specific |
| --- | --- | --- | --- |
| `database/sql` | Portability, minimal deps | Manual `Scan` | No |
| `sqlx` | Multi-database projects | `StructScan` | No |
| `pgx` | PostgreSQL native APIs | `pgx.RowToStructByName` | Yes (COPY, LISTEN, arrays) |
| GORM/ent | Existing or requested ORM workflows | Library mapping | Inspect generated SQL and supported features |

**ORM tradeoffs to inspect:**

- Generated queries can obscure N+1 behavior; inspect query logs and plans
- Magic hooks and callbacks (BeforeCreate, AfterUpdate) make debugging harder
- Schema migrations coupled to application code
- ORM APIs add concepts and do not remove the need to understand SQL

## Parameterized Queries

```go
// ✗ VERY BAD — SQL injection vulnerability
query := fmt.Sprintf("SELECT * FROM users WHERE email = '%s'", email)

// ✓ Good — parameterized (PostgreSQL)
var user User
err := db.GetContext(ctx, &user, "SELECT id, name, email FROM users WHERE email = $1", email)

// ✓ Good — parameterized (MySQL)
err := db.GetContext(ctx, &user, "SELECT id, name, email FROM users WHERE email = ?", email)
```

### Dynamic IN clauses

```go
query, args, err := sqlx.In("SELECT * FROM users WHERE id IN (?)", ids)
if err != nil {
    return fmt.Errorf("building IN clause: %w", err)
}
query = db.Rebind(query) // adjust placeholders for your driver
err = db.SelectContext(ctx, &users, query, args...)
```

### Dynamic column names

Never interpolate column names from user input. Use an allowlist:

```go
allowed := map[string]bool{"name": true, "email": true, "created_at": true}
if !allowed[sortCol] {
    return fmt.Errorf("invalid sort column: %s", sortCol)
}
query := fmt.Sprintf("SELECT id, name, email FROM users ORDER BY %s", sortCol)
```

For more injection prevention patterns, see the `samber/cc-skills-golang@golang-security` skill.

## Struct Scanning and NULLable Columns

Use `db:"column_name"` tags for sqlx, `pgx.CollectRows` with `pgx.RowToStructByName` for pgx. Handle NULLable columns with pointer fields (`*string`, `*time.Time`) — they work cleanly with both scanning and JSON marshaling. See [Scanning Reference](./references/scanning.md) for examples of all approaches.

## Error Handling

```go
func GetUser(ctx context.Context, id string) (*User, error) {
    var user User

    err := db.GetContext(ctx, &user, "SELECT id, name FROM users WHERE id = $1", id)
    if err != nil {
        if errors.Is(err, sql.ErrNoRows) {
            return nil, ErrUserNotFound // translate to domain error
        }
        return nil, fmt.Errorf("querying user %s: %w", id, err)
    }

    return &user, nil
}
```

or:

```go
func GetUser(ctx context.Context, id string) (u *User, exists bool, err error) {
    var user User

    err := db.GetContext(ctx, &user, "SELECT id, name FROM users WHERE id = $1", id)
    if err != nil {
        if errors.Is(err, sql.ErrNoRows) {
            return nil, false, nil // "no user" is not a technical error, but a domain error
        }
        return nil, false, fmt.Errorf("querying user %s: %w", id, err)
    }

    return &user, true, nil
}
```

### Always close rows

```go
rows, err := db.QueryContext(ctx, "SELECT id, name FROM users")
if err != nil {
    return fmt.Errorf("querying users: %w", err)
}
defer rows.Close() // prevents connection leaks

for rows.Next() {
    // ...
}
if err := rows.Err(); err != nil { // always check after iteration
    return fmt.Errorf("iterating users: %w", err)
}
```

### Common database error patterns

| Error | How to detect | Action |
| --- | --- | --- |
| Row not found | `errors.Is(err, sql.ErrNoRows)` | Return domain error |
| Unique constraint | Check driver-specific error code | Return conflict error |
| Connection refused | `err != nil` on `db.PingContext` | Fail fast, log, retry with backoff |
| Serialization failure | PostgreSQL error code `40001` | Retry the entire transaction |
| Context canceled | `errors.Is(err, context.Canceled)` | Stop processing, propagate |

## Context Propagation

Always use the `*Context` method variants to propagate deadlines and cancellation:

```go
// ✗ Bad — no context, query runs until completion even if client disconnects
db.Query("SELECT ...")

// ✓ Good — respects context cancellation and timeouts
db.QueryContext(ctx, "SELECT ...")
```

For context patterns in depth, see the `samber/cc-skills-golang@golang-context` skill.

## Transactions, Isolation Levels, and Locking

For transaction patterns, isolation levels, `SELECT FOR UPDATE`, and locking variants, see [Transactions](./references/transactions.md).

## Connection Pool

```go
db.SetMaxOpenConns(25)              // limit total connections
db.SetMaxIdleConns(10)              // keep warm connections ready
db.SetConnMaxLifetime(5 * time.Minute)  // recycle stale connections
db.SetConnMaxIdleTime(1 * time.Minute)  // close idle connections faster
```

For sizing guidance and formulas, see [Database Performance](./references/performance.md).

## Migrations

Use an external migration tool. Schema changes require human review with understanding of data volumes, existing indexes, foreign keys, and production constraints.

Recommended tools:

- [golang-migrate](https://github.com/golang-migrate/migrate) — CLI + Go library, supports all major databases
- [Flyway](https://flywaydb.org/) — JVM-based, widely used in enterprise environments
- [Atlas](https://atlasgo.io/) — modern, declarative schema management

Write requested migration SQL, version it in source control and validate against representative data. Follow the project's review/deployment process; permission to author a migration is not permission to apply destructive production changes.

## Avoid Hidden SQL Features

Prefer explicit, testable behavior. Database features can enforce invariants across writers or centralize policy; keep their definitions versioned and account for operational effects. With row-level security, review privileged/bypass roles and avoid leaking tenant/session state across pooled connections; transaction-local settings can help scope that state.

## Schema Creation

For requested schema work, establish types, keys, nullability and constraints from domain invariants and query patterns. Assess representative data volumes, index/write tradeoffs and migration locking before proposing a rollout. State assumptions instead of refusing the task merely because it involves schema design.

## Deep Dives

- **[Transactions](./references/transactions.md)** — Transaction boundaries, isolation levels, deadlock prevention, `SELECT FOR UPDATE`
- **[Testing Database Code](./references/testing.md)** — Mock connections, integration tests with containers, fixtures, schema setup/teardown
- **[Database Performance](./references/performance.md)** — Connection pool sizing, batch processing, indexing strategy, query optimization
- **[Struct Scanning](./references/scanning.md)** — Struct tags, NULLable column handling, JSON marshaling patterns

## Cross-References

- → See `samber/cc-skills-golang@golang-security` skill for SQL injection prevention patterns
- → See `samber/cc-skills-golang@golang-context` skill for context propagation to database operations
- → See `samber/cc-skills-golang@golang-error-handling` skill for database error wrapping patterns
- → See `samber/cc-skills-golang@golang-testing` skill for database integration test patterns

## References

- [database/sql tutorial](https://go.dev/doc/database/)
- [sqlx](https://github.com/jmoiron/sqlx)
- [pgx](https://github.com/jackc/pgx)
- [golang-migrate](https://github.com/golang-migrate/migrate)
