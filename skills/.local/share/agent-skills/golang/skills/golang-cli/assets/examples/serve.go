// cmd/myapp/serve.go
package main

import (
	"fmt"

	"github.com/spf13/cobra"
	"github.com/spf13/viper"
)

// This minimal command demonstrates configuration and I/O. For an actual HTTP
// server lifecycle, see signal.go.
func newServeCommand(cfg *viper.Viper) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "serve",
		Short: "Show the configured server address",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			port := cfg.GetInt("port")
			if port < 1 || port > 65535 {
				return fmt.Errorf("port must be between 1 and 65535")
			}
			_, err := fmt.Fprintf(cmd.ErrOrStderr(), "configured address :%d\n", port)
			return err
		},
	}
	cmd.Flags().IntP("port", "p", 8080, "port to listen on")
	if err := cfg.BindPFlag("port", cmd.Flags().Lookup("port")); err != nil {
		panic(err)
	}
	return cmd
}

// For groups, both registration and GroupID assignment must precede Execute:
// root.AddGroup(&cobra.Group{ID: "management", Title: "Management Commands:"})
// cmd.GroupID = "management"
