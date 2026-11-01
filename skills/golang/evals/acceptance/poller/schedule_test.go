package poller_test

import (
	"context"
	"errors"
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"example.com/workbench/poller"
)

func TestAcceptanceScheduleAndIdleCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		start := time.Now()
		var calls []time.Duration
		done := make(chan error, 1)
		go func() {
			done <- poller.Run(ctx, time.Second, func(context.Context) error {
				calls = append(calls, time.Since(start))
				return nil
			})
		}()
		synctest.Wait()
		time.Sleep(3500 * time.Millisecond)
		synctest.Wait()
		want := []time.Duration{0, time.Second, 2 * time.Second, 3 * time.Second}
		if !slices.Equal(calls, want) {
			t.Errorf("poll times = %v, want %v", calls, want)
		}
		cancel()
		synctest.Wait()
		select {
		case err := <-done:
			if !errors.Is(err, context.Canceled) {
				t.Errorf("Run() = %v, want canceled", err)
			}
		default:
			t.Error("Run is still waiting after idle cancellation")
		}
	})
}
