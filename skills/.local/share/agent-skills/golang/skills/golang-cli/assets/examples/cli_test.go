package main

import (
	"bytes"
	"testing"

	"github.com/spf13/cobra"
)

// Test commands by executing them programmatically and capturing output.
// Use cmd.OutOrStdout() and cmd.ErrOrStderr() in commands (instead of
// os.Stdout / os.Stderr) so output can be captured in tests.

func executeCommand(root *cobra.Command, args ...string) (string, error) {
	buf := new(bytes.Buffer)
	root.SetOut(buf)
	root.SetErr(buf)
	root.SetArgs(args)
	err := root.Execute()
	return buf.String(), err
}

func TestServeCommand(t *testing.T) {
	t.Setenv("HOME", t.TempDir())
	t.Setenv("MYAPP_PORT", "")
	t.Chdir(t.TempDir())
	tests := []struct {
		name    string
		args    []string
		want    string
		wantErr bool
	}{
		{
			name: "default port",
			args: []string{"serve"},
			want: "configured address :8080\n",
		},
		{
			name: "custom port",
			args: []string{"serve", "--port", "9090"},
			want: "configured address :9090\n",
		},
		{
			name:    "invalid port",
			args:    []string{"serve", "--port", "0"},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := executeCommand(newRootCommand(), tt.args...)
			if (err != nil) != tt.wantErr {
				t.Errorf("error = %v, wantErr %v", err, tt.wantErr)
			}
			if !tt.wantErr && got != tt.want {
				t.Errorf("output = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestFlagExampleValidation(t *testing.T) {
	for _, args := range [][]string{
		{"--stdout"},
		{"--host", "", "--stdout"},
		{"--host", "localhost", "--stdout", "--env", "invalid"},
		{"--host", "localhost", "--stdout", "--json", "--yaml"},
	} {
		if _, err := executeCommand(newFlagExampleCommand(), args...); err == nil {
			t.Errorf("args %v: expected error", args)
		}
	}
	if _, err := executeCommand(newFlagExampleCommand(), "--host", "localhost", "--stdout"); err != nil {
		t.Fatal(err)
	}
}
