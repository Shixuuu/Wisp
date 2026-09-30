# Wisp

A small, fast, quiet web browser for Arch Linux.

Wisp is a Rust rewrite of [Search](https://github.com/driceroland/Search), the
Mac browser by Office Commun. It keeps Search's idea (a row of tabs, one field,
and the page, with nothing else in the way) and moves it to Linux:

| | Search (upstream) | Wisp |
|---|---|---|
| Language | Swift | Rust |
| Web engine | Apple WebKit (`WKWebView`) | WebKitGTK 6.0 (`webkit6`) |
| Interface | SwiftUI + AppKit | GTK 4 + libadwaita |
| Platform | macOS 14+ | Linux, packaged for Arch |
| Files | `~/Library/Application Support/Search` | XDG dirs: `~/.local/share/wisp`, `~/.config/wisp`, `~/.cache/wisp` |
| Passwords | macOS keychain | Not yet (see below) |
| Updates | Self-updater | pacman / your AUR helper |

---

## Install on Arch

From a checkout of this repository:

```sh
cd packaging/arch
makepkg -si
```

This builds the `wisp-git` package from the latest commit and installs it with
pacman. Uninstall with `sudo pacman -R wisp-git`.

Or build and install by hand:

```sh
sudo pacman -S --needed base-devel rust webkitgtk-6.0 gtk4 libadwaita
make
sudo make install          # to /usr/local; `sudo make uninstall` removes it
```

For video and audio, install the GStreamer plugins you need:

```sh
sudo pacman -S gst-plugins-good gst-plugins-bad gst-libav
```

To make Wisp your default browser:

```sh
xdg-settings set default-web-browser org.wisp.Wisp.desktop
```

---

## What it does

- **One field.** Type an address and you go there; type words and you search.
  It finishes addresses from your history as you type and suggests open tabs,
  bookmarks and pages you've visited. Nothing you type leaves the machine until
  you press Enter.
- **Keywords.** `aw pacman` searches the Arch Wiki, `aur paru` the AUR,
  `pkg firefox` the official repos, plus `yt`, `gh`, `w` (Wikipedia) and
  `crate`. Add your own in `~/.config/wisp/settings.json`.
- **Tabs down the left or across the top.** `Ctrl+Shift+S` switches. `Ctrl+B`
  folds them away. Drag tabs to reorder. Right-click a tab to pin, duplicate,
  mute or close it. Pinned tabs shrink to their icon and always come back.
