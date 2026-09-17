// cmd/myapp/root.go
package main

import (
	"fmt"
	"os"
	"strings"

	"github.com/spf13/cobra"
	"github.com/spf13/viper"
)

func newRootCommand() *cobra.Command {
	cfg := viper.New()
	var cfgFile string
	root := &cobra.Command{
		Use:   "myapp",
		Short: "Example application",
		PersistentPreRunE: func(cmd *cobra.Command, args []string) error {
			return initConfig(cfg, cfgFile)
		},
		SilenceUsage:  true,
		SilenceErrors: true, // main prints returned errors exactly once
	}
	root.PersistentFlags().StringVar(&cfgFile, "config", "", "config file (default $HOME/.myapp.yaml)")
	root.PersistentFlags().String("log-level", "info", "log level (debug, info, warn, error)")
	// These flags are registered directly above; a binding error is a programmer error.
	if err := cfg.BindPFlag("log-level", root.PersistentFlags().Lookup("log-level")); err != nil {
		panic(err)
	}
	root.AddCommand(newServeCommand(cfg), newVersionCommand(), newCompletionCommand())
	return root
}

func Execute() error { return newRootCommand().Execute() }

func initConfig(cfg *viper.Viper, cfgFile string) error {
	if cfgFile != "" {
		cfg.SetConfigFile(cfgFile)
	} else {
		home, err := os.UserHomeDir()
		if err != nil {
			return fmt.Errorf("finding home directory: %w", err)
		}
		cfg.AddConfigPath(home)
		cfg.AddConfigPath(".")
		cfg.SetConfigName(".myapp")
		cfg.SetConfigType("yaml")
	}
	cfg.SetEnvPrefix("MYAPP")
	cfg.SetEnvKeyReplacer(strings.NewReplacer("-", "_", ".", "_"))
	cfg.AutomaticEnv()
	if err := cfg.ReadInConfig(); err != nil {
		if _, missing := err.(viper.ConfigFileNotFoundError); cfgFile != "" || !missing {
			return fmt.Errorf("reading config: %w", err)
		}
	}
	return nil
}
