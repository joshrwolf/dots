PYTHON := python3
PACKAGES := ../scripts/pi-packages.py

# No default installation or activation.
check: check-manager
	$(PYTHON) $(PACKAGES) check

check-manager:
	$(PYTHON) -m unittest discover -s tests -v

install:
	$(PYTHON) $(PACKAGES) install

deps:
	$(PYTHON) $(PACKAGES) deps

import:
	$(PYTHON) $(PACKAGES) import "$(SOURCE)"

update:
	$(PYTHON) $(PACKAGES) update "$(SOURCE)"

.PHONY: check check-manager install deps import update
