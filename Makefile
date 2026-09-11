# project dirs, not stow packages (.stow-local-ignore does not exclude
# packages — stow only reads it from inside a package)
PROJECTS := tools herdr-plugins
PACKAGES := $(filter-out $(PROJECTS),$(patsubst %/,%,$(wildcard */)))
PLUGINS := herdr-plugins
PLUGIN_MANIFESTS := $(wildcard $(PLUGINS)/*/herdr-plugin.toml)
PLUGIN_NAMES := $(notdir $(patsubst %/,%,$(dir $(PLUGIN_MANIFESTS))))
PLUGIN_BUILD_TARGETS := $(addprefix plugin-build-,$(PLUGIN_NAMES))

# plugins-link runs last: a link failure must be loud rather than leaving
# config.toml bound to plugin actions that do not exist — ctrl+hjkl is
# unusable in that state — and it can only succeed once the binaries exist,
# because `[[build]]` in a manifest runs on install and never on link.
all: plugins-build mcp
	stow --verbose --target="$(HOME)" --restow $(PACKAGES)
	$(MAKE) plugins-link

# Each manifest builds only its package. The aggregate target composes those
# same primitives, and staging uses a same-directory rename so a running plugin
# can never observe a partially copied executable.
plugins-build: $(PLUGIN_BUILD_TARGETS)

$(PLUGIN_BUILD_TARGETS): plugin-build-%:
	@test -f "$(PLUGINS)/$*/herdr-plugin.toml" || { echo "plugin-build-$*: missing manifest" >&2; exit 1; }
	cargo build --release --manifest-path $(PLUGINS)/Cargo.toml --package herdr-$*
	@built="$(PLUGINS)/target/release/herdr-$*"; \
		destination="$(PLUGINS)/$*/bin/herdr-$*"; \
		[ -x "$$built" ] || { echo "plugin-build-$*: missing executable $$built" >&2; exit 1; }; \
		mkdir -p "$(PLUGINS)/$*/bin"; \
		temporary="$$destination.tmp.$$$$"; \
		trap 'rm -f "$$temporary"' EXIT HUP INT TERM; \
		cp "$$built" "$$temporary"; \
		chmod +x "$$temporary"; \
		mv -f "$$temporary" "$$destination"; \
		trap - EXIT HUP INT TERM

# Plugin registration lives in ~/.config/herdr/plugins.json, not in this repo,
# so a fresh machine needs this even though the plugin files are checked in.
# Re-linking an already-linked plugin succeeds, so this is idempotent.
plugins-link:
	@command -v herdr >/dev/null 2>&1 || { echo "plugins-link: herdr not installed, skipping"; exit 0; }
	@rc=0; for manifest in $(PLUGIN_MANIFESTS); do \
		p=$$(dirname "$$manifest"); \
		herdr plugin link "$(CURDIR)/$$p" >/dev/null || { echo "plugins-link: LINK FAILED for $$p" >&2; rc=1; }; \
	done; exit $$rc

# Claude Code reads MCP servers from ~/.claude.json, never from settings.json,
# so the codex skill needs this on a fresh machine even though the skill itself
# is stowed. `claude mcp add` exits 1 when the server already exists.
mcp:
	@command -v claude >/dev/null 2>&1 || { echo "mcp: claude not installed, skipping"; exit 0; }
	@claude mcp get codex >/dev/null 2>&1 || claude mcp add -s user codex -- codex mcp-server

check:
	$(MAKE) -n $(PLUGIN_BUILD_TARGETS) >/dev/null
	cd $(PLUGINS) && cargo fmt --all --check
	cd $(PLUGINS) && cargo clippy --workspace --all-features --all-targets -- -D warnings
	cd $(PLUGINS) && cargo nextest run --workspace --all-features --all-targets
	# nextest deliberately does not run rustdoc tests.
	cd $(PLUGINS) && cargo test --workspace --all-features --doc
	cd $(PLUGINS) && RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
	@for file in nvim/.config/nvim/lua/herdr_review/*.lua nvim/.config/nvim/lua/herdr_review/tests/*.lua; do luac -p "$$file" || exit 1; done
	HERDR_REVIEW_NVIM_CONFIG="$(CURDIR)/nvim/.config/nvim" nvim --headless -u NONE -l nvim/.config/nvim/lua/herdr_review/tests/smoke.lua
	HERDR_REVIEW_NVIM_CONFIG="$(CURDIR)/nvim/.config/nvim" nvim --headless -u NONE -l nvim/.config/nvim/lua/herdr_review/tests/pages.lua
	cd $(PLUGINS) && cargo build --package herdr-review
	@temporary=$$(mktemp -d); trap 'rm -rf "$$temporary"' EXIT HUP INT TERM; \
		git init --quiet --initial-branch=main "$$temporary/checkout"; \
		touch "$$temporary/herdr.sock"; \
		HERDR_SOCKET_PATH="$$temporary/herdr.sock" \
		HERDR_WORKSPACE_ID="review-smoke-workspace" \
		HERDR_PANE_ID="review-smoke-pane" \
		HERDR_REVIEW_BINARY="$(CURDIR)/$(PLUGINS)/target/debug/herdr-review" \
		HERDR_REVIEW_CHECKOUT="$$temporary/checkout" \
		HERDR_REVIEW_NVIM_CONFIG="$(CURDIR)/nvim/.config/nvim" \
		nvim --headless -u NONE -l nvim/.config/nvim/lua/herdr_review/tests/smoke.lua

delete:
	stow --verbose --target="$(HOME)" --delete $(PACKAGES)

.PHONY: all plugins-build plugins-link $(PLUGIN_BUILD_TARGETS) mcp check delete
