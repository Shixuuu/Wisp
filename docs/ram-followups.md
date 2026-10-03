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

- **Background tabs wait until selected.** `Prefs::default` sets `lazy_tabs` to true (`src/settings.rs`). A saved settings file that already says false is left alone. A missing key still deserializes as false. Commit `3cd1e88`.
- **Visited pins can sleep.** The 60 second sleeper no longer skips a pin (`Browser::start_timers` in `src/browser.rs`). The active tab, sound, and a page in the split still stay up. The settings sentence no longer says pins stay awake. Commit `1cc71b4`.
- **Sleep drops the switcher picture.** `sleep` clears `Tab.preview`, and `wake` calls `capture` after `stage_add` (`src/tab.rs`). Commit `ceeb141`.
- **A failed download leaves the list.** `connect_failed` removes the `Fetch` and still announces a real failure (`src/browser.rs`). Commit `c13255a`.
- **The split map handler runs once.** `Slot` stores the handler id and disconnects it on fire, on the next `go`, and on detach (`src/panes.rs`). Commit `79afdd4`.

## Action items

None.

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
