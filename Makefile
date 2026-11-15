.PHONY: all build serve editor web-check web-project web-deploy

PORT ?= 8878

all: build serve

build:
	bash scripts/build-web.sh

serve:
	node scripts/serve-web.mjs $(PORT)

editor:
	OPENCITY_LAUNCH_ACTION="$(CURDIR)/opencity-launch.json" cargo editor

web-check:
	cargo xtask web-check

web-project:
	cargo xtask web-project-create

web-deploy:
	cargo xtask web-deploy

.PHONY: assets fmt lint hooks

assets:
	cargo xtask assets

fmt:
	cargo xtask

lint:
	cargo xtask --check

hooks:
	@current=$$(git config --get core.hooksPath || true); \
	if [ -n "$$current" ] && [ "$$current" != .githooks ]; then \
		echo "Existing core.hooksPath=$$current; integrate .githooks/pre-commit there first."; exit 1; \
	fi
	git config core.hooksPath .githooks
