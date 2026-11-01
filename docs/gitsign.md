# Commit signing

Commits are signed with stock [gitsign](https://github.com/sigstore/gitsign).
Its system credential cache keeps credentials in the macOS Keychain for
the certificate lifetime (about 10 minutes), so repeated signing skips the
browser flow without a daemon or wrapper.

## Setup

Enable the Keychain cache once:

```sh
git config --global gitsign.credentialCacheMode system
```

Then opt in each repository:

```sh
git config --local gpg.format x509
git config --local gpg.x509.program gitsign
git config --local commit.gpgsign true
```

If the cache cannot be set up, gitsign falls back to ordinary OIDC
authentication.

## Use

```sh
gitsign credentials list   # show cached credentials
gitsign credentials clear  # remove the current configuration's credential
```

## Constraints

Machine setup supplies the current stable release through native mise. System caching requires
gitsign v0.18.0 or later and a macOS build with cgo enabled. After an upgrade,
Keychain may ask you to approve access for the new binary.

Caching does not enable repository signing; opt in only the repositories you
want signed. Credential reuse is automatic, and authentication is needed again
after expiry.
