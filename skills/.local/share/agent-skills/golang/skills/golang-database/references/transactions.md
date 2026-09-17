# Transactions, Isolation Levels, and Locking

## Basic transaction pattern

```go
tx, err := db.BeginTxx(ctx, nil) // database/configuration default; PostgreSQL normally READ COMMITTED
if err != nil {
    return fmt.Errorf("beginning transaction: %w", err)
}
defer tx.Rollback() // after Commit this returns sql.ErrTxDone, which can be ignored here

// ... execute queries using tx ...

if err := tx.Commit(); err != nil {
    return fmt.Errorf("committing transaction: %w", err)
}
```

## Custom isolation level

```go
tx, err := db.BeginTxx(ctx, &sql.TxOptions{
    Isolation: sql.LevelSerializable, // strongest guarantee
})
```

| Level | Use when |
| --- | --- |
| `LevelReadCommitted` | PostgreSQL default; each statement sees a committed snapshot |
| `LevelRepeatableRead` | Need consistent reads within a transaction |
| `LevelSerializable` | Invariants requiring equivalence to serial execution; retry serialization failures |

## SELECT FOR UPDATE — prevent race conditions

```go
var balance int
err := tx.GetContext(ctx, &balance, "SELECT balance FROM accounts WHERE id = $1 FOR UPDATE", accountID)
// Row is locked until tx.Commit() or tx.Rollback()
```

Use `FOR UPDATE` when you read a value, compute something from it, and then write it back. Atomic updates or optimistic conditional updates can also protect the invariant. Row locks do not lock missing rows, and ordinary SELECT does not wait on them. Acquire multiple locks in a consistent order and handle deadlocks; retry serialization failures by rerunning the entire transaction.

## Locking variants

| Clause | Effect |
| --- | --- |
| `FOR UPDATE` | Locks selected existing rows against conflicting writes/locks; ordinary SELECT is not blocked |
| `FOR UPDATE NOWAIT` | Same, but fails immediately instead of waiting |
| `FOR SHARE` | Locks rows for read — prevents writes but allows other reads |
