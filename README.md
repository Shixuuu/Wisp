# Wisp

A small, fast, quiet web browser for Arch Linux.

Wisp is a Rust rewrite of [Search](https://github.com/driceroland/Search), the
Mac browser by Office Commun. It keeps Search's look, motion and behaviour (a
column of tabs, one field, and the page, with nothing else in the way) and
moves it to Linux:

| | Search (upstream) | Wisp |
|---|---|---|
| Language | Swift | Rust |
| Web engine | Apple WebKit (`WKWebView`) | WebKitGTK 6.0 (`webkit6`) |
| Interface | SwiftUI + AppKit | GTK 4 + libadwaita, drawn to Search's design |
| Platform | macOS 14+ | Linux, packaged for Arch |
| Files | `~/Library/Application Support/Search` | XDG dirs: `~/.local/share/wisp`, `~/.config/wisp`, `~/.cache/wisp` |
| Updates | Self-updater | pacman / your AUR helper |

---

## Install on Arch

One command, which installs the dependencies, builds the package and installs
it with pacman:

```sh
curl -fsSL https://raw.githubusercontent.com/Shixuuu/Wisp/main/install.sh | bash
```

Run it again to upgrade. Uninstall with `sudo pacman -R wisp-git`.

Or from a checkout of this repository:

```sh
cd packaging/arch
makepkg -si
```

This builds the `wisp-git` package and installs it with pacman.

Or by hand:

```sh
sudo pacman -S --needed base-devel rust webkitgtk-6.0 gtk4 libadwaita
make
sudo make install          # to /usr/local; `sudo make uninstall` removes it
```

For video and audio: `sudo pacman -S gst-plugins-good gst-plugins-bad gst-libav`.
To make Wisp the default browser:
`xdg-settings set default-web-browser org.wisp.Wisp.desktop`.

---

## What it does

Everything here is checked by the end-to-end tests (see below).

- **One field.** A blank tab is just the field. Type an address and go; type
  words and search. It completes addresses from history inline, offers pages
  you've visited, and `Ctrl+K` jumps to an open tab. Keywords: `aw pacman`
  (Arch Wiki), `aur paru`, `pkg firefox`, and your own in `settings.json`.
- **Tabs down the side or across the top.** `Ctrl+Shift+S` switches, `Ctrl+S`
  folds the column away; touch the window's edge and it peeks back. Drag to
  reorder, drag the edge to resize. Click the tab you're on to type over its
  address. Right-click for pin, rename, duplicate, copy, mute, sleep, close.
- **Pins.** A pinned tab becomes a square with its letter or icon, keeps its
  home page, and comes back every launch.
- **`Ctrl+Tab` switcher.** Tap it to go back to the last tab; hold it for
  pictures of your recent tabs.
- **Tabs that cost nothing until used.** Last session's tabs come back as
  names; background tabs asleep after 30 minutes give their memory back.
- **Ad and tracker blocker** (Search's rules, compiled into WebKit's content
  blocker), off per site with one switch.
- **Hide anything for good.** `Ctrl+Shift+H`, click a cookie banner; it stays
  gone on that site. `Ctrl+Shift+U` lists what's hidden and brings it back.
- **Reading mode** (`Ctrl+Shift+R`), **find on page** (`Ctrl+F`), **zoom**
  remembered per site, **permission questions** in a small line at the bottom,
  remembered once answered.
- **Private tabs** (`Ctrl+Shift+N`). A new tab from a private tab is private
  too. Nothing is written to history or the session.
- **History, bookmarks, downloads and settings** as panels that slide over the
  page, each one key away. Chromium-family bookmarks import in one click.
- **Light, dark or the system's**, with Search's exact greys.

### Search features not in Wisp

Search is about 50,000 lines of Swift. Wisp ports the browser at its core, in
about 8,000 lines of Rust. These parts of Search are **not** ported:

| Feature | Why |
|---|---|
| Chrome extensions | WebKitGTK has no WebExtensions API like `WKWebExtension`. |
| Saved passwords, passkeys | WebKitGTK has no autofill API. Possible later via `libsecret`. |
| Floating video (picture-in-picture) | WebKitGTK doesn't implement the PiP API. |
| Split view, spaces, tab groups | Not written yet. |
| Link peek, the small "Little" window, bookmarks bar, site card, middle-button auto-scroll | Not written yet. |
| AI assistant, AppleScript, sharing, Arc/Safari import | macOS-specific or out of scope. |
| Self-updater, welcome and what's-new pages | pacman handles updates. |

On purpose, as with Search: no account, no sync, no telemetry, one window.

---

## Keyboard

