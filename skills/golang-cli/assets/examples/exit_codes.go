package main

import "errors"

const (
	exitFailure = 1
	exitUsage   = 2
)

// Commands wrap a failure in ExitError when the CLI contract needs a specific
// status. This example marks only flag-parse errors (root.go) and errors that a
// custom Args validator wraps itself (args.go) with exitUsage. Cobra returns
// built-in Args validator, required or grouped flag, and unknown-command errors
// as plain errors, so they exit with exitFailure here; a CLI that promises a
// distinct code for every usage error must classify those separately.
type ExitError struct {
	Code int
	Err  error
}

func (e *ExitError) Error() string { return e.Err.Error() }
func (e *ExitError) Unwrap() error { return e.Err }

func exitCode(err error) int {
	if err == nil {
		return 0
	}
	if exitErr, ok := errors.AsType[*ExitError](err); ok && exitErr.Code > 0 && exitErr.Code < 126 {
		return exitErr.Code
	}
	return exitFailure
}
