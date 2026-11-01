# Manual Constructor Injection

Manual DI is the simplest approach — pass dependencies through constructors. No library, no magic.

## Complete Application Example

```go
func main() {
    ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
    defer stop()

    // Layer 1: Configuration
    cfg := LoadConfig()
    logger := slog.New(slog.NewJSONHandler(os.Stdout, nil))

    // Layer 2: Infrastructure
    db, err := postgres.Connect(cfg.DatabaseURL)
    if err != nil {
        logger.Error("database connection failed", "error", err)
        os.Exit(1)
    }
    defer db.Close()

    cache := redis.NewClient(cfg.RedisURL)
    defer cache.Close()

    mailer := smtp.NewMailer(cfg.SMTPAddr)

    // Layer 3: Repositories
    userRepo := postgres.NewUserRepository(db)
    orderRepo := postgres.NewOrderRepository(db)

    // Layer 4: Services
    userSvc := service.NewUserService(userRepo, cache, mailer, logger)
    orderSvc := service.NewOrderService(orderRepo, userSvc, logger)
    paymentSvc := service.NewPaymentService(orderRepo, cfg.StripeKey, logger)

    // Layer 5: Transport
    handler := http.NewHandler(userSvc, orderSvc, paymentSvc, logger)
    server := http.NewServer(cfg.Port, handler)

    // Run
    go server.ListenAndServe()
    <-ctx.Done()
    server.Shutdown(context.Background())
}
```

## When Manual DI Works Well

- Dependency relationships remain easy to follow
- Constructors expose the required resources and test seams
- The application can clearly own startup, cleanup, and any lazy initialization

## When a Container May Help

Repeated scope setup or lifecycle coordination can justify a container when it reduces real duplication. Service count, lazy initialization, or editing `main()` alone is not a reason to adopt one. Manual wiring supports these behaviors too; compare the explicit code with the container's registration rules and runtime failure modes.

Manual DI is a useful default while wiring remains clear. Initialize dependencies before their consumers and unwind successful initialization if a later step fails; the exact order follows the dependency graph.
