# Domain-Driven Design (DDD) in Go

## Table of Contents

- [When to Use](#when-to-use)
- [Building Blocks](#building-blocks)
- [Project Structure](#project-structure)
- [Code Examples](#code-examples)
  - [Value Object — Money](#value-object--money)
  - [Aggregate Root — Order](#aggregate-root--order)
  - [Repository Interface — defined in domain](#repository-interface--defined-in-domain)
  - [Application Service — orchestrates a use case](#application-service--orchestrates-a-use-case)
- [Bounded Contexts](#bounded-contexts)
- [Wiring](#wiring)

## When to Use

Apply DDD when rich business rules and bounded contexts make domain modeling useful. Code size is not a threshold, and a CLI can still contain a complex domain. Simple CRUD applications often need fewer layers.

## Building Blocks

| Concept | Go Mapping | Purpose |
| --- | --- | --- |
| **Entity** | Struct with identity field | Has unique ID, mutable state, lifecycle |
| **Value Object** | Immutable struct, compared by value | No identity — represents a measurement, quantity, or descriptor |
| **Aggregate** | Entity + child entities/value objects | Consistency boundary — all mutations go through the root |
| **Repository** | Interface in domain, impl in infrastructure | Persistence abstraction for aggregates |
| **Domain Service** | Function or struct in domain package | Logic that spans multiple aggregates |
| **Domain Event** | Struct describing a fact that happened | Decouples bounded contexts |

## Project Structure

Organize by **bounded context**, grouping domain, application, and adapters vertically. This scales across multiple contexts and clarifies ownership.

```
order-service/
├── cmd/
│   └── server/
│       └── main.go                  # Wiring only
├── internal/
│   ├── order/                       # Bounded context: Order
│   │   ├── domain/
│   │   │   ├── order.go             # Order aggregate root
│   │   │   ├── item.go              # OrderItem entity
│   │   │   ├── status.go            # OrderStatus enum
│   │   │   ├── repository.go        # OrderRepository interface
│   │   │   └── events.go            # OrderPlaced, OrderShipped events
│   │   ├── application/
│   │   │   ├── place_order.go       # PlaceOrderHandler (command)
│   │   │   └── get_order.go         # GetOrderHandler (query)
│   │   └── adapters/
│   │       ├── persistence/
│   │       │   └── postgres.go      # OrderRepository implementation
│   │       └── http/
│   │           └── handler.go       # HTTP transport
│   ├── billing/                     # Bounded context: Billing (another example)
│   │   ├── domain/
│   │   ├── application/
│   │   └── adapters/
│   ├── shared/
│   │   └── money.go                 # Value object reused across contexts
│   └── events/
│       └── publisher.go             # Shared event bus (infrastructure)
├── go.mod
└── go.sum
```

**Key principles:**

- Group each bounded context **vertically** (domain → application → adapters), not by technical role
- Use `adapters/` instead of `infrastructure/` to be explicit about Hexagonal Architecture
- Make cross-context boundaries explicit (see **Bounded Contexts** section below)
- Place shared infrastructure (event bus, logging) at `internal/{shared}/` or `internal/events/`

## Code Examples

### Value Object — Money

```go
// internal/shared/money.go
package shared

type Money struct {
    amount   int64  // currency minor units; scale is a domain/currency decision
    currency string
}

func NewMoney(amount int64, currency string) (Money, error) {
    if currency == "" {
        return Money{}, errors.New("currency is required")
    }
    return Money{amount: amount, currency: currency}, nil
}

func (m Money) Add(other Money) (Money, error) {
    if m.currency != other.currency {
        return Money{}, fmt.Errorf("cannot add %s to %s", other.currency, m.currency)
    }
    if (other.amount > 0 && m.amount > math.MaxInt64-other.amount) ||
        (other.amount < 0 && m.amount < math.MinInt64-other.amount) {
        return Money{}, errors.New("money addition overflow")
    }
    return Money{amount: m.amount + other.amount, currency: m.currency}, nil
}
```

### Aggregate Root — Order

```go
// internal/order/domain/order.go
package domain

type Order struct {
    id     string
    items  []Item
    status Status
    total  shared.Money
}

func NewOrder(id, currency string) (*Order, error) {
    total, err := shared.NewMoney(0, currency)
    if err != nil { return nil, err }
    return &Order{id: id, status: StatusDraft, total: total}, nil
}

// All mutations go through the aggregate root
func (o *Order) AddItem(item Item) error {
    if o.status != StatusDraft {
        return ErrOrderNotEditable
    }
    total, err := o.total.Add(item.Total())
    if err != nil { return err } // do not mutate the aggregate on failure
    o.items = append(o.items, item)
    o.total = total
    return nil
}

func (o *Order) Place() (OrderPlaced, error) {
    if o.status != StatusDraft { return OrderPlaced{}, ErrOrderNotEditable }
    if len(o.items) == 0 {
        return OrderPlaced{}, ErrEmptyOrder
    }
    o.status = StatusPlaced
    return OrderPlaced{OrderID: o.id, Total: o.total}, nil
}
```

### Repository Interface — defined in domain

```go
// internal/order/domain/repository.go
package domain

type Repository interface {
    Save(ctx context.Context, order *Order) error
    FindByID(ctx context.Context, id string) (*Order, error)
}
```

The implementation lives in `internal/order/adapters/persistence/postgres.go` and depends on the domain — never the reverse.

### Application Service — orchestrates a use case

```go
// internal/order/application/place_order.go
package application

import (
    "context"
    "fmt"

    "myapp/internal/order/domain"
)

type PlaceOrderHandler struct {
    orders domain.Repository
    events EventPublisher
}

func (h *PlaceOrderHandler) Handle(ctx context.Context, cmd PlaceOrderCommand) error {
    order, err := h.orders.FindByID(ctx, cmd.OrderID)
    if err != nil {
        return fmt.Errorf("finding order: %w", err)
    }

    evt, err := order.Place()
    if err != nil {
        return fmt.Errorf("placing order: %w", err)
    }

    if err := h.orders.Save(ctx, order); err != nil {
        return fmt.Errorf("saving order: %w", err)
    }

    return h.events.Publish(ctx, evt)
}
```

Save followed by Publish has a failure window. If event delivery is required, persist an outbox entry with the order and publish it with retries; consumers should tolerate duplicates. Private fields restrict outside mutation but do not prevent zero values or mutation within the package.

## Bounded Contexts

Each bounded context maps to a top-level package under `internal/` with its own domain, application, and adapters. Contexts communicate through domain events or explicit anti-corruption layers — never by importing each other's internal types directly.

**Anti-corruption layer example:** If `billing/` needs to consume an `order.OrderPlaced` event, translate it to a billing-specific type:

```go
// internal/billing/adapters/events/order_events.go
package events

import (
    "myapp/internal/events"
    "myapp/internal/billing/domain"
)

type OrderPlacedSubscriber struct {
    invoices domain.InvoiceRepository
}

// Receives order.OrderPlaced, translates to billing domain
func (s *OrderPlacedSubscriber) OnOrderPlaced(evt events.OrderPlaced) error {
    // Translate and create invoice
    return s.invoices.Create(evt.OrderID, evt.Total)
}
```

This prevents billing from depending on order's internal types.

For large systems, each context can be its own Go module in a workspace (`go.work`). See the `samber/cc-skills-golang@golang-project-layout` skill for workspace setup.

## Wiring

Wire dependencies in `cmd/server/main.go` using manual constructor injection. → See `samber/cc-skills-golang@golang-dependency-injection` skill for DI library alternatives.
