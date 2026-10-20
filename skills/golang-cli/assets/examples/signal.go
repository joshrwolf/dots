package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/spf13/cobra"
)

// Alternative to the minimal serve.go example: a real HTTP server lifecycle.
var serveWithSignalCmd = &cobra.Command{
	Use:   "serve",
	Short: "Start the server",
	RunE: func(cmd *cobra.Command, args []string) error {
		ctx, stop := signal.NotifyContext(cmd.Context(), os.Interrupt, syscall.SIGTERM)
		defer stop()
		srv := &http.Server{Addr: ":8080", Handler: http.NewServeMux(), ReadHeaderTimeout: 5 * time.Second}
		return runServer(ctx, srv, stop)
	},
}

func runServer(ctx context.Context, srv *http.Server, stop func()) error {
	serveErr := make(chan error, 1)
	go func() { serveErr <- srv.ListenAndServe() }()
	select {
	case err := <-serveErr:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return fmt.Errorf("server failed: %w", err)
	case <-ctx.Done():
		stop() // restore default handling so a second interrupt can terminate
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		err := srv.Shutdown(shutdownCtx) // wait for draining before returning
		if err != nil {
			err = errors.Join(err, srv.Close())
		}
		if listenErr := <-serveErr; !errors.Is(listenErr, http.ErrServerClosed) {
			err = errors.Join(err, listenErr)
		}
		return err
	}
}
