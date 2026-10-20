package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"text/tabwriter"

	"github.com/fatih/color"
	"github.com/spf13/cobra"
	"golang.org/x/term"
)

// === stdout vs stderr ===
// stdout: Program output (data, results). This is what gets piped.
// stderr: Logs, progress, errors, diagnostics. Not piped by default.

func outputExample(cmd *cobra.Command, result string, err error) {
	// Output data to stdout (pipeable)
	fmt.Fprintln(cmd.OutOrStdout(), result)

	// Logs and errors to stderr (use slog)
	// slog.Error("operation failed", "error", err)
}

// === Detecting Pipe vs Terminal ===

func isTerminal(w io.Writer) bool {
	file, ok := w.(*os.File)
	return ok && term.IsTerminal(int(file.Fd()))
}

// === Machine-Readable Output ===
// Support --output flag for different output formats.

type User struct {
	ID   string
	Name string
}

func printUsers(cmd *cobra.Command, users []User) error {
	format, err := cmd.Flags().GetString("output")
	if err != nil {
		return err
	}
	switch format {
	case "json":
		enc := json.NewEncoder(cmd.OutOrStdout())
		enc.SetIndent("", "  ")
		return enc.Encode(users)
	case "plain":
		for _, u := range users {
			if _, err := fmt.Fprintf(cmd.OutOrStdout(), "%s\t%s\n", u.ID, u.Name); err != nil {
				return err
			}
		}
	case "table":
		w := tabwriter.NewWriter(cmd.OutOrStdout(), 0, 0, 2, ' ', 0)
		if _, err := fmt.Fprintln(w, "ID\tNAME"); err != nil {
			return err
		}
		for _, u := range users {
			if _, err := fmt.Fprintf(w, "%s\t%s\n", u.ID, u.Name); err != nil {
				return err
			}
		}
		return w.Flush()
	default:
		return fmt.Errorf("unsupported output format %q", format)
	}
	return nil
}

// === Colors ===
// Use fatih/color — it auto-disables when output is not a terminal.

func colorExamples(cmd *cobra.Command, env string, failure error) error {
	output := cmd.OutOrStdout()
	style := color.New(color.FgGreen)
	if !isTerminal(output) {
		style.DisableColor()
	}
	if _, err := style.Fprintf(output, "deployed to %s\n", env); err != nil {
		return err
	}
	if failure != nil {
		// Diagnostics go to the command's error stream, not color's global writer.
		_, err := fmt.Fprintf(cmd.ErrOrStderr(), "error: %v\n", failure)
		return err
	}
	return nil
}
