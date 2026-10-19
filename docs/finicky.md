# Finicky browser routing

Finicky is an optional macOS default URL handler. The shared config routes
HTTPS links on `oauth2.sigstore.dev` and HTTP(S) links on `github.com` (including
subdomains) to Brave's Work profile. YouTube and Twitch
HTTP(S) links go to Personal, including subdomains, `youtu.be` short links, and
`youtube-nocookie.com` embeds. Other links use Brave's usual last-active-profile
behavior. Links already inside Brave are not
automatically rerouted; the optional upstream browser extension can forward
them to Finicky.

## Install and activate

```sh
brew install --cask finicky
mise run finicky:install
open -a Finicky
```

Choose Finicky as the default browser when macOS prompts. If needed, select it
under System Settings > Desktop & Dock > Default web browser. Base `mise run install`
does not link this optional package. Finicky watches its configuration for edits.

The profile directories are configured in
`home/finicky/.config/finicky/finicky.js`. Open `brave://version` in the intended Brave
profile and use the final component of **Profile Path**. On this machine,
Work uses `Default` and Personal uses `Profile 1`. Check these values when
setting up a new machine or recreating browser profiles.

Routing matches hostnames with explicit subdomain boundaries, not arbitrary URL text. Add additional
public domains deliberately; employer names and private domains must not be
committed to this public repository. OAuth URLs are forwarded without removing
query parameters or following authentication redirects in our rule.

## Implementation review

The initial focused review inspected upstream stable tag
[`v4.2.2`](https://github.com/johnste/finicky/tree/v4.2.2), commit
`53414af0c1caba2a606e512848486366baa9c366`. This was a review of routing,
profile launching, logging, and update behavior, not a full security audit or
verification that the distributed binary reproduces the source build.

- Browser arguments are passed through `exec.Command`, without shell evaluation.
- The built-in `profile` lookup can fall back to an ordinary browser launch if
  it cannot resolve a profile. This config instead supplies `open -n --args
  --profile-directory=...` explicitly, so a failed lookup cannot select the
  last active profile. A removed profile directory may be recreated by Brave;
  it does not recover the old profile's sessions.
- Normal URL logging to disk is disabled. Finicky still keeps diagnostic URLs
  in memory/stdout, and upstream error handling can enable file logging. Treat
  its diagnostic output as private, especially during authentication.
- Built-in update checks are disabled; use Homebrew to update the app.
- Upstream also resolves known URL-shortener hosts over the network before
  matching. The signing host above is not a URL shortener.

Source: [browser launcher](https://github.com/johnste/finicky/blob/v4.2.2/apps/finicky/src/browser/launcher.go),
[logging](https://github.com/johnste/finicky/blob/v4.2.2/apps/finicky/src/logger/logger.go),
[config runtime](https://github.com/johnste/finicky/blob/v4.2.2/apps/finicky/src/config/vm.go).

## Disable

Select Brave again as the default web browser in System Settings, then quit
Finicky. The routing config can remain linked for future use.
