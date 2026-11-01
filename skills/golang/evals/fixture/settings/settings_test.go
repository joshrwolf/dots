package settings

import "testing"

func TestNew(t *testing.T) {
	cfg := New(12, map[string]string{"team": "blue"})
	if cfg.Enabled == nil || !*cfg.Enabled || cfg.Limit != 12 {
		t.Fatalf("New() = %+v, want enabled with limit 12", cfg)
	}
	if cfg.Labels["team"] != "blue" {
		t.Errorf("New().Labels = %v, want team:blue", cfg.Labels)
	}
}

func BenchmarkNew(b *testing.B) {
	for i := 0; i < b.N; i++ {
		cfg := New(12, nil)
		if cfg.Enabled == nil {
			b.Fatal("New().Enabled is nil")
		}
	}
}
