# Build and install Wisp. `make && sudo make install` puts it in /usr/local;
# the PKGBUILD uses `make install PREFIX=/usr DESTDIR="$pkgdir"`.

PREFIX  ?= /usr/local
DESTDIR ?=
BINDIR   = $(DESTDIR)$(PREFIX)/bin
DATADIR  = $(DESTDIR)$(PREFIX)/share
CARGO   ?= cargo

.PHONY: all build test install uninstall clean

all: build

build:
	$(CARGO) build --release --locked

test:
	$(CARGO) test --locked

install:
	install -Dm755 target/release/wisp "$(BINDIR)/wisp"
	install -Dm644 data/org.wisp.Wisp.desktop "$(DATADIR)/applications/org.wisp.Wisp.desktop"
	install -Dm644 data/org.wisp.Wisp.metainfo.xml "$(DATADIR)/metainfo/org.wisp.Wisp.metainfo.xml"
	install -Dm644 data/icons/org.wisp.Wisp.svg "$(DATADIR)/icons/hicolor/scalable/apps/org.wisp.Wisp.svg"
	install -Dm644 LICENSE "$(DATADIR)/licenses/wisp/LICENSE"

uninstall:
	rm -f "$(BINDIR)/wisp" \
	      "$(DATADIR)/applications/org.wisp.Wisp.desktop" \
	      "$(DATADIR)/metainfo/org.wisp.Wisp.metainfo.xml" \
	      "$(DATADIR)/icons/hicolor/scalable/apps/org.wisp.Wisp.svg"
	rm -rf "$(DATADIR)/licenses/wisp"

clean:
	$(CARGO) clean
