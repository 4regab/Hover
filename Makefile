# Hover on Linux. (Windows: .\build.ps1.)
#   make               release build
#   make test          the workspace's tests
#   make package       .deb and tarball in dist/, at the version in Cargo.toml
#   make install       into $(DESTDIR)$(PREFIX) (default /usr/local; sudo for that)
#   make uninstall
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
PREFIX ?= /usr/local
# Installed as hover: Discord's rename (hoverai) is a Windows matter, and autostart
# entries and StartupWMClass (the binary's name) already say hover.
BIN := target/release/hoverai

.PHONY: all build test package install uninstall version

all: build

build:
	cargo build --release -p hover

test:
	cargo test --release --workspace

package: build
	sh packaging/linux/package-linux.sh $(VERSION) dist

install: build
	install -Dm755 $(BIN) $(DESTDIR)$(PREFIX)/bin/hover
	install -Dm644 app/assets/hover-mark.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/hover.png
	install -d $(DESTDIR)$(PREFIX)/share/applications
	{ sed 's|@EXEC@|$(PREFIX)/bin/hover|' packaging/linux/hover.desktop; echo 'X-AppVersion=$(VERSION)'; } > $(DESTDIR)$(PREFIX)/share/applications/hover.desktop
	@echo "Hover $(VERSION) installed in $(DESTDIR)$(PREFIX)"

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/hover $(DESTDIR)$(PREFIX)/share/applications/hover.desktop \
	  $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/hover.png

version:
	@echo $(VERSION)
