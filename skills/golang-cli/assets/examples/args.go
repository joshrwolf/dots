package main

import (
	"fmt"

	"github.com/spf13/cobra"
)

// Cobra provides built-in validators for positional arguments; see the skill's
// references/cobra/commands-and-args.md for the full set.
var deployCmd = &cobra.Command{
	Use:   "deploy [environment]",
	Short: "Deploy to an environment",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		env := args[0]
		_ = env
		// deploy...
		return nil
	},
}

// Custom validation example. Wrapping marks these as usage errors (exit 2).
var deployWithValidationCmd = &cobra.Command{
	Use:   "deploy [environment]",
	Short: "Deploy to an environment",
	Args: func(cmd *cobra.Command, args []string) error {
		if len(args) != 1 {
			return &ExitError{Code: exitUsage, Err: fmt.Errorf("expected exactly 1 argument, got %d", len(args))}
		}
		valid := map[string]bool{"dev": true, "staging": true, "prod": true}
		if !valid[args[0]] {
			return &ExitError{Code: exitUsage, Err: fmt.Errorf("invalid environment %q, must be one of: dev, staging, prod", args[0])}
		}
		return nil
	},
	RunE: func(cmd *cobra.Command, args []string) error {
		// deploy...
		return nil
	},
}
