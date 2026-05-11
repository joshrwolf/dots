# Concurrency

**Philosophy**: Share memory by communicating, don't communicate by sharing memory.

## Goroutines

Launch with `go`:
```go
go func() {
    // Runs concurrently
}()
```

**Always ensure goroutines can exit** to prevent leaks:
```go
func worker(ctx context.Context) {
    for {
        select {
        case <-ctx.Done():
            return  // Exit on cancellation
        case <-time.After(time.Second):
            doWork()
        }
    }
}
```

## Channels

### Unbuffered
Synchronous - blocks until both sender and receiver are ready:
```go
ch := make(chan int)
go func() { ch <- 42 }()  // Blocks until received
value := <-ch              // Blocks until sent
```

### Buffered
Asynchronous up to capacity:
```go
ch := make(chan int, 100)
ch <- 1  // Doesn't block until buffer full
```

### Closing
Only sender closes:
```go
ch := make(chan int)
go func() {
    for i := 0; i < 10; i++ {
        ch <- i
    }
    close(ch)  // Signal no more values
}()

for v := range ch {  // Reads until closed
    fmt.Println(v)
}
```

## Context

Always pass as first parameter:
```go
func ProcessData(ctx context.Context, data []byte) error
```

### Cancellation
```go
ctx, cancel := context.WithCancel(context.Background())
defer cancel()

go worker(ctx)
time.Sleep(time.Second)
cancel()  // Stop worker
```

### Timeout
```go
ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
defer cancel()

result, err := doWork(ctx)
```

### Check Cancellation
```go
select {
case <-ctx.Done():
    return ctx.Err()
case result := <-processData():
    return result
}
```

## Common Patterns

### errgroup (golang.org/x/sync/errgroup)

Run multiple goroutines with error handling:
```go
import "golang.org/x/sync/errgroup"

func processBatch(ctx context.Context, items []Item) error {
    g, ctx := errgroup.WithContext(ctx)

    for _, item := range items {
        g.Go(func() error {
            return processItem(ctx, item)
        })
    }

    return g.Wait()  // Returns first error, cancels context for others
}
```

Limit concurrency:
```go
g, ctx := errgroup.WithContext(ctx)
g.SetLimit(10)  // Max 10 concurrent goroutines

for _, item := range items {
    g.Go(func() error {
        return processItem(ctx, item)
    })
}

return g.Wait()
```

Collect results:
```go
type Result struct {
    ID    int
    Value string
}

func fetchAll(ctx context.Context, ids []int) ([]Result, error) {
    g, ctx := errgroup.WithContext(ctx)
    results := make([]Result, len(ids))

    for i, id := range ids {
        g.Go(func() error {
            val, err := fetch(ctx, id)
            if err != nil {
                return err
            }
            results[i] = Result{ID: id, Value: val}
            return nil
        })
    }

    if err := g.Wait(); err != nil {
        return nil, err
    }
    return results, nil
}
```

### Worker Pool
```go
func workerPool(ctx context.Context, jobs <-chan Job, results chan<- Result, n int) {
    var wg sync.WaitGroup
    for i := 0; i < n; i++ {
        wg.Add(1)
        go func() {
            defer wg.Done()
            for job := range jobs {
                select {
                case <-ctx.Done():
                    return
                case results <- processJob(job):
                }
            }
        }()
    }
    wg.Wait()
    close(results)
}
```

### Fan-Out, Fan-In
```go
func fanOut(in <-chan int, n int) []<-chan int {
    outs := make([]<-chan int, n)
    for i := range n {
        outs[i] = worker(in)
    }
    return outs
}

func fanIn(channels ...<-chan int) <-chan int {
    out := make(chan int)
    var wg sync.WaitGroup
    for _, ch := range channels {
        wg.Go(func() {
            for v := range ch {
                out <- v
            }
        })
    }
    go func() {
        wg.Wait()
        close(out)
    }()
    return out
}
```

### Pipeline
```go
func generate(nums ...int) <-chan int {
    out := make(chan int)
    go func() {
        defer close(out)
        for _, n := range nums {
            out <- n
        }
    }()
    return out
}

func square(in <-chan int) <-chan int {
    out := make(chan int)
    go func() {
        defer close(out)
        for n := range in {
            out <- n * n
        }
    }()
    return out
}

// Usage: for n := range square(generate(1, 2, 3))
```

## Synchronization

### sync.Mutex
```go
var (
    mu    sync.Mutex
    count int
)

mu.Lock()
count++
mu.Unlock()
```

### sync.RWMutex
Multiple readers OR single writer:
```go
var (
    mu    sync.RWMutex
    data  map[string]int
)

// Read
mu.RLock()
v := data["key"]
mu.RUnlock()

// Write
mu.Lock()
data["key"] = 42
mu.Unlock()
```

### sync.WaitGroup
Use `wg.Go` (Go 1.25) to launch tracked goroutines:
```go
var wg sync.WaitGroup
for i := range 5 {
    wg.Go(func() {
        doWork(i)
    })
}
wg.Wait()
```

### sync.Once
Run exactly once:
```go
var (
    once sync.Once
    db   *Database
)

func getDB() *Database {
    once.Do(func() {
        db = connectDB()
    })
    return db
}
```

### sync.Map
Concurrent map (for specific use cases):
```go
var m sync.Map

m.Store("key", "value")
v, ok := m.Load("key")
m.Delete("key")
```

## Race Detection

Always test with race detector:
```bash
go test -race ./...
go run -race main.go
```
