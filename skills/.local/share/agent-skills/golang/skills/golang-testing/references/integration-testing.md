# Integration Testing

## Table of Contents

- [Docker Compose Fixture](#docker-compose-fixture)
- [SQL Schema Fixture](#sql-schema-fixture)
- [Test Data Fixture](#test-data-fixture)
- [Using Fixtures in Tests](#using-fixtures-in-tests)
- [Test Helper with Embedded Fixtures](#test-helper-with-embedded-fixtures)

## Docker Compose Fixture

Create `pkg/myfeature/testdata/docker-compose.yml` for test services:

```yaml
version: "3.8"
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: test
      POSTGRES_PASSWORD: test
      POSTGRES_DB: testdb
    ports:
      - "5433:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U test"]
      interval: 5s
      timeout: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports:
      - "6380:6379"
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 5s
      retries: 5
```

## SQL Schema Fixture

Create `pkg/myfeature/testdata/schema.sql` for database initialization:

```sql
CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS orders (
    id SERIAL PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    amount DECIMAL(10,2) NOT NULL,
    status VARCHAR(50) DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

## Test Data Fixture

Create `pkg/myfeature/testdata/testdata.sql`:

```sql
INSERT INTO users (name, email) VALUES
    ('Alice Johnson', 'alice@example.com'),
    ('Bob Smith', 'bob@example.com'),
    ('Charlie Brown', 'charlie@example.com');

INSERT INTO orders (user_id, amount, status) VALUES
    (1, 100.00, 'completed'),
    (1, 50.00, 'pending'),
    (2, 200.00, 'completed');
```

## Using Fixtures in Tests

Start the test services outside the test process, wait for readiness, and provide
`TEST_DATABASE_URL`. Give each test an isolated transaction; every query under
test must use that transaction for rollback to provide isolation. Tests that
commit transactions need their own database or schema instead.

```go
//go:build integration

package database_test

import (
	"context"
	"database/sql"
	"os"
	"testing"

	_ "github.com/lib/pq"
)

func fixtureTx(t *testing.T) *sql.Tx {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Fatal("TEST_DATABASE_URL must point to an isolated test database")
	}
	db, err := sql.Open("postgres", dsn)
	if err != nil {
		t.Fatalf("open database: %v", err)
	}
	t.Cleanup(func() {
		if err := db.Close(); err != nil {
			t.Errorf("close database: %v", err)
		}
	})
	tx, err := db.BeginTx(context.Background(), nil)
	if err != nil {
		t.Fatalf("begin fixture transaction: %v", err)
	}
	t.Cleanup(func() {
		if err := tx.Rollback(); err != nil {
			t.Errorf("rollback fixture transaction: %v", err)
		}
	})
	for _, path := range []string{"testdata/schema.sql", "testdata/testdata.sql"} {
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		if _, err := tx.Exec(string(data)); err != nil {
			t.Fatalf("load %s: %v", path, err)
		}
	}
	return tx
}

func TestUserCount(t *testing.T) {
	tx := fixtureTx(t)
	var count int
	if err := tx.QueryRow("SELECT COUNT(*) FROM users").Scan(&count); err != nil {
		t.Fatalf("count users: %v", err)
	}
	if count != 3 {
		t.Errorf("user count = %d, want 3", count)
	}
}
```

Cleanup registration order ensures rollback happens before the connection closes.
The fixtures assume an empty, disposable database; do not load them into a shared
or production database. Match the SQL driver to the application's existing stack.

## Test Helper with Embedded Fixtures

```go
package myfeature

import (
    "database/sql"
    "embed"
)

//go:embed testdata/schema.sql testdata/testdata.sql
var fixtures embed.FS

func SetupDB(db *sql.DB) error {
    schema, err := fixtures.ReadFile("testdata/schema.sql")
    if err != nil {
        return err
    }
    if _, err := db.Exec(string(schema)); err != nil {
        return err
    }

    data, err := fixtures.ReadFile("testdata/testdata.sql")
    if err != nil {
        return err
    }
    if _, err := db.Exec(string(data)); err != nil {
        return err
    }
    return nil
}
```
