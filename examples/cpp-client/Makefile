ROOT := ../..
NATIVE_DIR := $(ROOT)/target/debug
WEB_DIR := $(ROOT)/web/dist
WASM_CXX ?= /opt/homebrew/opt/llvm@20/bin/clang++
WASM_LD ?= /opt/homebrew/opt/lld@20/bin/wasm-ld
PORT ?= 8878

.PHONY: native run web serve native-engine web-engine

native: mtl-cpp

mtl-cpp: client.cpp mtl_engine.h Makefile $(NATIVE_DIR)/libmtl_jobines.dylib
	$(CXX) -std=c++17 $(CXXFLAGS) $< -L$(NATIVE_DIR) -lmtl_jobines \
		-Wl,-rpath,@executable_path/$(NATIVE_DIR) -o $@

native-engine $(NATIVE_DIR)/libmtl_jobines.dylib:
	cd $(ROOT) && cargo rustc --locked -p mtl_jobines --lib --crate-type cdylib

run: native
	cd $(ROOT) && ./examples/cpp-client/mtl-cpp

web: $(WEB_DIR)/vehicle-return.wasm

$(WEB_DIR)/vehicle-return.wasm: client.cpp mtl_engine.h Makefile $(WEB_DIR)/pkg/mtl_jobines.js
	$(WASM_CXX) --target=wasm32-unknown-unknown -ffreestanding -nostdlib -O2 \
		-fuse-ld=$(WASM_LD) -Wl,--no-entry -Wl,--export=mtl_vehicle_return_v1 $< -o $@

web-engine $(WEB_DIR)/pkg/mtl_jobines.js:
	cd $(ROOT) && bash scripts/build-web.sh

serve: web
	@echo "C++ client: http://127.0.0.1:$(PORT)/game/?client=cpp"
	node $(ROOT)/scripts/serve-web.mjs $(PORT)
