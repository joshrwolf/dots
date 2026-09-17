// cmd/myapp/version.go
package main

import (
	"fmt"
	"runtime/debug"

	"github.com/spf13/cobra"
)

// Set via ldflags
var (
	version = "dev"
	commit  = "unknown"
	date    = "unknown"
)

func newVersionCommand() *cobra.Command {
	return &cobra.Command{
		Use:   "version",
		Short: "Print version information",
		// Version should remain available even with a malformed config file.
		PersistentPreRunE: func(cmd *cobra.Command, args []string) error { return nil },
		RunE: func(cmd *cobra.Command, args []string) error {
			if _, err := fmt.Fprintf(cmd.OutOrStdout(), "myapp %s (commit: %s, built: %s)\n", version, commit, date); err != nil {
				return err
			}

			if info, ok := debug.ReadBuildInfo(); ok {
				_, err := fmt.Fprintf(cmd.OutOrStdout(), "go: %s\n", info.GoVersion)
				return err
			}
			return nil
		},
	}
}

// Build with:
//
//   go build -ldflags "-X main.version=1.2.3 \
//     -X main.commit=$(git rev-parse --short HEAD) \
//     -X main.date=$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
//     -o bin/myapp ./cmd/myapp