- **Tabs that cost nothing until you use them.** Tabs from last session come
  back as just a name; the page loads when you open it. Background tabs you
  haven't looked at in 30 minutes go to sleep and give their memory back
  (configurable, and never a tab that's playing sound).
- **An ad and tracker blocker that runs before the page.** The same rules as
  Search, compiled once into WebKit's content-blocker bytecode and enforced
  inside WebKit's networking. Turn it off per site if it breaks something.
  WebKit's Intelligent Tracking Prevention is on too.
- **Hide anything, for good.** `Ctrl+Shift+H`, then click a cookie banner, a
  newsletter box, a sidebar. It goes, and it stays gone on that site, applied
  as a user stylesheet before the page draws. `Ctrl+Shift+U` lists what's
  hidden and brings things back.
- **Reading mode.** `Ctrl+Alt+R` (or `F9`) strips a page down to the article.
  Follows dark mode.
- **Private tabs.** `Ctrl+Shift+N`. They share one in-memory cookie jar that
  is thrown away when the last private tab closes. Never written to history or
  to the session.
- **Bookmarks, history, downloads**: each a searchable panel one key away.
  Import bookmarks from Chromium, Chrome, Brave, Vivaldi or Edge in one click.
- **Light, dark, or the system's.** The frame follows your desktop's
  light/dark setting through libadwaita. Reading mode follows it too.

### What it doesn't do

On purpose, as with Search: no account, no sync, no telemetry, no start page,
no crash reports. One window.

### Not ported yet

Search is about 50,000 lines of Swift. This first version covers its core. These
parts of Search are **not** in Wisp yet:

| Feature | Why it's missing |
|---|---|
| Chrome extensions | WebKitGTK has no WebExtensions API like the one Search builds on (`WKWebExtension`, macOS 15.4+). Would need a large shim. |
| Saved passwords and passkeys | WebKitGTK has no autofill API. A version using the Secret Service (GNOME Keyring / KWallet) via `libsecret` is possible but not written. |
| Floating video (picture-in-picture) | WebKitGTK doesn't implement the PiP API. |
| Split view (two pages side by side) | Planned; a `gtk::Paned` with two views. |
| Spaces and tab groups | Planned. |
| AI assistant, AppleScript, Arc/Safari import | macOS-specific or out of scope. |
| Self-updater | Not needed: pacman handles updates. |

---

## Keyboard

`Ctrl+?` inside Wisp lists these too.

| Keys | Action | Keys | Action |
|---|---|---|---|
| `Ctrl+L`, `Alt+D`, `F6` | Address field | `Ctrl+K` | Switch to an open tab |
| `Ctrl+T` / `Ctrl+W` | New / close tab | `Ctrl+Shift+T` | Reopen closed tab |
| `Ctrl+Shift+N` | New private tab | `Ctrl+Shift+D` | Duplicate tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab | `Ctrl+1`…`Ctrl+9` | Jump to tab (9 = last) |
| `Alt+←` / `Alt+→` | Back / forward | `Ctrl+Alt+P` | Pin / unpin tab |
| `Ctrl+R`, `F5` | Reload | `Ctrl+Shift+R`, `Shift+F5` | Reload, bypassing the cache |
| `Ctrl+F`, `Ctrl+G` | Find, find next | `Ctrl+Alt+R`, `F9` | Reading mode |
| `Ctrl+Shift+H` | Hide something | `Ctrl+Shift+U` | What's hidden here |
| `Ctrl+D` | Bookmark page | `Ctrl+Shift+O` | Bookmarks |
| `Ctrl+H`, `Ctrl+Y` | History | `Ctrl+J` | Downloads |
| `Ctrl+Shift+S` | Tabs left / top | `Ctrl+B` | Fold tabs away |
| `Ctrl+Shift+C` | Copy address | `Ctrl+Shift+V` | Paste and go |
| `Ctrl +` / `Ctrl −` / `Ctrl+0` | Zoom | `F11` | Full screen |
| `F12`, `Ctrl+Shift+I` | Web inspector | `Ctrl+,` | Settings |

Middle-click or `Ctrl`+click a link to open it in a background tab. Middle-click
a tab to close it. Mouse back/forward buttons work.

**Where Wisp's keys differ from Search's**, to match what Linux browsers use:
bookmarking is `Ctrl+D` (Search uses `⇧⌘B`), `Ctrl+Shift+R` is a hard reload,
so reading mode moved to `Ctrl+Alt+R`; folding the sidebar is `Ctrl+B` rather
than `⌘S`, because `Ctrl+S` means save.

---

## Privacy, concretely

| What | Where it is | Who can read it |
|---|---|---|
| History, bookmarks, open tabs, hidden elements | Small JSON files in `~/.local/share/wisp/` | You. |
| Settings | `~/.config/wisp/settings.json` | You. |
| Cookies and site data | `~/.local/share/wisp/webkit/` | The sites that set them. Clear them in Settings › Privacy. |
| Cache and compiled block list | `~/.cache/wisp/` | Safe to delete at any time. |
| Private tabs | Memory only | Gone when the last private tab closes. |
| Anything else | Nowhere. There is no server. | — |

---

## Made for Arch

- **Uses your system's WebKit.** Wisp links against Arch's `webkitgtk-6.0`
  and gets its security fixes through `pacman -Syu`, rather than bundling an
  engine. The stripped release binary is about 1.2 MB.
- **Native Wayland** (and X11), through GTK 4. Touchpad swipe for back/forward.
- **Sandboxed web processes.** WebKitGTK runs each page in its own process
  inside a `bubblewrap` sandbox (a dependency of `webkitgtk-6.0`).
- **XDG everywhere.** Files go where the XDG spec says. Downloads go to your
  `xdg-user-dirs` Downloads folder. File pickers and "open file" go through
  `xdg-desktop-portal`, so they look native on GNOME, KDE and others.
- **Optimised release build.** Fat LTO, one codegen unit, `panic = abort`
  (see `Cargo.toml`). The PKGBUILD follows the Arch Rust packaging guidelines
  (`cargo fetch --locked`, `--frozen` builds, tests in `check()`) and respects
  your `makepkg.conf` `RUSTFLAGS`. For a build tuned to your own CPU, add
  `-C target-cpu=native` to `RUSTFLAGS` in `/etc/makepkg.conf`.

### Troubleshooting

- **Blank or flickering pages on NVIDIA's proprietary driver.** A known
  WebKitGTK issue. Run with `WEBKIT_DISABLE_DMABUF_RENDERER=1 wisp`, or turn
  off Settings › Performance › Hardware acceleration.
- **Videos don't play.** Install `gst-plugins-good`, `gst-plugins-bad` and
  `gst-libav`.
- **No spell checking.** Install a hunspell dictionary, e.g. `hunspell-en_us`.
- **Netflix, Disney+ and other DRM sites don't play.** WebKitGTK has no
  Widevine support. This is a WebKitGTK limitation, not something Wisp can fix.

---

## For developers

```sh
cargo run -- https://archlinux.org     # debug build
cargo test                             # unit tests for the non-GUI parts
cargo clippy --all-targets
```

Needs Rust 1.88+, GTK 4.14+, libadwaita 1.5+ and WebKitGTK 2.44+ (Arch ships
newer versions of all of them).

### How it's put together

One file per concern, as in Search:

| File | What it does | Search's equivalent |
|---|---|---|
| `main.rs` | Start-up, single instance, command line | `App.swift` |
| `browser.rs` | The window, layout, tabs, session | `Browser.swift`, `TabBar.swift`, `Side.swift` |
| `tab.rs` | One tab: lazy WebView, its row in the list | `Tab.swift` |
| `view.rs` | Building a WebView and handling its signals | `Tab.swift`, `Stage.swift` |
| `web.rs` | Network sessions, WebKit settings, block-list compile | `Engine.swift`, `Shield.swift` |
| `omnibox.rs` | The address field and its suggestions | `Omnibox.swift` |
| `address.rs` | Is it an address or a search? | `Address.swift` |
| `history.rs` | Visits and frecency ranking | `History.swift` |
| `bookmarks.rs` | Bookmarks and Chromium import | `Bookmarks.swift`, `Import.swift` |
| `shield.rs` | The ad-block rules (same as Search's) | `Shield.swift` |
| `curtain.rs`, `js/picker.js` | Hidden elements and the picker | `Curtain.swift` |
| `js/reader.js` | Reading mode | `Reader.swift` |
| `downloads.rs` | Downloads | `Fetching.swift` |
| `panels.rs` | History, bookmarks, downloads, tab switcher dialogs | `Recall.swift`, `TabSwitcher.swift` |
| `prefs.rs`, `settings.rs` | Settings window and stored settings | `Settings.swift`, `Prefs.swift` |
| `actions.rs` | Every command and its shortcut | `Shortcuts.swift` |

## License

MIT. Wisp carries over code, rules and design from Search, © Office Commun,
also MIT. Both notices are in [LICENSE](LICENSE).
