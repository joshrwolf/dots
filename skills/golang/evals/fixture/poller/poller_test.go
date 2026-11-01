package poller

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestRunCanceled(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	err := Run(ctx, time.Hour, func(context.Context) error { return nil })
	if !errors.Is(err, context.Canceled) {
		t.Errorf("Run() = %v, want context.Canceled", err)
	}
}
