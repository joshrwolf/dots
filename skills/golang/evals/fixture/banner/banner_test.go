package banner

import "testing"

func TestReady(t *testing.T) {
	if got := Ready(); got == "" {
		t.Fatal("Ready() is empty")
	}
}
