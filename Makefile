STOW_PACKAGES := aerospace brew codex ghostty skills theme zsh
HERDR_STOW_PACKAGES := herdr nvim
HERDR_PLUGINS := herdr-plugins

all: stow

stow:
	stow --verbose --target="$(HOME)" --restow $(STOW_PACKAGES)

unstow:
	stow --verbose --target="$(HOME)" --delete $(STOW_PACKAGES)

nvim:
	stow --verbose --target="$(HOME)" --restow nvim

skills:
	stow --verbose --target="$(HOME)" --restow skills

# Preserve the required setup order: binaries, config, then registration.
herdr: herdr-build
	stow --verbose --target="$(HOME)" --restow $(HERDR_STOW_PACKAGES)
	$(MAKE) -C $(HERDR_PLUGINS) link

herdr-build:
	$(MAKE) -C $(HERDR_PLUGINS) build

herdr-link:
	$(MAKE) -C $(HERDR_PLUGINS) link

herdr-check:
	$(MAKE) -C $(HERDR_PLUGINS) check

herdr-issue-build:
	$(MAKE) -C $(HERDR_PLUGINS) tool-build-issue

# Plugin manifests and callers can still build one plugin from the root.
plugin-build-%:
	$(MAKE) -C $(HERDR_PLUGINS) $@

.PHONY: all stow unstow nvim skills herdr herdr-build herdr-link herdr-check herdr-issue-build
