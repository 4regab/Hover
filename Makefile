# Hover on Linux. (Windows: .\build.ps1.)
#   make               build ./hover-linux
#   make test          the Go tests (CI does not run them)
#   make package       .deb and tarball in dist/, at the version in VERSION
#   make install       into $(DESTDIR)$(PREFIX) (default /usr/local; sudo for that)
#   make uninstall
# The office draws with wgpu-native. `make wgpu` fetches libwgpu_native.so into lib/; or set
# WGPU_NATIVE_LIB to one you have. The build needs a C compiler and the EGL headers
# (libegl1-mesa-dev, libgles2-mesa-dev on Debian and Ubuntu): Gio draws through EGL.
VERSION := $(shell tr -d '\r' < VERSION | head -n 1)
PREFIX ?= /usr/local
MODULE := github.com/4regab/Hover
# Wayland only: Gio's own Wayland, X11 and Vulkan are left out (the window code speaks Wayland itself).
TAGS := nowayland,nox11,novulkan
WGPU_VERSION := v29.0.0.0
WGPU_NATIVE_LIB ?= lib/libwgpu_native.so
# Installed as hover: Discord's rename (hoverai) is a Windows matter, and autostart
# entries and StartupWMClass (the binary's name) already say hover.
BIN := hover-linux

.PHONY: all build test wgpu package install uninstall version

all: build

build:
	go build -tags $(TAGS) -ldflags "-X $(MODULE)/internal/shell.Version=$(VERSION)" -o $(BIN) ./cmd/hover

test:
	go test -tags $(TAGS) ./cmd/... ./internal/...

wgpu:
	curl -sL -o wgpu.zip https://github.com/gfx-rs/wgpu-native/releases/download/$(WGPU_VERSION)/wgpu-linux-x86_64-release.zip
	unzip -o -q wgpu.zip lib/libwgpu_native.so
	rm -f wgpu.zip

$(WGPU_NATIVE_LIB):
	$(MAKE) wgpu

package: build $(WGPU_NATIVE_LIB)
	WGPU_NATIVE_LIB=$(WGPU_NATIVE_LIB) sh packaging/linux/package-linux.sh $(VERSION) dist

install: build $(WGPU_NATIVE_LIB)
	install -Dm755 $(BIN) $(DESTDIR)$(PREFIX)/bin/hover
	install -Dm755 $(WGPU_NATIVE_LIB) $(DESTDIR)$(PREFIX)/lib/hover/libwgpu_native.so
	install -Dm644 internal/ui/assets/hover-mark.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/hover.png
	install -d $(DESTDIR)$(PREFIX)/share/applications
	{ sed 's|@EXEC@|$(PREFIX)/bin/hover|' packaging/linux/hover.desktop; echo 'X-AppVersion=$(VERSION)'; } > $(DESTDIR)$(PREFIX)/share/applications/hover.desktop
	@echo "Hover $(VERSION) installed in $(DESTDIR)$(PREFIX)"

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/hover $(DESTDIR)$(PREFIX)/share/applications/hover.desktop \
	  $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/hover.png $(DESTDIR)$(PREFIX)/lib/hover/libwgpu_native.so

version:
	@echo $(VERSION)
