# Finicky routing

Finicky routes external links to selected Brave profiles on macOS.

## Setup

Select the desktop module in [machine setup](../bootstrap/README.md), then
open Finicky from Applications.

Select Finicky as the default browser in System Settings. Finicky watches its configuration for edits.

Check profile directories in `brave://version` before using
`home/finicky/.config/finicky/finicky.js`: the configured `Default` and
`Profile 1` names are machine-specific, not portable profile identities.

## Use

Signing and GitHub links go to Work; video links go to Personal. Other links use
Brave's last-active-profile behavior. The config is authoritative for host rules.
Links already inside Brave are not automatically rerouted.

To disable routing, select Brave as the default browser and quit Finicky.
Use native mise package upgrades; built-in update checks are disabled.

## Constraints

Explicit profile-directory arguments avoid lookup falling back to the last
active profile. Brave may recreate a missing directory, but cannot recover its
old sessions.

OAuth parameters are preserved. Normal disk URL logging is disabled, but
diagnostics and upstream error handling can still expose URLs; keep authentication
output private. Upstream may resolve known shortener hosts before routing.

Match hostname boundaries, not arbitrary URL text. Never commit private domains
or employer names to this public configuration.
