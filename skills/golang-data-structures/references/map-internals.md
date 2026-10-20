# Map Internals Deep Dive

## Hash Table Structure

Go 1.24 introduced Swiss Tables for built-in maps, replacing the older `hmap` bucket/overflow-chain implementation. Swiss Tables use groups of slots and control bytes to narrow key comparisons. These layouts and growth policies are runtime details, not language contracts.

## Memory Growth and Capacity

- `make(map[K]V, n)` supplies an initial size hint, not a fixed capacity or limit.
- `cap` is not defined for maps.
- Deleting entries releases references held by those entries, but does not promise to shrink the live map's storage or immediately reduce process RSS.
- To compact a map after a large spike, copy surviving entries into a new map and release all references to the old one. Synchronize the replacement if concurrent callers can access it. RSS reclamation still depends on the allocator and scavenger.

See [Faster Go maps with Swiss Tables](https://go.dev/blog/swisstable).

## Preallocation

```go
// Without preallocation — multiple growths as entries are added
m := map[string]int{}

// With a size hint — lets the runtime reserve initial storage
m := make(map[string]int, expectedSize)
```

Preallocation can avoid intermediate growth when the estimate is useful. Do not compute storage from an old runtime bucket formula.

## Pointers vs Values

For large value types, storing pointers reduces copy overhead:

```go
// Large struct — copied on every read/write
m := map[string]BigStruct{}  // copies large struct

// Pointer — only pointer is copied
m := map[string]*BigStruct{} // copies 8-byte pointer
```

Trade-off: pointers introduce sharing, indirection, and potentially additional allocations and GC work. Measure realistic access patterns rather than using a universal size threshold.

## `maps` Package (Go 1.21+)

| Function | Description |
| --- | --- |
| `Clone`, `Equal`, `EqualFunc` | Shallow copy and equality comparison |
| `Keys`, `Values`, `All` (1.23+) | Iterators over keys, values, or pairs |
| `Collect`, `Insert` (1.23+) | Build maps from iterators or insert entries |

See `samber/cc-skills-golang@golang-safety` skill for `Clone`, `Equal`, and sorted iteration patterns.

## Map Key Requirements

Map keys must be comparable (`==` must work). This includes:

- All numeric types, `string`, `bool`
- Pointers and channels (compared by identity)
- Interfaces, provided their dynamic values are comparable; interface equality compares dynamic type and value, and can panic for a slice/map/function value
- Arrays of comparable types
- Structs where all fields are comparable

Slices, maps, and functions **cannot** be map keys.
