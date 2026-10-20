# Panic tracebacks

Get full stack traces on panic:

```bash
GOTRACEBACK=all ./app
```

| Level    | Shows                                 |
| -------- | ------------------------------------- |
| `none`   | No stack traces                       |
| `single` | Current goroutine only (default)      |
| `all`    | All user goroutines at an unrecovered panic |
| `system` | All goroutines + runtime frames       |

`GOTRACEBACK` controls failure output; it does not capture a currently hung process.
For a live hang, collect a goroutine stack dump using existing diagnostic access
and inspect the blocked operations and their owners.

```sh
curl 'http://localhost:6060/debug/pprof/goroutine?debug=2'
```

The example assumes an already configured local pprof endpoint. For programmatic
stacks, `runtime.Stack(buf, true)` includes all goroutines; allow for truncation when
sizing the buffer. Treat captured stacks as potentially sensitive diagnostic data.
