PYTHON := python3
PACKAGES := ../scripts/pi-packages.py
# Package checks use development Node, not Pi's private runtime on PATH.
NODE_PREFIX ?= $(shell brew --prefix node 2>/dev/null)
ifneq ($(strip $(NODE_PREFIX)),)
export PATH := $(NODE_PREFIX)/bin:$(PATH)
endif

# No default installation or activation.
check: check-node check-manager
	$(PYTHON) $(PACKAGES) check

check-node:
	node -e 'if (Number(process.versions.node.split(".")[0]) < 24) { console.error("Pi package checks require Node 24+; set NODE_PREFIX to its installation prefix."); process.exit(1); }'

check-manager:
	$(PYTHON) -m unittest discover -s tests -v

install:
	$(PYTHON) $(PACKAGES) install

deps:
	$(PYTHON) $(PACKAGES) deps

.PHONY: check check-node check-manager install deps
