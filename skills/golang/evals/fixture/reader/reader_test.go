package reader

import (
	"strings"
	"testing"
)

func TestDecode(t *testing.T) {
	got, err := Decode(strings.NewReader(" hello "))
	if err != nil {
		t.Fatal(err)
	}
	if got != "hello" {
		t.Errorf("Decode() = %q, want hello", got)
	}
}
