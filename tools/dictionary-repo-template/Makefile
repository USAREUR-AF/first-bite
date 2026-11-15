# Gates for the {{NAME}} dictionary. `make check` is what a pull request must pass.

BITE ?= bite
DICTIONARY ?= dictionary
TABLES ?= tables
# A released version of the dictionary to compare with, for `make diff`.
OLD ?=

.PHONY: check fmt tables import diff

check: ## format, structure and vectors, conventions, coverage
	$(BITE) fmt --check $(DICTIONARY)
	$(BITE) check --strict $(DICTIONARY)
	$(BITE) lint $(DICTIONARY)
	$(BITE) coverage --strict $(DICTIONARY)

fmt: ## rewrite the dictionary in canonical form
	$(BITE) fmt $(DICTIONARY)

tables: ## dictionary -> tables, to edit in a spreadsheet
	$(BITE) export-csv $(DICTIONARY) --out $(TABLES)

import: ## tables -> dictionary
	$(BITE) import-csv $(TABLES) --into $(DICTIONARY)

diff: ## what changed against a released version: make diff OLD=<directory>
	@test -n "$(OLD)" || { echo "usage: make diff OLD=<directory of the released dictionary>"; exit 2; }
	$(BITE) diff --check-version $(OLD) $(DICTIONARY)
