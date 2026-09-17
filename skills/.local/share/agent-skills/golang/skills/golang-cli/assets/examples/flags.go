package main

import (
	"fmt"
	"github.com/spf13/cobra"
	"github.com/spf13/viper"
)

// Standalone example; do not register the same flag twice on a command.
func newFlagExampleCommand() *cobra.Command {
	cfg := viper.New()
	cmd := &cobra.Command{Use: "flags"}
	cmd.PersistentFlags().String("config", "", "config file path")
	cmd.Flags().IntP("port", "p", 8080, "port")
	cmd.Flags().String("host", "", "hostname")
	if err := cmd.MarkFlagRequired("host"); err != nil {
		panic(err)
	}
	cmd.Flags().Bool("json", false, "JSON output")
	cmd.Flags().Bool("yaml", false, "YAML output")
	cmd.MarkFlagsMutuallyExclusive("json", "yaml")
	cmd.Flags().String("output-file", "", "destination file")
	cmd.Flags().Bool("stdout", false, "write to stdout")
	cmd.MarkFlagsOneRequired("output-file", "stdout")
	cmd.Flags().String("env", "dev", "environment")
	if err := cmd.RegisterFlagCompletionFunc("env", func(*cobra.Command, []string, string) ([]string, cobra.ShellCompDirective) {
		return []string{"dev", "staging", "prod"}, cobra.ShellCompDirectiveNoFileComp
	}); err != nil {
		panic(err)
	}
	if err := cfg.BindPFlag("port", cmd.Flags().Lookup("port")); err != nil {
		panic(err)
	}
	cmd.RunE = func(cmd *cobra.Command, args []string) error {
		// Completion suggestions and required-flag markers do not validate values.
		env, err := cmd.Flags().GetString("env")
		if err != nil {
			return err
		}
		switch env {
		case "dev", "staging", "prod":
		default:
			return fmt.Errorf("invalid environment %q", env)
		}
		host, err := cmd.Flags().GetString("host")
		if err != nil {
			return err
		}
		if host == "" {
			return fmt.Errorf("host must not be empty")
		}
		_ = cfg.GetInt("port")
		return nil
	}
	return cmd
}
