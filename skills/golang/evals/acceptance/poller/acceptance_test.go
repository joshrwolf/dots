package poller_test

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"example.com/workbench/poller"
)

func TestAcceptanceAlreadyCanceled(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		var calls atomic.Int32
		err := poller.Run(ctx, time.Second, func(context.Context) error {
			calls.Add(1)
			return nil
		})
		synctest.Wait()
		if !errors.Is(err, context.Canceled) || calls.Load() != 0 {
			t.Errorf("Run(canceled) = %v, calls %d; want canceled, zero calls", err, calls.Load())
		}
	})
}

func TestAcceptanceInvalidInterval(t *testing.T) {
	for _, interval := range []time.Duration{0, -time.Second} {
		t.Run(interval.String(), func(t *testing.T) {
			defer func() {
				if recovered := recover(); recovered != nil {
					t.Errorf("Run(interval=%v) panicked: %v", interval, recovered)
				}
			}()
			calls := 0
			err := poller.Run(t.Context(), interval, func(context.Context) error {
				calls++
				return nil
			})
			if err == nil || errors.Is(err, context.Canceled) || calls != 0 {
				t.Errorf("Run(interval=%v) = %v, calls %d; want validation error, zero calls", interval, err, calls)
			}
		})
	}
}

func TestAcceptancePollError(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		want := errors.New("source unavailable")
		calls := 0
		err := poller.Run(t.Context(), time.Second, func(context.Context) error {
			calls++
			return want
		})
		if !errors.Is(err, want) || calls != 1 {
			t.Errorf("Run() = %v, calls %d; want %v, one call", err, calls, want)
		}
	})
}

func TestAcceptanceLifetime(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(t.Context())
		started := make(chan struct{})
		done := make(chan error, 1)
		var calls atomic.Int32
		var active atomic.Int32
		var overlap atomic.Bool
		go func() {
			done <- poller.Run(ctx, time.Second, func(ctx context.Context) error {
				if active.Add(1) != 1 {
					overlap.Store(true)
				}
				defer active.Add(-1)
				if calls.Add(1) == 1 {
					close(started)
				}
				select {
				case <-ctx.Done():
					// A short cooperative shutdown makes an early Run return observable.
					time.Sleep(time.Millisecond)
					return ctx.Err()
				case <-time.After(2 * time.Second):
					return nil
				}
			})
		}()
		<-started
		time.Sleep(3 * time.Second)
		synctest.Wait()
		cancel()
		err := <-done
		if !errors.Is(err, context.Canceled) {
			t.Errorf("Run() = %v, want canceled", err)
		}
		if active.Load() != 0 || overlap.Load() {
			t.Errorf("Run returned with active=%d, overlap=%v", active.Load(), overlap.Load())
		}
		before := calls.Load()
		time.Sleep(3 * time.Second)
		synctest.Wait()
		if calls.Load() != before {
			t.Errorf("poll calls after return: before=%d after=%d", before, calls.Load())
		}
	})
}
