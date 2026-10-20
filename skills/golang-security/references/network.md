# Network/Web Security Rules

Network and web security vulnerabilities can lead to data leakage and unauthorized access.

**Rules:**

1. Redirects MUST be validated against an allowlist of domains.
2. Configure header, body, write, and idle timeouts for the endpoint; streaming handlers need a different write-timeout policy from short requests.
3. Pprof endpoints MUST NEVER be exposed publicly.
4. Check XML parser behavior: standard `encoding/xml` does not resolve external entities; disable external resolution in parsers that support it.

---

## Table of Contents

- [Open Redirect Vulnerability — Medium](#open-redirect-vulnerability--medium)
- [Bind to All Interfaces — Medium](#bind-to-all-interfaces--medium)
- [Slowloris Attack Vulnerability — Medium](#slowloris-attack-vulnerability--medium)
- [Insecure HTTP Server Configuration — Medium](#insecure-http-server-configuration--medium)
- [Observable Timing (Timing Attacks) — Medium](#observable-timing-timing-attacks--medium)
- [Exposed pprof Profiling Endpoints — High](#exposed-pprof-profiling-endpoints--high)
- [XXE Vulnerability — High](#xxe-vulnerability--high)
- [Permissive Regex Validation — Low](#permissive-regex-validation--low)
- [CWE References](#cwe-references)

## Open Redirect Vulnerability — Medium

Redirects to unvalidated URLs can be used for phishing.

**Bad:**

```go
target := r.URL.Query().Get("url")
http.Redirect(w, r, target, http.StatusFound) // DON'T
```

**Good:**

```go
target := r.URL.Query().Get("url")
u, err := url.Parse(target)
if err != nil || u.User != nil { return errors.New("invalid URL") }
// Only allow http/https
if u.Scheme != "http" && u.Scheme != "https" {
    return errors.New("invalid scheme")
}
// Exact hostname allowlist; validate ports too if the policy restricts them.
if !isAllowedDomain(strings.ToLower(u.Hostname())) {
    return errors.New("invalid domain")
}
http.Redirect(w, r, target, http.StatusFound)
```

---

## Bind to All Interfaces — Medium

Binding to 0.0.0.0 listens on all IPv4 interfaces. This is normal for public/container services; it is a finding only when exposure violates the service boundary or lacks required access controls. Loopback is appropriate for local-only services.

**Bad:**

```go
listener, _ := net.Listen("tcp", "0.0.0.0:8080") // DON'T: Exposes all interfaces
```

**Good:**

```go
// Bind only to localhost
listener, _ := net.Listen("tcp", "127.0.0.1:8080")

// Or specific internal IP
listener, _ := net.Listen("tcp", "10.0.1.5:8080")
```

---

## Slowloris Attack Vulnerability — Medium

Slowloris attacks exhaust connection pools.

**Bad:**

```go
server := &http.Server{
    Addr: ":8080",
    // Missing ReadTimeout, WriteTimeout, IdleTimeout
}
```

**Good:**

```go
server := &http.Server{
    Addr:         ":8080",
    ReadHeaderTimeout: 5 * time.Second,
    ReadTimeout:  5 * time.Second,
    WriteTimeout: 10 * time.Second,
    IdleTimeout:  120 * time.Second,
    MaxHeaderBytes: 1 << 20, // Limit header size to 1MB
}
```

---

## Insecure HTTP Server Configuration — Medium

Running HTTP servers without proper security settings.

**Bad:**

```go
http.ListenAndServe(":8080", handler) // DON'T: No security hardening
```

**Good:**

```go
server := &http.Server{
    Addr:         ":443",
    ReadHeaderTimeout: 5 * time.Second,
    ReadTimeout:  5 * time.Second,
    WriteTimeout: 10 * time.Second,
    IdleTimeout:  120 * time.Second,
    MaxHeaderBytes: 1 << 20,
}
server.ListenAndServeTLS("cert.pem", "key.pem")
```

---

## Observable Timing (Timing Attacks) — Medium

Timing differences can leak sensitive information.

**Bad:**

```go
func checkPassword(input, secret string) bool {
    return input == secret // DON'T: Short-circuit leaks length
}
```

**Good:**

```go
import "crypto/subtle"

// For comparing fixed-length tokens or hashes:
func checkToken(input, expected string) bool {
    // Unequal lengths return immediately; use a fixed-length token/hash format.
    return subtle.ConstantTimeCompare([]byte(input), []byte(expected)) == 1
}

// For passwords, derive with Argon2id and compare the stored/derived fixed-length
// hashes with ConstantTimeCompare. IDKey only derives; it does not verify.
// bcrypt.CompareHashAndPassword performs verification:
// argon2: hash := argon2.IDKey([]byte(password), salt, 3, 64*1024, 4, 32)
// bcrypt: err := bcrypt.CompareHashAndPassword([]byte(storedHash), []byte(password))

// For HMAC verification:
import "crypto/hmac"
func verifyHMAC(message, messageMAC, key []byte) bool {
    mac := hmac.New(sha256.New, key)
    mac.Write(message)
    expectedMAC := mac.Sum(nil)
    return hmac.Equal(messageMAC, expectedMAC) // Constant-time
}
```

---

## Exposed pprof Profiling Endpoints — High

Debug pprof endpoints expose sensitive runtime information.

**Bad:**

```go
import _ "net/http/pprof" // DON'T: Automatically registers /debug/pprof
http.ListenAndServe(":8080", nil) // nil serves DefaultServeMux
```

**Good:**

```go
// Option 1: Use build tags to exclude pprof from production builds
// File: debug_pprof.go
//go:build !production

package main

import _ "net/http/pprof"

// Option 2 (separate alternative; use a named "net/http/pprof" import):
// Serve pprof on a separate internal-only listener
func startDebugServer() {
    debugMux := http.NewServeMux()
    debugMux.HandleFunc("/debug/pprof/", pprof.Index)
    debugMux.HandleFunc("/debug/pprof/cmdline", pprof.Cmdline)
    debugMux.HandleFunc("/debug/pprof/profile", pprof.Profile)
    debugMux.HandleFunc("/debug/pprof/symbol", pprof.Symbol)
    debugMux.HandleFunc("/debug/pprof/trace", pprof.Trace)
    go http.ListenAndServe("127.0.0.1:6060", debugMux) // localhost only
}
```

---

## XXE Vulnerability — High

External entity resolution can expose files or make unintended network requests in parsers that support it. Go's standard `encoding/xml` does not resolve external entities or process DTDs; `Strict` is already true by default and is not an XXE switch. A raw substring filter is not a substitute for knowing the parser's behavior.

```go
// In an HTTP handler: bound input and handle malformed XML.
r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
if err := xml.NewDecoder(r.Body).Decode(&person); err != nil {
    http.Error(w, "invalid XML", http.StatusBadRequest)
    return
}
```

For a third-party XML library, verify its entity/DTD defaults and disable external resolution. Bound input and application-level nesting/work independently of XXE protection.

---

## Permissive Regex Validation — Low

Weak regex validation can allow malicious input.

**Bad:**

```go
matched, _ := regexp.MatchString(`.+@.+\..+`, email) // DON'T: Too permissive
```

**Good:**

```go
addr, err := mail.ParseAddress(email)
if err != nil || addr.Address != email {
    return errors.New("invalid email address")
}
// Parsing does not prove deliverability or ownership: verify by email when needed.
// Use context-appropriate escaping/parameterization at each output sink.
```

---

## CWE References

- **CWE-601**: Open Redirect
- **CWE-208**: Observable Timing Discrepancy
- **CWE-611**: Improper Restriction of XML External Entity Reference
- **CWE-770**: Allocation of Resources Without Limits
- **CWE-20**: Improper Input Validation
- **CWE-200**: Exposure of Sensitive Information
