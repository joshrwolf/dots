PLUGIN_MANIFESTS := $(wildcard */herdr-plugin.toml)
PLUGIN_NAMES := $(notdir $(patsubst %/,%,$(dir $(PLUGIN_MANIFESTS))))
PLUGIN_BUILD_TARGETS := $(addprefix plugin-build-,$(PLUGIN_NAMES))

build: $(PLUGIN_BUILD_TARGETS)

$(PLUGIN_BUILD_TARGETS): plugin-build-%:
	@test -f "$*/herdr-plugin.toml" || { echo "plugin-build-$*: missing manifest" >&2; exit 1; }
	cargo build --release --package herdr-$*
	@built="target/release/herdr-$*"; \
		destination="$*/bin/herdr-$*"; \
		[ -x "$$built" ] || { echo "plugin-build-$*: missing executable $$built" >&2; exit 1; }; \
		mkdir -p "$*/bin"; \
		temporary="$$destination.tmp.$$$$"; \
		trap 'rm -f "$$temporary"' EXIT HUP INT TERM; \
		cp "$$built" "$$temporary"; \
		chmod +x "$$temporary"; \
		mv -f "$$temporary" "$$destination"; \
		trap - EXIT HUP INT TERM

link:
	@command -v herdr >/dev/null 2>&1 || { echo "link: herdr not installed, skipping"; exit 0; }
	@rc=0; for manifest in $(PLUGIN_MANIFESTS); do \
		plugin=$$(dirname "$$manifest"); \
		herdr plugin link "$(CURDIR)/$$plugin" >/dev/null || { echo "link: LINK FAILED for $$plugin" >&2; rc=1; }; \
	done; exit $$rc

check:
	$(MAKE) -n $(PLUGIN_BUILD_TARGETS) >/dev/null
	cargo fmt --all --check
	cargo clippy --workspace --all-features --all-targets -- -D warnings
	cargo nextest run --workspace --all-features --all-targets
	# nextest deliberately does not run rustdoc tests.
	cargo test --workspace --all-features --doc
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

.PHONY: build link check $(PLUGIN_BUILD_TARGETS)
