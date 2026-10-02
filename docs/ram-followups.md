# RAM and lifecycle follow-ups

Recorded 2026-10-03 at `f07409a`. Findings verified against source by an
independent grok-4.7 review; line numbers are from that pass.

The original report was that repeatedly closing and reopening tabs piles up
WebKit instances and RAM, and that spamming `Ctrl+W` on a pinned tab closes
it, then reopens it, piling up further.

## Fixed

- **Pinned `Ctrl+W` rebuilt the pin on every press.** The loop was not in the
  pin branch: that branch already slept the pin and picked only an awake tab.
  The bug was the next `Ctrl+W`. Closing the fallback blank tab took
  `CloseOutcome::Remove` and selected `tabs[index]`, the sleeping pin, and
  `select` woke it into a new `WebView`. `Browser::close` now wraps around the
  closed index and accepts a tab only when it is awake or unpinned, so a
  sleeping pin fails both checks and a blank opens instead (`src/browser.rs:814-823`).
  Repeating `Ctrl+W` replaces the blank; it does not build another view.
- **Pane focus handlers accumulated.** `Panes::attach` connected a `has-focus`
  handler on every reattach, and `detach` only dropped the ID from a vector
  without disconnecting the signal. The handler ID is stored
  (`src/panes.rs:126`) and disconnected on detach and sleep
  (`src/panes.rs:210-213`, `src/tab.rs:235`).

Commits: `260cc36`, `f07409a`.

## Action items

### 1. Default `lazy_tabs` to `true` (highest impact)

`lazy_tabs` defaults to `false` (`src/settings.rs:151`). A background open —
middle-click or Ctrl+click (`src/tab.rs:388-393`), bookmark middle-click
(`src/panels.rs:1420`), or Ctrl+Return when not forced forward
(`src/omnibox.rs:827-829`) — hits the eager branch at `src/browser.rs:718-722`
and calls `build`, `load_uri`, `stage_add`. `build` (`src/tab.rs:157-206`)
creates a `UserContentManager`, both script handlers, and a `WebView`, and
`listen` connects the page signals. That view then sits in the `GtkStack`
until close or the sleeper.

With `lazy_tabs = true` the branch is skipped; the tab keeps its URL with no
view (`src/tab.rs:105-107`) until it is selected and `wake` builds exactly one.
The lazy path already exists and is skipped only because of the default.

### 2. Let visited pins sleep

The 60-second sleeper (`src/browser.rs:1479-1505`) only sleeps a tab that is
inactive, unpinned (`src/browser.rs:1492`), not playing audio, not in the
split, and idle for 30 minutes. A pin you have opened therefore stays a live
`WebView` until `Ctrl+W`. This is the main way WebKit processes survive after
the close-loop fix.

Change: include pinned tabs in the sleeper, or sleep a pin when it stops being
the active tab. Waking a selected pin already goes through `wake`
(`src/tab.rs:209-223`), so selection is unaffected.

### 3. Clear the switcher preview on sleep

Nothing ever clears `Tab.preview`. It is written only in `capture`
(`src/tab.rs:501-511`) and read only in the switcher (`src/switcher.rs:234`).
`select` snapshots the tab being left (`src/browser.rs:642`) and the switcher
snapshots the active tab (`src/switcher.rs:188-190`), so every tab you leave
keeps a texture. `sleep` (`src/tab.rs:229-241`) drops the view, the content
manager, and `tuned`, and leaves `preview` alone; a sleeping pin keeps it for
the life of the `Tab`. Snapshots wider than 480 px are scaled
(`src/tab.rs:518-533`), and the full texture is kept when `shrink` fails.

This is UI-process RAM, not extra WebKit processes. Change: drop `preview` in
`sleep` and re-capture on wake. `tab.icon` is also kept across sleep but is
small; lower priority.

### 4. Remove failed downloads

`src/browser.rs:1157-1166` keeps a failed `Fetch` forever. Cancelled and
finished ones are removed. This grows `Download` objects, not `WebView`s, so
it is not the instance leak — but it is unbounded.

### 5. Disconnect the `map` handler in `Slot::go`

`Slot::go` connects `map` and never disconnects it (`src/panes.rs:100-110`).
The closure owns `Tween`s, and those strong-reference the shell
(`src/motion.rs:64-69`, `src/panes.rs:47-71`). The cycle exists only while the
pane is unmapped. It leaks GTK widgets, not web processes; medium confidence
it fires in steady use.

## Closed: page cache is not the cause

`CacheModel::WebBrowser` (`src/web.rs:38`) and the page cache
(`src/web.rs:151`) were suspected of amplifying repeated recreation. Both
match WebKitGTK's own defaults, and the page cache belongs to a live view — it
dies when `sleep` drops the `WebView`. Neither allocates a view per
navigation. Do not re-investigate this as a cause of growing process count.

## Notes

- Ordinary close does release the view: `sleep` takes `tab.view`, unparents it
  from the stack or pane (`src/browser.rs:544-549`), disconnects the pane
  handler, and drops the content manager (`src/tab.rs:229-237`). Signal
  closures hold `Weak<Browser>` and look tabs up by id, so there is no Rust
  strong-reference cycle on that path. Any residual RSS climb after close is
  WebKit process-cache lag, which is unproven and outside this code.
- Ghosts are bounded by `MAX_GHOSTS = 25` (`src/browser.rs:33`) and hold three
  fields, not a view. Tab-chrome removal keeps one in-flight widget per close
  (`src/tabs.rs:328-343`).
- No runtime RSS or process-count measurement backs these items; they are
  source traces. WebKit shares processes and tears them down asynchronously,
  so process count and RSS alone cannot prove or disprove object retention.
