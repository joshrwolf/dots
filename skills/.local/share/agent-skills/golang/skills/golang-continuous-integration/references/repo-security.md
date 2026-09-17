# Repository Security Settings

After creating workflow files, repository security settings should be configured. Apply the project's threat model and available GitHub plan; configuration examples are not permission to change repository settings.

The project's GitHub URL can be determined from its git remote (e.g., `git remote -v`). For a project hosted at `https://github.com/{owner}/{repo}`, the relevant settings links are:

- Branch protection: `https://github.com/{owner}/{repo}/settings/branches`
- Actions permissions: `https://github.com/{owner}/{repo}/settings/actions`
- Secrets: `https://github.com/{owner}/{repo}/settings/secrets/actions`
- Environments: `https://github.com/{owner}/{repo}/settings/environments`

These links allow direct navigation to the appropriate settings page.

## Branch Protection Rules

Configure a branch protection rule for `main` (or the default branch):

1. **Require a pull request before merging** — prevents direct pushes to main
2. **Require approvals** (at least 1) — no self-merging without review
3. **Dismiss stale pull request approvals when new commits are pushed** — prevents approving then sneaking in changes
4. **Require status checks to pass before merging** — select stable, uniquely named gating checks (not optional/skipped jobs) (e.g., `Test (Go 1.24)`, `Test (Go stable)`, `Lint`)
5. **Require branches to be up to date before merging** — prevents merging stale PRs that haven't been tested against latest main
6. **Do not allow bypassing the above settings** — applies rules to admins too

## Workflow Permissions

Set the default `GITHUB_TOKEN` to **read-only** at the repository level:

1. Go to **Actions permissions** (link above)
2. Workflow permissions MUST follow least privilege. Under **Workflow permissions**, select **"Read repository contents and packages permissions"**
3. Uncheck **"Allow GitHub Actions to create and approve pull requests"** (enable only if a workflow must create or approve PRs; enabling auto-merge on an existing PR is different)

This gives workflows read-only defaults. Each workflow that needs elevated permissions must explicitly declare them in its `permissions:` block. This is defense-in-depth: if a workflow is compromised, it cannot write to the repository unless explicitly granted.

## Fork Pull Request Restrictions

For public/open-source repositories:

1. In **Actions permissions** (link above), set **"Fork pull request workflows from outside collaborators"** to **"Require approval for all outside collaborators"**
2. This requires approval before those runs consume resources; approval does not grant fork pull_request workflows repository secrets
3. NEVER use `pull_request_target` with untrusted code — it runs with write access to the base repo

## Secrets and Environments

- Never put secrets in workflow files — use **Secrets** settings (link above)
- If releases need approval, create a **"release" environment** with required reviewers and reference it with `environment: release` on the release job; creating the environment alone does not gate the job
- Rotate `CODECOV_TOKEN` and other third-party tokens periodically

## Permissions Cheat Sheet

The security implications of every permission used are documented below:

| Permission | Workflows that need it | Risk |
| --- | --- | --- |
| `contents: read` | All workflows | **Low** — read-only, default safe |
| `contents: write` | Release, auto-merge | **High** — can modify repo contents, create releases |
| `packages: write` | Docker | **High** — can push container images to GHCR |
| `pull-requests: write` | Auto-merge | **High** — can merge PRs, approve changes |
| `attestations: write` | Explicit GitHub artifact-attestation integration | **Medium** — can create provenance/SBOM attestations |
| `id-token: write` | OIDC/signing integrations that request a token | **Medium** — OIDC token for signing attestations |
| `security-events: write` | Security/SAST, Docker | **Medium** — can upload SARIF to Security tab |

Always prefer the narrowest permission scope. If a workflow only needs `contents: read`, do not grant `contents: write`.
