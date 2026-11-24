.PHONY: all build serve editor web-check web-project web-deploy test-web test-web-only

PORT ?= 8878

all: build serve

build:
	bash scripts/build-web.sh

serve:
	node scripts/serve-web.mjs $(PORT)

editor:
	OPENCITY_LAUNCH_ACTION="$(CURDIR)/opencity-launch.json" cargo editor

# Build the web game, then load it in Chrome (Playwright) and check that all
# shaders compile and the first frame renders. Use test-web-only to reuse web/dist.
test-web: build test-web-only

test-web-only:
	tests/web/run.sh

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

.PHONY: view_metrics
view_metrics:
	node scripts/view-metrics.mjs
