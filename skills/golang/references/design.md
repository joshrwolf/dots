# Construction, configuration and data flow

## Constructors and configuration

- Required inputs are parameters. Use a configuration struct when optional settings
  form a coherent value. Functional options suit extensible public APIs with many
  optional settings; a growing field count alone does not require them. Builders
  are for genuinely staged construction.
- Validate the final configuration, including constraints across fields, before
  returning a usable value. With functional options, the constructor can report
  invalid combinations; options need not return errors.
- Document defaults. Preserve an established constructor API unless changing it
  is the task.

## `init` and globals

Avoid `init` and mutable package globals for anything that can fail or that tests
need to control: `init` cannot return errors and runs before tests. Construct
dependencies explicitly in `main` and pass them in. Package-level
`regexp.MustCompile`, `//go:embed` and immutable lookup tables are fine.

## Enums

A named integer type accepts any integer; validate at boundaries. Reserve zero for
`Unknown` when omission must be detectable; otherwise zero may be the useful
default. Do not renumber values that are persisted or serialized.

## Timeouts and limits

- Bound external calls with a deadline from the caller's context or an explicit
  timeout; the zero-value `http.Client` never times out.
- Bound pools, queues, buffers and concurrency. Unbounded growth fails under load.
- Retries check `ctx.Err()` between attempts and back off with `select` on
  `ctx.Done()`.

## Strings and bytes

Use `string` for immutable text and map keys, `[]byte` for mutation and I/O. Avoid
repeated conversions. A rune is a code point, not a user-perceived character, and
decoding invalid UTF-8 to runes is lossy.

## Iterators and streaming

For large results, return an `iter.Seq`/`iter.Seq2` (Go 1.23+) or stream to the
writer when bounded memory or incremental consumption matters. A
`Seq2[T, error]` iterator normally stops after yielding an error, unless its
contract permits per-item errors and continuation. It must respect `yield`
returning false. Once HTTP headers are committed, the status cannot change;
follow the streaming protocol's error contract or abort the response with
`panic(http.ErrAbortHandler)` when a truncated response is the chosen policy.
