package banner_test

import (
	"testing"

	"example.com/workbench/banner"
)

func TestAcceptanceReady(t *testing.T) {
	if got := banner.Ready(); got != "Service is ready" {
		t.Errorf("Ready() = %q, want Service is ready", got)
	}
}
