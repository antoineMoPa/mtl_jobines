.PHONY: all build serve editor

PORT ?= 8878

all: build serve

build:
	bash scripts/build-web.sh

serve:
	node scripts/serve-web.mjs $(PORT)

editor:
	OPENCITY_LAUNCH_ACTION="$(CURDIR)/opencity-launch.json" cargo run --manifest-path opencity/Cargo.toml --bin editor
