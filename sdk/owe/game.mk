# Include from a game directory after setting OWE_ROOT, GAME and MAP_FILES.
SDK := $(OWE_ROOT)/sdk/owe
NATIVE_DIR := $(OWE_ROOT)/target/owe-native
WEB_ROOT := $(OWE_ROOT)/target/owe-web
DIST := $(WEB_ROOT)/$(GAME)
WASM_CXX ?= /opt/homebrew/opt/llvm@20/bin/clang++
WASM_LD ?= /opt/homebrew/opt/lld@20/bin/wasm-ld
PORT ?= 8879
HEADERS := $(wildcard $(SDK)/include/owe/*)
NATIVE_LIB := $(NATIVE_DIR)/libowe.dylib
.PHONY: native run web serve native-engine web-engine
native: $(GAME) game.pack.json
$(GAME): game.cpp $(HEADERS) $(NATIVE_LIB) $(SDK)/game.mk
	$(CXX) -std=c++20 $(CXXFLAGS) -I$(SDK)/include $< -L$(NATIVE_DIR) -lowe \
		-Wl,-rpath,@executable_path -Wl,-rpath,$(abspath $(NATIVE_DIR)) -o $@
native-engine $(NATIVE_LIB):
	cd $(OWE_ROOT) && cargo rustc --locked -p mtl_jobines --lib --crate-type cdylib
	mkdir -p $(NATIVE_DIR)
	@set -e; tmp=$$(mktemp "$(NATIVE_DIR)/libowe.XXXXXX"); \
		cp -p "$(OWE_ROOT)/target/debug/libmtl_jobines.dylib" "$$tmp"; \
		install_name_tool -id @rpath/libowe.dylib "$$tmp"; \
		codesign --force --sign - "$$tmp"; \
		mv "$$tmp" "$(NATIVE_LIB)"
game.pack.json: game.json $(MAP_FILES) $(SDK)/pack.mjs
	node $(SDK)/pack.mjs $< $@
run: native
	./$(GAME) game.pack.json
web: $(WEB_ROOT)/.engine-ready $(DIST)/game.wasm $(DIST)/game.pack.json $(DIST)/index.html
web-engine $(WEB_ROOT)/.engine-ready: $(SDK)/stage-assets.mjs $(OWE_ROOT)/web/assets.list
	cd $(OWE_ROOT) && cargo rustc --locked --release -p mtl_jobines --target wasm32-unknown-unknown --lib --crate-type cdylib
	wasm-bindgen $(OWE_ROOT)/target/wasm32-unknown-unknown/release/mtl_jobines.wasm --target web --no-typescript --out-dir $(WEB_ROOT)/pkg
	node $(SDK)/stage-assets.mjs $(OWE_ROOT) $(WEB_ROOT)
	touch $(WEB_ROOT)/.engine-ready
$(DIST):
	mkdir -p $@
$(DIST)/game.pack.json: game.pack.json | $(DIST)
	cp $< $@
$(DIST)/index.html: $(SDK)/index.html | $(DIST)
	cp $< $@
$(DIST)/game.wasm: game.cpp $(HEADERS) $(SDK)/game.mk $(SDK)/wasm-imports | $(DIST)
	$(WASM_CXX) --target=wasm32-unknown-unknown -std=c++20 -ffreestanding -nostdlib \
		-fno-exceptions -fno-rtti -fno-threadsafe-statics -O2 -I$(SDK)/include \
		-fuse-ld=$(WASM_LD) -Wl,--no-entry -Wl,--export=owe_client_resume_v1 \
		-Wl,--allow-undefined-file=$(SDK)/wasm-imports $< -o $@
serve: web
	@echo "http://127.0.0.1:$(PORT)/$(GAME)/"
	python3 -m http.server $(PORT) --bind 127.0.0.1 --directory $(WEB_ROOT)
