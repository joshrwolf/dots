---
name: golang-cli
description: "Build and review Go command-line applications. Use for command trees with spf13/cobra, layered configuration with spf13/viper, flags and arguments, stdout/stderr and machine-readable output, exit codes, signal handling, version info, completions, and command tests."
license: MIT
---

# Go CLIs

When extending a CLI, read the current command tree and match its structure. Keep
using its existing libraries. For a new tool, use stdlib `flag` when there is one
command and only a few flags. Use Cobra for a command tree. Add Viper only when
flags, environment and config files need layered precedence.

## Process contract

- `main` executes the command, reports the returned error and sets the exit
  status. Commands return errors; they never call `os.Exit`, which skips defers
  and cleanup.
- stdout carries program output and stderr carries logs, progress and errors.
  Write through `cmd.OutOrStdout()` / `cmd.ErrOrStderr()` (or injected writers)
  so tests can capture both.
- Machine consumers need a stable format: offer `--output json` (or similar)
  when scripts will parse the output.
- Check whether the actual output writer is a terminal (`golang.org/x/term`)
  before adding color or progress, and honor `NO_COLOR`.
- Exit codes are part of the contract: 0 for success and nonzero for failure.
  If the CLI distinguishes usage errors, the convention is 2, and `main` needs a
  way to recognize them. Signal exits are conventionally 128+N.
- Cancel with `signal.NotifyContext` and pass that context down.
- Use the version that `-ldflags -X` injects, falling back to
  `debug.ReadBuildInfo` for `go install` builds.

## Cobra gotchas

- By default Cobra prints the error and the usage text for every error,
  including runtime failures from `RunE`. Set `SilenceUsage` and `SilenceErrors`
  on the root so `main` reports each error once. These only stop printing; they
  do not classify errors. Flag, argument and unknown-command errors arrive as
  plain errors. To give usage errors their own exit code or a help hint, mark
  them yourself. `SetFlagErrorFunc` sees only flag-parse errors, and custom
  `Args` validators can wrap their own. Built-in `Args` validators, required or
  grouped flags and unknown commands need separate classification if every usage
  error must get the same code.
- A child `PersistentPreRunE` replaces its parent's rather than chaining. Call
  shared initialization explicitly, or enable hook traversal deliberately.
- Build a fresh command tree (and Viper instance) for each test. Package-level
  commands accumulate flag state between `Execute` calls.
- `MarkFlagRequired` checks only that a flag was set on the command line. Values
  from config or env do not satisfy it. Validate effective values after loading.
- An explicitly requested config file that is missing is an error. Only a
  default search may silently find nothing.
- Prefix environment variables (`SetEnvPrefix("MYAPP")`) so `PORT` does not
  collide with other tools.

## References

- [cobra.md](references/cobra.md): command tree, Run hooks, args validators,
  flags, completions and command testing. Its own links cover each area in depth.
- [viper.md](references/viper.md): precedence, sources, env binding, flag binding,
  unmarshaling, hot reload and test isolation.
- [assets/examples/](assets/examples/) holds a Cobra + Viper example package
  (add a `go.mod` to build it):
  `main.go`, `root.go`, `serve.go`, `flags.go`, `args.go`, `config.go`,
  `output.go`, `exit_codes.go`, `signal.go`, `version.go`, `completion.go` and
  `cli_test.go`. Some files show alternative patterns; do not register two
  commands with the same name.
