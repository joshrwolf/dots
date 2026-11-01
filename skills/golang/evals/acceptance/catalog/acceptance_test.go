package catalog_test

import (
	"slices"
	"testing"

	"example.com/workbench/catalog"
)

func TestAcceptanceFindByTags(t *testing.T) {
	c := catalog.New([]catalog.Item{
		{Name: "zinc", Tags: []string{"metal", "blue"}},
		{Name: "amber", Tags: []string{"blue"}},
		{Name: "copper", Tags: []string{"metal", "red"}},
		{Name: "tin", Tags: []string{"metal", "metal"}},
	})
	for _, tt := range []struct {
		name string
		tags []string
		want []string
	}{
		{name: "all", want: []string{"amber", "copper", "tin", "zinc"}},
		{name: "empty", tags: []string{}, want: []string{"amber", "copper", "tin", "zinc"}},
		{name: "intersection", tags: []string{"blue", "metal"}, want: []string{"zinc"}},
		{name: "duplicate", tags: []string{"metal", "metal"}, want: []string{"copper", "tin", "zinc"}},
		{name: "stored duplicate is not intersection", tags: []string{"metal", "red"}, want: []string{"copper"}},
		{name: "case", tags: []string{"Metal"}},
		{name: "absent", tags: []string{"green"}},
	} {
		t.Run(tt.name, func(t *testing.T) {
			got := c.FindByTags(tt.tags)
			var names []string
			for _, item := range got {
				names = append(names, item.Name)
			}
			if !slices.Equal(names, tt.want) {
				t.Errorf("FindByTags(%v) names = %v, want %v", tt.tags, names, tt.want)
			}
			if tt.want == nil && got != nil {
				t.Errorf("FindByTags(%v) = %#v, want nil", tt.tags, got)
			}
		})
	}
	if got := catalog.New(nil).FindByTags(nil); got != nil {
		t.Errorf("empty catalog returned %#v, want nil", got)
	}
	got := c.FindByTags([]string{"metal"})
	got[0].Tags[0] = "changed"
	got[0].Name = "changed"
	again := c.FindByTags([]string{"metal"})
	if len(again) != 3 || again[0].Name != "copper" || again[0].Tags[0] != "metal" {
		t.Errorf("caller mutation changed catalog: %#v", again)
	}
}
