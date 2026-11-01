package poller

import (
	"context"
	"time"
)

// Run polls immediately, then every interval, until cancellation or a poll error.
func Run(ctx context.Context, interval time.Duration, poll func(context.Context) error) error {
	go func() {
		ticker := time.NewTicker(interval)
		_ = poll(ctx)
		for range ticker.C {
			_ = poll(ctx)
		}
	}()
	<-ctx.Done()
	return ctx.Err()
}
