package main

import "errors"

// Commands may wrap a failure in ExitError when the CLI contract needs a
// specific status. Other errors use 1; Cobra does not classify them for us.
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
	var exitErr *ExitError
	if errors.As(err, &exitErr) && exitErr.Code > 0 && exitErr.Code < 126 {
		return exitErr.Code
	}
	return 1
}
