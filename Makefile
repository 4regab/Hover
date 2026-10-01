# Hover on Linux. (Windows: .\build.ps1.)
#   make               release build
#   make test          the workspace's tests
#   make package       .deb and tarball in dist/, at the version in native/Cargo.toml
#   make install       into $(DESTDIR)$(PREFIX) (default /usr/local; sudo for that)
#   make uninstall
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' native/Cargo.toml | head -1)
PREFIX ?= /usr/local
# Installed as hover: Discord's rename (hoverai) is a Windows matter, and autostart
# entries and StartupWMClass (the binary's name) already say hover.
BIN := native/target/release/hoverai

.PHONY: all build test package install uninstall version

all: build

build:
	cargo build --manifest-path native/Cargo.toml --release -p hover

test:
	cargo test --manifest-path native/Cargo.toml --release --workspace

package: build
	sh native/installer/package-linux.sh $(VERSION) dist

install: build
	install -Dm755 $(BIN) $(DESTDIR)$(PREFIX)/bin/hover
	install -Dm644 native/apps/hover/assets/hover-mark.png $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/hover.png
	install -d $(DESTDIR)$(PREFIX)/share/applications
	printf '%s\n' '[Desktop Entry]' 'Type=Application' 'Name=Hover' 'Comment=Agent office in a notch at the top of the screen' \
	  'Exec=$(PREFIX)/bin/hover' 'Icon=hover' 'Terminal=false' 'Categories=Development;Utility;' 'StartupWMClass=hover' \
	  'X-AppVersion=$(VERSION)' > $(DESTDIR)$(PREFIX)/share/applications/hover.desktop
	@echo "Hover $(VERSION) installed in $(DESTDIR)$(PREFIX)"

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/hover $(DESTDIR)$(PREFIX)/share/applications/hover.desktop \
	  $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps/hover.png

version:
	@echo $(VERSION)
