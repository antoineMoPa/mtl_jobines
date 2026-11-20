OWE_ROOT := ../..
GAME := breakwater
MAP_FILES := range.road ../../assets/fps/fps-rifle.glb ../../assets/fps/fps-handgun.glb
include $(OWE_ROOT)/sdk/owe/game.mk

.PHONY: app
app: native
	mkdir -p Breakwater.app/Contents/MacOS
	cp Info.plist Breakwater.app/Contents/
	cp breakwater $(NATIVE_LIB) Breakwater.app/Contents/MacOS/
	codesign --force --sign - Breakwater.app
