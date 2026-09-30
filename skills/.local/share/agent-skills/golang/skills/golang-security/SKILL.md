---
name: golang-security
description: "Review and harden Go code against security threats. Use for injection, authentication, cryptography, unsafe input handling, or vulnerability audits."
user-invocable: true
license: MIT
compatibility: Designed for Claude Code, Codex or similar harness, and for projects using Golang.
metadata:
  author: samber
  version: "1.2.2"
  openclaw:
    emoji: "🔒"
    homepage: https://github.com/samber/cc-skills-golang
    requires:
      bins:
        - go
        - govulncheck
    install:
      - kind: go
        package: golang.org/x/vuln/cmd/govulncheck@latest
        bins: [govulncheck]
allowed-tools: Read Edit Write Glob Grep Bash(go:*) Bash(golangci-lint:*) Bash(git:*) Agent WebFetch Bash(govulncheck:*) WebSearch AskUserQuestion EnterWorktree ExitWorktree
paths:
  - "**/*.go"
---

When reviewing a change, trace affected call sites and data flows into adjacent code;
a vulnerability may live outside the diff but be triggered by it. Report findings
for an audit request; implement fixes when authorized, grouping related changes
for review.

**Dependencies:**

- govulncheck: `go install golang.org/x/vuln/cmd/govulncheck@latest`

# Go Security

## Overview

Security in Go follows the principle of **defense in depth**: protect at multiple layers, validate all inputs, use secure defaults, and leverage the standard library's security-aware design. Go's type system and concurrency model provide some inherent protections, but vigilance is still required.

## Findings and Severity

A finding needs a reachable attack path, attacker-controlled input, and a concrete impact. Account for validation and authorization in callers, middleware, or interceptors: concatenating an already validated integer is not SQL injection merely because placeholders would be clearer. Distinguish exploitable defects from defense-in-depth improvements.

Use the project's severity definitions. Exposure, privileges, affected data, and existing controls determine priority; a vulnerability category alone does not establish severity or a remediation deadline. Explain any important preconditions in the review. Add a code comment only when it captures a non-obvious invariant future maintainers need, not to record every dismissed finding.

## Threat Modeling

For a requested threat analysis, connect the affected assets and trust boundaries to plausible attacks and mitigations. STRIDE can help organize coverage; diagrams and DREAD scoring are optional deliverables when requested or used by the project.

The **[Threat Modeling Guide](./references/threat-modeling.md)** retains Go examples, trust-boundary diagrams, optional scoring references, and the OWASP mapping.

## Quick Reference

| Vulnerability | Defense | Standard Library Solution |
| --- | --- | --- |
| SQL Injection | Parameterized queries separate data from code | `database/sql` with `?` placeholders |
| Command Injection | Pass args separately, never via shell concatenation | `exec.Command` with separate args |
| XSS | Auto-escaping renders user data as text, not HTML/JS | `html/template` (`text/template` does not HTML-escape) |
| Path Traversal | Scope untrusted file access to an allowed root | Go 1.24+: use `os.Root`. Pre-Go 1.24: use `filepath.IsLocal` + `filepath.Rel` + separator-aware checks; never rely on `filepath.Clean` + `strings.HasPrefix` alone. |
| Timing Attacks | Constant-time comparison avoids byte-by-byte leaks | `crypto/subtle.ConstantTimeCompare` |
| Crypto Issues | Use vetted algorithms; never roll your own | `crypto/aes`, `crypto/rand` |
| HTTP Security | TLS + security headers prevent downgrade attacks | `net/http`, configure TLSConfig |
| Missing Headers | HSTS, CSP, X-Frame-Options prevent browser attacks | Security headers middleware |
| Rate Limiting | Rate limits prevent brute-force and resource exhaustion | `golang.org/x/time/rate`, server timeouts |
| Race Conditions | Protect shared state to prevent data corruption | `sync.Mutex`, channels, avoid shared state |

## Detailed Categories

For complete examples, code snippets, and CWE mappings, see:

- **[Cryptography](./references/cryptography.md)** — Algorithms, key derivation, TLS configuration.
- **[Injection Vulnerabilities](./references/injection.md)** — SQL, command, template injection, XSS, SSRF.
- **[Filesystem Security](./references/filesystem.md)** — Path traversal, zip bombs, file permissions, symlinks.
- **[Network/Web Security](./references/network.md)** — SSRF, open redirects, HTTP headers, timing attacks, session fixation.
- **[Cookie Security](./references/cookies.md)** — Secure, HttpOnly, SameSite flags.
- **[Third-Party Data Leaks](./references/third-party.md)** — Analytics privacy risks, GDPR/CCPA compliance.
- **[Memory Safety](./references/memory-safety.md)** — Integer overflow, memory aliasing, `unsafe` usage.
- **[Secrets Management](./references/secrets.md)** — Hardcoded credentials, env vars, secret managers.
- **[Logging Security](./references/logging.md)** — PII in logs, log injection, sanitization.
- **[Threat Modeling Guide](./references/threat-modeling.md)** — STRIDE, DREAD scoring, trust boundaries, OWASP Top 10.
- **[Security Architecture](./references/architecture.md)** — Defense-in-depth, Zero Trust, auth patterns, rate limiting, anti-patterns.

