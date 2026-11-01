package catalog

import (
	"fmt"
	"slices"
)

// Item is a named catalog entry.
type Item struct {
	Name string
	Tags []string
}

// Catalog owns its entries.
type Catalog struct {
	items map[string]Item
}

// New builds a catalog from entries. Later entries with the same name win.
func New(items []Item) *Catalog {
	c := &Catalog{items: make(map[string]Item, len(items))}
	for _, item := range items {
		item.Tags = slices.Clone(item.Tags)
		c.items[item.Name] = item
	}
	return c
}

// Get returns an entry by name with a caller-owned tag slice.
func (c *Catalog) Get(name string) (Item, error) {
	item, ok := c.items[name]
	if !ok {
		return Item{}, fmt.Errorf("item %q not found", name)
	}
	item.Tags = slices.Clone(item.Tags)
	return item, nil
}
