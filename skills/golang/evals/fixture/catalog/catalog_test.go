package catalog

import "testing"

func TestGet(t *testing.T) {
	c := New([]Item{{Name: "copper", Tags: []string{"metal"}}})
	got, err := c.Get("copper")
	if err != nil {
		t.Fatal(err)
	}
	if got.Name != "copper" {
		t.Errorf("Get().Name = %q, want copper", got.Name)
	}
}