Search's keys, with `Ctrl` for `⌘`. Settings › Shortcuts lists them too.

| Keys | Action | Keys | Action |
|---|---|---|---|
| `Ctrl+L` | Address field | `Ctrl+K` | Go to an open tab |
| `Ctrl+T` / `Ctrl+W` | New / close tab | `Ctrl+Shift+T` | Reopen closed tab |
| `Ctrl+Shift+N` | New private tab | `Ctrl+D` | Duplicate tab |
| `Ctrl+Tab` | Recent tabs | `Ctrl+Shift+[` / `]` | Previous / next tab |
| `Ctrl+1`…`Ctrl+9` | Jump to tab | `Ctrl+[` / `Ctrl+]` | Back / forward |
| `Ctrl+R`, `F5` | Reload | `Ctrl+Alt+R` | Reload, skipping the cache |
| `Ctrl+F`, `Ctrl+G` | Find, find next | `Ctrl+Shift+R` | Reading mode |
| `Ctrl+Shift+H` | Hide something | `Ctrl+Shift+U` | What's hidden here |
| `Ctrl+Shift+B` | Bookmark page | `Ctrl+Y` | History |
| `Ctrl+Shift+J` | Downloads | `Ctrl+,` | Settings |
| `Ctrl+Shift+S` | Tabs side / top | `Ctrl+S` | Fold tabs away |
| `Ctrl+Shift+C` | Copy address | `Ctrl+Shift+V` | Paste and go |
| `Ctrl +` / `−` / `0` | Zoom | `F11` / `F12` | Full screen / inspector |

---

## Privacy, concretely

| What | Where | Who can read it |
|---|---|---|
| History, bookmarks, tabs, hidden elements, downloads list | JSON files in `~/.local/share/wisp/` | You. |
| Settings | `~/.config/wisp/settings.json` | You. |
| Cookies and site data | `~/.local/share/wisp/webkit/` | The sites that set them. Clear in Settings › Privacy. |
| Cache, compiled block list | `~/.cache/wisp/` | Safe to delete. |
| Private tabs | Memory only | Gone when the last private tab closes. |

---

## Made for Arch

- Links against Arch's `webkitgtk-6.0`, so engine security fixes arrive with
  `pacman -Syu`.
- Native Wayland and X11 through GTK 4. Each page runs in WebKit's
  `bubblewrap` sandbox.
- XDG directories, `xdg-user-dirs` Downloads, file dialogs through
  `xdg-desktop-portal`.
- Release build with fat LTO, one codegen unit and `panic = abort`.

**Troubleshooting.** Blank pages on NVIDIA's proprietary driver: run with
`WEBKIT_DISABLE_DMABUF_RENDERER=1`, or turn off hardware acceleration in
Settings. No video: install the GStreamer plugins above. DRM sites (Netflix
and similar) don't play: WebKitGTK has no Widevine.

---

## For developers

```sh
cargo run -- https://archlinux.org
tests/e2e/run.sh                 # every end-to-end test
tests/e2e/run.sh find zoom       # just the ones whose names match
```

There are no unit tests. The tests drive the real browser on a virtual
screen: they type with `xdotool`, read the window through the accessibility
tree (AT-SPI), and load pages from a small local server posing as the web
(`news.test`, `shop.test`, a search engine, a download, a site that fails).
Each test starts Wisp with an empty home, so they don't touch yours. A
failing test saves a screenshot to `target/e2e/`.

They need `xorg-server-xvfb xdotool python-gobject at-spi2-core imagemagick`.
Building needs Rust 1.88+, GTK 4.14+, libadwaita 1.5+, WebKitGTK 2.44+.

| File | What it does |
|---|---|
| `main.rs` | Start-up, one instance, command line, colours |
| `browser.rs` | The window: tabs, the room the column takes, session, downloads |
| `tab.rs` | One tab and its WebView's signals |
| `tabs.rs` | The column and the strip: rows, pins, the live pill, dragging |
| `omnibox.rs` | The field and its suggestions |
| `bars.rs` | The line at the bottom, the link under the pointer, find |
| `panels.rs` | History, downloads, bookmarks, settings, hidden things |
| `switcher.rs` | `Ctrl+Tab` |
| `keys.rs` | Every key and menu action |
| `motion.rs` | Search's springs and fades |
| `web.rs`, `shield.rs`, `curtain.rs` | WebKit sessions, the blocker, hidden elements |
| `settings.rs`, `history.rs`, `bookmarks.rs`, `loot.rs`, `address.rs`, `store.rs` | What's kept, and how |

## License

MIT. Wisp carries over code, rules and design from Search, © Office Commun,
also MIT. Both notices are in [LICENSE](LICENSE).
