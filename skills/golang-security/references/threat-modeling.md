# Threat Modeling Guide

Go-specific threat examples, trust boundaries, and optional frameworks for a requested security analysis. Use the project's risk model; diagrams, STRIDE, and numerical scoring are not prerequisites for reviewing code.

## Table of Contents

- [STRIDE Methodology](#stride-methodology)
  - [STRIDE per Element Matrix](#stride-per-element-matrix)
  - [Go-Specific STRIDE Analysis](#go-specific-stride-analysis)
- [DREAD Risk Scoring](#dread-risk-scoring)
  - [Example: SQL Injection in Login Handler](#example-sql-injection-in-login-handler)
- [Trust Boundary Analysis](#trust-boundary-analysis)
- [OWASP Top 10 Mapping for Go](#owasp-top-10-mapping-for-go)
- [Choosing Analysis and Validation](#choosing-analysis-and-validation)
- [Vulnerability Severity Matrix](#vulnerability-severity-matrix)

## STRIDE Methodology

When STRIDE is requested or useful for coverage, this matrix relates threat categories to data-flow elements. It is a reference, not a required traversal of every component:

### STRIDE per Element Matrix

| DFD Element                           | S   | T   | R   | I   | D   | E   |
| ------------------------------------- | --- | --- | --- | --- | --- | --- |
| External Entity (user, API client)    | X   |     | X   |     |     |     |
| Process (HTTP handler, gRPC service)  | X   | X   | X   | X   | X   | X   |
| Data Store (database, cache, file)    |     | X   | X   | X   | X   |     |
| Data Flow (HTTP, gRPC, message queue) |     | X   |     | X   | X   |     |

### Go-Specific STRIDE Analysis

**Spoofing** — Can an attacker impersonate a user or service?

```go
// Which endpoints require authentication, and which are deliberately public?
// Check: Are JWT tokens validated (algorithm, issuer, expiry)?
// Does service authentication match the deployment trust model?
r.Use(authMiddleware) // apply to a route group that requires authentication
```

**Tampering** — Can data be modified in transit or at rest?

```go
// Check: Are all external inputs validated?
// Check: Is HMAC used for webhook/callback verification?
mac := hmac.New(sha256.New, key)
mac.Write(payload)
expected := mac.Sum(nil)
if !hmac.Equal(signature, expected) {
    return errors.New("tampered payload")
}
```

**Repudiation** — Can a user deny performing an action?

```go
// Check: Are all security-relevant actions logged with structured data?
logger.Info("action_performed",
    "user_id", userID,
    "action", "delete_account",
    "ip", r.RemoteAddr,
    "timestamp", time.Now().UTC(),
)
```

**Information Disclosure** — Can sensitive data leak?

```go
// Check: Are error messages generic to clients?
// Are logged fields limited to the intended audience and retention policy?
// Check: Is TLS configured (no InsecureSkipVerify)?
// Are diagnostic endpoints restricted to authorized operators?
```

**Denial of Service** — Can the service be overwhelmed?

```go
// Check: Are timeouts set on the HTTP server?
// Check: Are request body sizes limited?
// Check: Is rate limiting in place?
server := &http.Server{
    ReadTimeout:    5 * time.Second,
    WriteTimeout:   10 * time.Second,
    MaxHeaderBytes: 1 << 20, // 1MB
}
```

**Elevation of Privilege** — Can a user gain unauthorized access?

```go
// Check: Is authorization checked server-side on every request?
// Check: Are object references validated (no IDOR)?
// Check: Are admin routes properly protected?
if !user.HasPermission("admin:write") {
    http.Error(w, "Forbidden", http.StatusForbidden)
    return
}
```

---

## DREAD Risk Scoring

Use DREAD when the project requests that scoring method. The scales below are illustrative; their arithmetic is not a calibrated measure of risk. Explain the concrete impact and attack prerequisites behind any score:

| Factor | 1-3 (Low) | 4-6 (Medium) | 7-10 (High) |
| --- | --- | --- | --- |
| **D**amage | Minor info disclosure | Partial data breach | Full system compromise, data destruction |
| **R**eproducibility | Timing-dependent, hard to reproduce | Reproducible with some effort | Always reproducible, automated tools exist |
| **E**xploitability | Custom exploit, advanced skills needed | Basic tools available | No skills required, public exploit exists |
| **A**ffected users | Individual user | Subset of users | All users |
| **D**iscoverability | Requires insider knowledge | Found via scanning | Publicly documented, obvious |

**Score** = (D + R + E + A + D) / 5. Risk levels: **8-10 Critical**, **6-7.9 High**, **4-5.9 Medium**, **1-3.9 Low**.

### Example: SQL Injection in Login Handler

| Factor          | Score | Justification                              |
| --------------- | ----- | ------------------------------------------ |
| Damage          | 9     | Full database access, credential theft     |
| Reproducibility | 9     | Consistent, automated tools exist (sqlmap) |
| Exploitability  | 8     | Well-documented attack, easy tooling       |
| Affected Users  | 10    | All users with accounts                    |
| Discoverability | 7     | Automated scanners detect easily           |

**Illustrative DREAD score: 8.6.** This assumes the broad database privileges and exposure listed above; actual severity depends on the reachable query, database role, and affected data.

---

## Trust Boundary Analysis

Map where untrusted data enters your Go application:

```
                        ┌─────────────────────────────────────┐
                        │           TRUST BOUNDARY             │
                        │                                      │
Internet ──→ [LB/WAF] ──→ [Go HTTP Server]                   │
                        │        │                             │
                        │   [Middleware]                        │
                        │   - Auth (JWT/session)               │
                        │   - Rate limiting                    │
                        │   - Input validation                 │
                        │   - Security headers                 │
                        │        │                             │
                        │   [Service Layer] ──→ [Cache]        │
                        │        │                             │
                        │   [Database] (parameterized queries) │
                        │                                      │
                        └──────────┬──────────────────────────┘
                                   │
                          External APIs (mTLS)
```

For each relevant boundary, distinguish deliberately public access from operations requiring an authenticated identity. The useful distinctions are:

1. **Authentication** — who is making this request?
2. **Input validation** — is the data well-formed and within bounds?
3. **Authorization** — is this caller allowed to perform this action on this resource?

---

## OWASP Top 10 Mapping for Go

The categories below refer to the OWASP Top 10 **2021** edition, not a version-independent taxonomy.

| Rank | Vulnerability | STRIDE | Go Defense |
| --- | --- | --- | --- |
| A01 | Broken Access Control | E | Server-side authz middleware, RBAC, IDOR checks |
| A02 | Cryptographic Failures | I | `crypto/aes` GCM, `crypto/rand`, TLS 1.2+ |
| A03 | Injection | T, E | `database/sql` placeholders, `exec.Command` separate args, `html/template` |
| A04 | Insecure Design | All | Threat modeling with STRIDE, defense-in-depth |
| A05 | Security Misconfiguration | I, E | Server timeouts, TLS config, no `InsecureSkipVerify`, no exposed pprof |
| A06 | Vulnerable Components | All | `govulncheck`, Dependabot/Renovate, `go.sum` verification |
| A07 | Authentication Failures | S, E | Argon2id/bcrypt, JWT validation (algorithm pinning), MFA |
| A08 | Software/Data Integrity | T | Module checksums (`go.sum`), signed releases, CI verification |
| A09 | Logging Failures | R | Structured logging (`log/slog`), audit trails, no PII |
| A10 | SSRF | I, T | URL allowlists, block internal IPs and metadata endpoints |

---

## Choosing Analysis and Validation

Anchor findings in a reachable attack path, the affected asset, and the controls already in place. Use a diagram when relationships or trust boundaries are difficult to follow; use formal taxonomies and scoring when the requested deliverable calls for them.

Validation should address the claimed mitigation. Authorization flaws need cross-user or cross-tenant cases; input limits need boundary cases; races need exercised concurrent paths. Linters and vulnerability scans cover particular classes of defects, not the security of the whole design.

---

## Vulnerability Severity Matrix

An illustrative qualitative matrix for projects that use this approach. Apply the project's severity definitions and explain impact and exploitability rather than treating a matrix cell as proof:

| Impact \ Exploitability | Easy     | Moderate | Difficult |
| ----------------------- | -------- | -------- | --------- |
| Critical                | Critical | Critical | High      |
| High                    | Critical | High     | Medium    |
| Medium                  | High     | Medium   | Low       |
| Low                     | Medium   | Low      | Low       |