## Code Review Checklist

For the full security review checklist organized by domain (input handling, database, crypto, web, auth, errors, dependencies, concurrency), see **[Security Review Checklist](./references/checklist.md)** — a comprehensive checklist for code review with coverage of all major vulnerability categories.

## Tooling & Verification

### Static Analysis & Linting

Security-relevant linters: `bodyclose`, `sqlclosecheck`, `nilerr`, `errcheck`, `govet`, `staticcheck`. See the `samber/cc-skills-golang@golang-lint` skill for configuration and usage.

For deeper security-specific analysis:

```bash
# Go security checker (SAST)
go get -tool github.com/securego/gosec/v2/cmd/gosec@latest
go tool gosec ./...

# Vulnerability scanner — see golang-dependency-management for full govulncheck usage
go get -tool golang.org/x/vuln/cmd/govulncheck@latest
go tool govulncheck ./...
```

For a module not yet in the build, consult the official Go vulnerability database. API documentation alone does not establish vulnerability status.

### Security Testing

```bash
# Race detector
go test -race ./...

# Fuzz testing
go test -fuzz=Fuzz
```

## Common Mistakes

| Mistake | Fix |
| --- | --- |
| `math/rand` for tokens | Output is predictable — attacker can reproduce the sequence. Use `crypto/rand` |
| SQL string concatenation | Attacker can modify query logic. Parameterized queries keep data and code separate |
| `exec.Command("bash -c")` | Shell interprets metacharacters (`;`, `\|`, `` ` ``). Pass args separately to avoid shell parsing |
| Trusting unsanitized input | Validate at trust boundaries — internal code trusts the boundary, so catching bad input there protects everything |
| Hardcoded secrets | Secrets in source code end up in version history, CI logs, and backups. Use env vars or secret managers |
| Comparing secrets with `==` | `==` short-circuits on first differing byte, leaking timing info. Use `crypto/subtle.ConstantTimeCompare` |
| Returning detailed errors | Stack traces and DB errors help attackers map your system. Return generic messages, log details server-side |
| Ignoring `-race` findings | Races cause data corruption and can bypass authorization checks under concurrency. Fix all races |
| MD5/SHA1 for passwords | Fast general-purpose hashes enable offline password guessing. Prefer Argon2id; bcrypt is a deliberately slow legacy option, not memory-hard |
| Encryption without authentication | Use a vetted AEAD such as AES-GCM or ChaCha20-Poly1305; unauthenticated ECB/CBC does not protect ciphertext integrity |
| Binding to 0.0.0.0 | Exposes service to all network interfaces. Bind to specific interface to limit attack surface |

## Security Anti-Patterns

| Anti-Pattern | Why It Fails | Fix |
| --- | --- | --- |
| Security through obscurity | Hidden URLs are discoverable via fuzzing, logs, or source | Authentication + authorization on all endpoints |
| Trusting client headers | `X-Forwarded-For`, `X-Is-Admin` are trivially forged | Server-side identity verification |
| Client-side authorization | JavaScript checks are bypassed by any HTTP client | Server-side permission checks on every handler |
| Shared secrets across envs | Staging breach compromises production | Per-environment secrets via secret manager |
| Ignoring crypto errors | `_, _ = encrypt(data)` silently proceeds unencrypted | Always check errors — fail closed, never open |
| Rolling your own crypto | Custom encryption hasn't been analyzed by cryptographers | Use `crypto/aes` GCM, `golang.org/x/crypto/argon2` |

See **[Security Architecture](./references/architecture.md)** for detailed anti-patterns with Go code examples.

## Cross-References

See `samber/cc-skills-golang@golang-database`, `samber/cc-skills-golang@golang-safety`, `samber/cc-skills-golang@golang-observability`, `samber/cc-skills-golang@golang-continuous-integration` skills.

- → See `samber/cc-skills-golang@golang-continuous-integration` skill for automated AI-driven code review in CI using these guidelines

## Additional Resources

- [Go Security Best Practices](https://go.dev/doc/security/best-practices)
- [gosec Security Linter](https://github.com/securego/gosec)
- [govulncheck](https://pkg.go.dev/golang.org/x/vuln/cmd/govulncheck)
- [OWASP Go Secure Coding Practices](https://owasp.org/www-project-go-secure-coding-practices-guide/)
