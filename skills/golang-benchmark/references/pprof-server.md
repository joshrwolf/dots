# pprof server setup

## Enable pprof HTTP Server

Restrict diagnostic access using the deployment's network and authentication controls.
These endpoints expose sensitive runtime details and allow expensive captures. The
examples below show a loopback development listener and one authenticated setup;
they do not require a particular environment-variable or authentication scheme.

### Quick Setup (Development)

```go
import _ "net/http/pprof"

func main() {
    go func() {
        log.Println(http.ListenAndServe("localhost:6060", nil))
    }()
    // ... rest of app
}
```

### Secure Setup (Production)

This example uses environment-controlled Basic authentication. Use it over TLS or
an appropriately protected local connection; Basic authentication alone does not
encrypt credentials. Integrate the handler with the service's existing server lifecycle:

```go
import "net/http/pprof"

func setupPprof(mux *http.ServeMux) {
    if os.Getenv("PPROF_ENABLED") != "true" {
        return
    }

    // Protect pprof endpoints with basic auth — never expose unauthenticated
    username := os.Getenv("PPROF_USERNAME")
    password := os.Getenv("PPROF_PASSWORD")
    if username == "" || password == "" {
        panic("PPROF_USERNAME and PPROF_PASSWORD must be set when pprof is enabled")
    }
    auth := basicAuth(username, password)

    mux.Handle("/debug/pprof/", auth(http.HandlerFunc(pprof.Index)))
    mux.Handle("/debug/pprof/cmdline", auth(http.HandlerFunc(pprof.Cmdline)))
    mux.Handle("/debug/pprof/profile", auth(http.HandlerFunc(pprof.Profile)))
    mux.Handle("/debug/pprof/symbol", auth(http.HandlerFunc(pprof.Symbol)))
    mux.Handle("/debug/pprof/trace", auth(http.HandlerFunc(pprof.Trace)))

    slog.Info("pprof endpoints enabled (basic auth required)")
}

// basicAuth wraps an http.Handler with HTTP Basic Authentication.
func basicAuth(username, password string) func(http.Handler) http.Handler {
    return func(next http.Handler) http.Handler {
        return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
            u, p, ok := r.BasicAuth()
            if !ok || u != username || subtle.ConstantTimeCompare([]byte(p), []byte(password)) != 1 {
                w.Header().Set("WWW-Authenticate", `Basic realm="pprof"`)
                http.Error(w, "unauthorized", http.StatusUnauthorized)
                return
            }
            next.ServeHTTP(w, r)
        })
    }
}
```

Importing `net/http/pprof` also registers handlers on `http.DefaultServeMux`.
Serve the explicit mux above, and do not expose the default mux through another
listener that bypasses these controls.

For capture commands and interpretation, see [pprof](pprof.md).
