STOW_PACKAGES := aerospace brew codex ghostty pi skills theme zsh
HERDR_STOW_PACKAGES := herdr nvim
HERDR_PLUGINS := herdr-plugins
PI_PLUGINS := pi-plugins
CLAUDE_MODS := claude-mods

all: stow

stow: skills-links
	stow --verbose --target="$(HOME)" --restow $(STOW_PACKAGES)

unstow:
	stow --verbose --target="$(HOME)" --delete $(STOW_PACKAGES)

nvim:
	stow --verbose --target="$(HOME)" --restow nvim

# Optional machine-local Slack credentials.
slack:
	stow --verbose --target="$(HOME)" --restow slack

.PHONY: slack

# Optional routing of external links to browser profiles.
finicky:
	test "$$(uname -s)" = Darwin
	test -x /Applications/Finicky.app/Contents/MacOS/Finicky
	stow --verbose --target="$(HOME)" --restow finicky

.PHONY: finicky

# Optional macOS signing cache. Repository signer selection stays explicit.
gitsign:
	test "$$(uname -s)" = Darwin
	test -x /opt/homebrew/bin/gitsign
	test -x /opt/homebrew/bin/gitsign-credential-cache
	test -x /opt/homebrew/bin/python3
	stow --verbose --target="$(HOME)" --restow gitsign
	launchctl enable "gui/$$(id -u)/dev.joshrwolf.gitsign-credential-cache"
	@launchctl print "gui/$$(id -u)/dev.joshrwolf.gitsign-credential-cache" >/dev/null 2>&1 || \
		launchctl bootstrap "gui/$$(id -u)" "$(HOME)/Library/LaunchAgents/dev.joshrwolf.gitsign-credential-cache.plist"

skills: skills-links
	stow --verbose --target="$(HOME)" --restow skills

skills-links:
	python3 scripts/skill-sources.py links

# Claude mods install through committed skills links; checks stay explicit.
mods-check:
	$(MAKE) -C $(CLAUDE_MODS) $(patsubst mods-%,%,$@)

# Deliberate network update; Git requires a clean working tree and creates a merge commit.
skills-update:
	python3 scripts/skill-sources.py update "$(SOURCE)"

skills-import:
	python3 scripts/skill-sources.py import "$(SOURCE)"

# Pi uses its official managed installer; no Homebrew or system npm dependency.
pi-cli-install:
	curl -fsSL https://pi.dev/install.sh | sh

pi-cli-update:
	pi update --self

# Explicit Pi package operations; plain make never installs dependencies or packages.
pi-install pi-deps pi-check:
	$(MAKE) -C $(PI_PLUGINS) $(patsubst pi-%,%,$@)

# Preserve the required setup order: binaries, config, then registration.
herdr: herdr-build
	stow --verbose --target="$(HOME)" --restow $(HERDR_STOW_PACKAGES)
	$(MAKE) -C $(HERDR_PLUGINS) link

herdr-build:
	$(MAKE) -C $(HERDR_PLUGINS) build

herdr-link:
	$(MAKE) -C $(HERDR_PLUGINS) link

herdr-check:
	$(MAKE) rust-check

# Shared Rust workspace: local commands and Herdr adapters use one lockfile/gate.
rust-check:
	$(MAKE) -C $(HERDR_PLUGINS) check

# Local commands are independently installable; no Herdr session is needed.
cli: cli-build skills-links
	stow --verbose --target="$(HOME)" --restow cli skills

cli-build:
	cargo build --release --package dots-cli
	@destination="cli/.local/bin/dots"; \
		mkdir -p "$$(dirname "$$destination")"; \
		temporary="$$destination.tmp.$$$$"; \
		trap 'rm -f "$$temporary"' EXIT HUP INT TERM; \
		cp target/release/dots "$$temporary"; \
		chmod +x "$$temporary"; \
		mv -f "$$temporary" "$$destination"; \
		trap - EXIT HUP INT TERM
	python3 scripts/cli-skills.py generate cli/.local/bin/dots

cli-skills:
	cargo build --package dots-cli
	python3 scripts/cli-skills.py generate target/debug/dots

cli-check:
	cargo fmt --all --check
	cargo clippy -p dots-cli -p dots-tasks -p github-inspect -p github-client --all-targets -- -D warnings
	cargo build --package dots-cli
	python3 scripts/cli-skills.py check target/debug/dots
	cargo nextest run -p dots-cli -p dots-tasks -p github-inspect -p github-client --all-targets
	cargo test -p dots-cli -p dots-tasks -p github-inspect -p github-client --doc

.PHONY: cli cli-build cli-check cli-skills rust-check

# Explicit, pinned test-only dependencies; herdr-check never downloads them.
herdr-test-deps:
	$(MAKE) -C $(HERDR_PLUGINS) test-deps

.PHONY: herdr-test-deps

herdr-issue-build:
	$(MAKE) -C $(HERDR_PLUGINS) tool-build-issue

# Plugin manifests and callers can still build one plugin from the root.
plugin-build-%:
	$(MAKE) -C $(HERDR_PLUGINS) $@

.PHONY: all stow unstow nvim skills skills-links mods-check skills-update skills-import pi-cli-install pi-cli-update pi-install pi-deps pi-check herdr herdr-build herdr-link herdr-check herdr-issue-build
.PHONY: gitsign
