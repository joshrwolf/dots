# Slice Internals

## Memory Layout

A slice header uses 3 machine words (24 bytes on 64-bit targets):

- **Pointer** — points to backing array (which may be stack- or heap-allocated)
- **Length** — number of elements in use
- **Capacity** — allocated size of backing array

Assigning or passing a slice copies the header, not the backing array. Both the original and copy point to the same underlying data—mutations are visible to both.

## Capacity Growth

When `append` exceeds capacity, the runtime allocates storage and copies the existing elements. Current implementations transition from roughly doubling small capacities toward smaller proportional growth for large slices. Bulk appends and allocator size classes affect the final capacity; no fixed growth factor is guaranteed.

### Growth Cost

A reallocation copies O(len(s)) elements. Repeated one-element appends have amortized O(1) cost under geometric growth, but there is no universal “2N copies” bound for the current growth policy. Useful preallocation avoids intermediate copies:

```go
// Known size — direct indexing
out := make([]Result, len(input))
for i, v := range input {
    out[i] = transform(v)
}

// Approximate size
out := make([]Result, 0, len(input)*2)
for _, v := range input {
    out = append(out, transform(v))
}
```

## `slices` Package (Go 1.21+)

| Category | Key Functions |
| --- | --- |
| **Sort** | `Sort`, `SortFunc`, `SortStableFunc`, `IsSorted` |
| **Search** | `BinarySearch`, `BinarySearchFunc`, `Contains`, `Index`, `IndexFunc` |
| **Mutate** | `Insert`, `Delete`, `DeleteFunc`, `Replace`, `Compact`, `Reverse`, `Grow`, `Clip` |
| **Create** | `Clone`, `Concat` (1.22+), `Repeat` (1.23+), `Chunk` (1.23+) |
| **Compare** | `Equal`, `EqualFunc`, `Compare`, `CompareFunc` |

## `copy()` vs `append()` vs `slices.Clone()`

| Operation             | Use When                         |
| --------------------- | -------------------------------- |
| `copy(dst, src)`      | Copying into pre-allocated slice |
| `append(dst, src...)` | Appending to a slice             |
| `slices.Clone(s)`     | Shallow copy; elements may still refer to shared data        |
| `s[:len(s):len(s)]`   | Preventing append aliasing       |
