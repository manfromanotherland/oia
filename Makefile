# Root task runner for the Óia monorepo.
#
# Drives the core, extension, and macOS component toolchains from the repository
# root via scripts/components.sh.
#
#   make push            # git push the current monorepo branch once
#   make lint            # lint every component
#   make test            # unit tests in every component
#   make build           # build every component
#   make check           # lint + test (fast pre-push gate)
#   make ci              # lint + test + build
#   make status          # git status for the monorepo
#   make shortcut         # build, test, validate, and sign the capture helper
#   make shortcut-install # release and verify the installed capture helper
#   make COMPONENT=core test  # narrow a quality target to one component
#
# COMPONENT is optional for quality targets; leave it empty to hit all components.

RUN       := scripts/components.sh
COMPONENT ?=

.DEFAULT_GOAL := help
.PHONY: help status push pull fetch fmt lint test build check ci shortcut shortcut-install

help:   ## Show this runner's help and per-component command mapping
	@$(RUN) help
	@printf '\nShortcut release:\n  make shortcut          build, test, validate, and sign\n  make shortcut-install  release and verify the installed marker\n'

status: ## git status -sb for the monorepo
	@$(RUN) status

push:   ## git push the current monorepo branch (sets upstream)
	@$(RUN) push

pull:   ## git pull --ff-only for the monorepo
	@$(RUN) pull

fetch:  ## git fetch --all --prune for the monorepo
	@$(RUN) fetch

fmt:    ## Auto-format every component
	@$(RUN) fmt $(COMPONENT)

lint:   ## Lint every component
	@$(RUN) lint $(COMPONENT)

test:   ## Run unit tests in every component
	@$(RUN) test $(COMPONENT)

build:  ## Build every component
	@$(RUN) build $(COMPONENT)

check:  ## lint + test (fast pre-push gate)
	@$(RUN) check $(COMPONENT)

ci:     ## lint + test + build (full)
	@$(RUN) ci $(COMPONENT)

shortcut: ## Build, test, validate, and sign the capture helper
	@shortcuts/release-shortcut.sh

shortcut-install: ## Release and verify the capture helper; preserve Óia!
	@shortcuts/release-shortcut.sh --install
