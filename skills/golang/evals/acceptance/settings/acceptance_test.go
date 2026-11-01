package settings_test

import (
	"testing"

	"example.com/workbench/settings"
)

func TestAcceptanceSettings(t *testing.T) {
	for _, tt := range []struct{ input, want int }{
		{input: -10, want: 1}, {input: 0, want: 1}, {input: 1, want: 1},
		{input: 42, want: 42}, {input: 100, want: 100}, {input: 200, want: 100},
	} {
		got := settings.New(tt.input, nil)
		if got.Limit != tt.want || got.Enabled == nil || !*got.Enabled || got.Labels != nil {
			t.Errorf("New(%d,nil) = %+v, want limit %d, enabled, nil labels", tt.input, got, tt.want)
		}
	}
	labels := map[string]string{"team": "blue"}
	a, b := settings.New(10, labels), settings.New(10, labels)
	labels["team"] = "red"
	a.Labels["team"] = "green"
	*a.Enabled = false
	if b.Labels["team"] != "blue" || !*b.Enabled || labels["team"] != "red" {
		t.Errorf("independent ownership violated: input=%v a=%+v b=%+v", labels, a, b)
	}
	if got := settings.New(10, map[string]string{}); got.Labels == nil {
		t.Error("New(empty nonnil labels) returned nil labels")
	}
}
