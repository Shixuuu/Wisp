"""Wisp, end to end: every test starts the real browser with a fresh home
and drives it with keys and clicks. Run with tests/e2e/run.sh."""

import os
import subprocess
import sys
import time
import traceback

from harness import App, Failure, Server, wait, ARTIFACTS

TESTS = []


def test(fn):
    TESTS.append(fn)
    return fn


def check(what, ok):
    if not ok:
        raise Failure(what)


def tab_titles(app):
    """The titles in the tab list, top to bottom."""
    labels = app.nodes(role="label")
    known = {"New tab"}
    return [n.get_name() for n in labels if n.get_name() and n.get_name() not in known and app.box(n)[0] < 240]


def shown(app, host):
    return wait(f"{host} to load", lambda: app.server.report(host), 12)


PAGES = [
    ("http://news.test", "news.test", "Daily News", "N"),
    ("http://shop.test", "shop.test", "Corner Shop", "S"),
    ("http://docs.test", "docs.test", "Arch Docs", "D"),
    ("http://blog.test", "blog.test", "A Blog", "B"),
]


def load(app, url, host):
    app.go(url)
    shown(app, host)
    wait(f"the {host} favicon", lambda: app.server.asked(host, "/favicon.ico"), 8)


def crop(app, name):
    evidence = os.environ.get("WISP_EVIDENCE")
    if not evidence:
        return
    app._xwindow()
    w, h = getattr(app, "size", (0, 0))
    if w < 100:
        return
    x, y = app.origin
    path = os.path.join(evidence, f"{name}.png")
    subprocess.run(["grim", "-g", f"{int(x)},{int(y)} {int(w)}x{int(h)}", path], check=False)
    print(f"    crop {path}", flush=True)


# MARK: searching and going places


@test
def searching_from_a_blank_tab(app):
    app.see("the field on a blank tab", role="text")
    app.type("arch linux wiki")
    app.key("Return", pause=1)
    wait("the search to reach the engine", lambda: app.server.asked("search.test", "/"), 10)
    host, path, query = app.server.asked("search.test", "/")[0]
    check(f"the words were sent: {query}", "arch+linux+wiki" in query)
    wait("the results' title", lambda: app.title() == "Results for arch linux wiki")
    check("the tab shows the page's title", "Results for arch linux wiki" in tab_titles(app))


@test
def going_to_an_address(app):
    app.type("http://news.test")
    app.key("Return")
    state = shown(app, "news.test")
    check("the page loaded", state["path"] == "/")
    wait("the title", lambda: app.title() == "Daily News")
    app.quit()
    check("remembered in history", any("news.test" in v["url"] for v in (app.read("history.json") or [])))


@test
def suggestions_and_inline_completion(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.go("http://docs.test")
    shown(app, "docs.test")
    app.key("ctrl+l")
    app.type("new")
    app.see("the place in the list", role="label", name="news.test")
    app.key("Return", pause=1)
    wait("going to the completed address", lambda: app.title() == "Daily News")


@test
def keyword_search(app):
    app.type("aw pacman")
    app.see("the keyword's row", role="label", name="wiki.archlinux.org")
    app.key("Escape")


@test
def switching_to_an_open_page_with_ctrl_k(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+t")
    app.go("http://shop.test")
    shown(app, "shop.test")
    app.key("ctrl+k")
    app.see("the open page offered", role="label", name="Daily News")
    app.key("Return", pause=0.8)
    wait("back on the news", lambda: app.title() == "Daily News")


# MARK: tabs


@test
def new_close_and_reopen_tabs(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+t")
    app.go("http://shop.test")
    shown(app, "shop.test")
    check("two tabs", {"Daily News", "Corner Shop"} <= set(tab_titles(app)))
    app.key("ctrl+w")
    wait("the shop closed", lambda: "Corner Shop" not in tab_titles(app))
    app.key("ctrl+shift+t", pause=1)
    wait("the shop back", lambda: "Corner Shop" in tab_titles(app))


@test
def jumping_and_stepping_between_tabs(app):
    for host in ("news.test", "shop.test", "docs.test"):
        app.go(f"http://{host}")
        shown(app, host)
        app.key("ctrl+t")
    app.key("ctrl+1")
    wait("tab 1", lambda: app.title() == "Daily News")
    app.key("ctrl+shift+bracketright")
    wait("the next tab", lambda: app.title() == "Corner Shop")
    app.key("ctrl+9")
    wait("the last tab", lambda: app.title() == "New Tab")


@test
def ctrl_tab_switcher(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+t")
    app.go("http://shop.test")
    shown(app, "shop.test")
    app.focus()
    app.hold("ctrl")
    app.key("Tab", pause=0.6, clear=False)
    wait("the switcher's card for the news", lambda: [n for n in app.nodes(role="label", name="Daily News") if app.box(n)[0] > 300])
    app.shot("switcher")
    app.release("ctrl")
    wait("back to the news", lambda: app.title() == "Daily News")


@test
def clicking_tabs_in_the_sidebar(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+t")
    app.go("http://shop.test")
    shown(app, "shop.test")
    app.press("the news tab", role="label", name="Daily News")
    wait("the news", lambda: app.title() == "Daily News")
    app.press("new tab row", role="push button", name="New tab")
    wait("a blank tab", lambda: app.title() == "New Tab")


@test
def pinning_a_tab(app):
    load(app, "http://news.test", "news.test")
    app.see("the site icon before pinning", role="image", name="Site icon")
    app.key("ctrl+shift+p")
    wait("the pin written down", lambda: any(t.get("pin") == "N" for t in (app.read("session.json") or {}).get("tabs", [])))
    app.see("the pin keeps its site icon", role="image", name="Site icon")
    check("a pin draws no letter", not app.has(role="label", name="N"))
    check("a pin draws no title", not app.has(role="label", name="Daily News"))


@test
def pinned_ctrl_w_does_not_reopen_the_pin(app):
    load(app, "http://news.test", "news.test")
    app.key("ctrl+shift+p")
    wait("the pin written down", lambda: any(t.get("pin") == "N" for t in (app.read("session.json") or {}).get("tabs", [])))
    app.key("ctrl+t")
    wait("the blank fallback tab", lambda: app.title() == "New Tab")
    pin = app.see("the pinned site icon", role="image", name="Site icon")
    app.click_node(pin)
    wait("the pinned page", lambda: app.title() == "Daily News")
    app.key("ctrl+w")
    wait("the blank fallback after closing the pin", lambda: app.title() == "New Tab")
    app.key("ctrl+w")
    wait("a replacement blank tab", lambda: app.title() == "New Tab")
    check("the pin remains in the session", any(t.get("pin") == "N" for t in (app.read("session.json") or {}).get("tabs", [])))


def pin_grid(app):
    """Four loaded pins in two rows of two, then one ordinary row.

    Returns the icon boxes. Each pin is a horizontal rectangle, so the two
    icons of a row sit side by side and the second row sits below them.
    """

    def ready():
        icons = app.nodes(role="image", name="Site icon")
        if len(icons) != 4:
            return None
        boxes = sorted((app.box(n) for n in icons), key=lambda b: (b[1], b[0]))
        top = [b for b in boxes if abs(b[1] - boxes[0][1]) <= 12]
        floor = boxes[0][1] + max(boxes[0][3] * 0.6, 16)
        bottom = [b for b in boxes if b[1] >= floor]
        if len(top) != 2 or len(bottom) != 2:
            return None
        if min(b[1] for b in bottom) < max(b[1] + b[3] for b in top) - 4:
            return None
        for row in (top, bottom):
            row = sorted(row, key=lambda b: b[0])
            if row[1][0] < row[0][0] + row[0][2] + 2:
                return None
        loose = app.nodes(role="label", name="New Tab")
        if not loose:
            return None
        pin_bottom = max(b[1] + b[3] for b in boxes)
        if app.box(loose[0])[1] < pin_bottom + 6:
            return None
        return boxes

    return wait("a 2 by 2 pin grid", ready, 4)


@test
def four_pins_sit_in_two_rows(app):
    for i, (url, host, title, letter) in enumerate(PAGES):
        if i:
            app.key("ctrl+t")
        load(app, url, host)
        app.key("ctrl+shift+p")
    app.key("ctrl+t")
    boxes = pin_grid(app)
    print(f"    pins {boxes}", flush=True)
    for _, _, title, letter in PAGES:
        check(f"no pin letter {letter}", not app.has(role="label", name=letter))
        check(f"no pin title {title}", not app.has(role="label", name=title))
    crop(app, "pins")


@test
def a_group_folds_to_three_icons(app):
    for i, (url, host, title, _) in enumerate(PAGES):
        if i:
            app.key("ctrl+t")
        load(app, url, host)
        app.see(f"the {host} icon", role="image", name="Site icon")
        app.key("ctrl+shift+g")
    app.see("the open group", name="Collapse group")
    for _, _, title, _ in PAGES:
        app.see(f"the member {title}", role="label", name=title)
    app.press("collapse", name="Collapse group")
    wait("the group folded", lambda: app.has(name="Expand group"))

    def folded():
        icons = app.nodes(role="image", name="Group icon")
        if len(icons) != 3:
            return None
        if not app.has(role="label", name="+"):
            return None
        for _, _, title, _ in PAGES:
            if app.has(role="label", name=title):
                return None
        return icons

    wait("three icons and a plus", folded, 4)

    def selection_on_folder():
        headers = [n for n in app.nodes(name="Expand group") if "button" in (n.get_role_name() or "")]
        marks = app.nodes(name="Current tab")
        if not headers or len(marks) != 1:
            return None
        _, hy, _, hh = app.box(headers[0])
        _, y, w, h = app.box(marks[0])
        # A row-sized bar under the folder is the old selection left behind.
        if y >= hy + hh - 2 or w < 80 or h < 18:
            return None
        if abs(y - hy) > 8 or abs(h - hh) > 8:
            return None
        return True

    wait("the selection sits on the folder", selection_on_folder, 4)
    crop(app, "group")
    app.press("expand", name="Expand group")

    def shop_row():
        rows = app.nodes(role="label", name="Corner Shop")
        headers = [n for n in app.nodes(name="Collapse group") if "button" in (n.get_role_name() or "")]
        if not rows or not headers:
            return None
        _, ry, _, _ = app.box(rows[0])
        _, hy, _, hh = app.box(headers[0])
        # The row has to finish its slide out from under the folder.
        if ry < hy + hh + 4:
            return None
        return rows[0]

    row = wait("the shop row below the group", shop_row, 4)
    rx, ry, rw, rh = app.box(row)
    app.pointer_click(rx + rw // 2, ry + rh // 2)
    wait("the shop is showing", lambda: app.title() == "Corner Shop", 6)


def pane_nodes(app):
    """The split-pane labels, found without entering a page document.

    The page column is the window's first child. Walking it first reaches
    the panes before the sidebar, so a read during the glide still sees it.
    """
    found = []

    def walk(node, depth=0):
        if depth > 30 or node is None or len(found) >= 3:
            return
        try:
            role = node.get_role_name() or ""
            name = node.get_name() or ""
            count = node.get_child_count()
        except Exception:
            return
        if "document" in role:
            return
        if name == "Split pane":
            found.append(node)
        for i in range(count):
            if len(found) >= 3:
                return
            try:
                walk(node.get_child_at_index(i), depth + 1)
            except Exception:
                pass

    walk(app._frame())
    return found


def pane_boxes(app, nodes=None):
    nodes = pane_nodes(app) if nodes is None else nodes
    return sorted((app.box(n) for n in nodes), key=lambda b: (b[0], b[1]))


@test
def dragging_a_tab_splits_the_page(app):
    load(app, "http://news.test", "news.test")
    app.key("ctrl+t")
    load(app, "http://shop.test", "shop.test")
    # The third page has to exist before the split. Opening it afterwards
    # selects it, and selecting a tab that is not in the split closes the split.
    app.key("ctrl+t")
    load(app, "http://docs.test", "docs.test")
    shop = app.see("the shop row", role="label", name="Corner Shop")
    sx, sy, sw, sh = app.box(shop)
    app.pointer_click(sx + sw // 2, sy + sh // 2)
    wait("the shop is the page", lambda: app.title() == "Corner Shop", 6)
    news = app.see("the news row", role="label", name="Daily News")
    _, _, window_w, window_h = app.box(app._frame())
    side = int(app.prefs().get("side_width", 232))
    start_w = window_w - side
    left_x = side + 48
    right_x = side + max((window_w - side) * 3 // 4, 80)
    y = max(window_h // 2, 180)
    app.drag(news, left_x, y)
    # Find the panes once, then re-read those same rectangles through the glide.
    nodes = []
    seen = []
    deadline = time.perf_counter() + 0.5
    while time.perf_counter() < deadline:
        if len(nodes) != 2:
            nodes = pane_nodes(app)
        if len(nodes) == 2:
            seen.append(pane_boxes(app, nodes))
        if len(seen) >= 8:
            break
        time.sleep(0.012)
    time.sleep(0.45)
    pair = pane_boxes(app)
    pair = sorted(pair, key=lambda b: b[0])

    def midway(sample):
        """How far a pane width sits from both where it began and where it rests."""
        sample = sorted(sample, key=lambda b: b[0])
        if len(sample) != 2 or len(pair) != 2:
            return 0
        room = 0
        for before, after in zip(sample, pair):
            began = start_w if before[2] > after[2] else 36
            lo, hi = min(began, after[2]), max(began, after[2])
            room += min(before[2] - lo, hi - before[2])
        return room

    early = max(seen, key=midway) if seen else None
    widths = [tuple(round(b[2]) for b in sorted(s, key=lambda b: b[0])) for s in seen]
    print(f"    samples {widths}", flush=True)
    print(f"    early {early}", flush=True)
    print(f"    pair {pair}", flush=True)
    check(f"two panes after the drop {pair}", len(pair) == 2 and early is not None and len(early) == 2)
    early = sorted(early or [], key=lambda b: b[0])
    gap = pair[1][0] - (pair[0][0] + pair[0][2])
    check(f"the panes sit side by side ({gap})", 2 <= gap <= 24)
    check("the panes share the page height", abs(pair[0][3] - pair[1][3]) < 30)
    moved = False
    for before, after in zip(early, pair):
        began = start_w if before[2] > after[2] else 36
        lo, hi = min(began, after[2]), max(began, after[2])
        # A frame from the glide, not the sliver it leaves from and not the rectangle it rests on.
        if lo + 24 < before[2] < hi - 24:
            moved = True
            print(f"    width {before[2]} between {began} and {after[2]}", flush=True)
    check("a pane was between its start and its rest", moved)
    docs = app.see("the docs row", role="label", name="Arch Docs")
    app.drag(docs, right_x, y)
    time.sleep(0.75)
    grid = pane_boxes(app)
    packed = [(round(b[0]), round(b[1]), round(b[2]), round(b[3])) for b in grid]
    print(f"    FINAL {packed}", flush=True)
    check(f"three panes {packed}", len(grid) == 3)
    large = max(grid, key=lambda b: b[3])
    stacked = [b for b in grid if b is not large]
    stacked = sorted(stacked, key=lambda b: b[1])
    check("one pane is taller than the other two", large[3] > stacked[0][3] * 1.4 and large[3] > stacked[1][3] * 1.4)
    check("the shorter panes share a column", abs(stacked[0][0] - stacked[1][0]) < 16)
    check("the shorter panes are stacked", stacked[1][1] >= stacked[0][1] + stacked[0][3] - 4)
    check("the tall pane is beside that column", abs(large[0] - stacked[0][0]) > 40)
    crop(app, "grid")


@test
def private_tab_keeps_nothing(app):
    app.key("ctrl+shift+n")
    app.see("the private tab's word", role="label", name="A tab that keeps nothing")
    app.go("http://blog.test")
    shown(app, "blog.test")
    app.key("ctrl+t")
    app.see("a new tab from a private one is private too", role="label", name="A tab that keeps nothing")
    app.key("ctrl+1")
    app.go("http://news.test")
    shown(app, "news.test")
    time.sleep(1.5)
    app.quit()
    history = app.read("history.json") or []
    check("the private visit left no history", not any("blog.test" in v["url"] for v in history))
    check("the ordinary visit did", any("news.test" in v["url"] for v in history))


@test
def links_open_in_new_tabs(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.server.forget()
    app.click_page(90, 30, button=2)
    wait("the middle-clicked link to load behind", lambda: app.server.asked("news.test", "/next"), 10)
    check("still on the news", app.title() == "Daily News")
    app.click_page(290, 30)
    wait("the target=_blank link in a new tab", lambda: app.title() == "Corner Shop", 10)


@test
def session_comes_back(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+t")
    app.go("http://shop.test")
    shown(app, "shop.test")
    time.sleep(1.2)
    home = app.home
    app.quit()
    again = App(app.server, app.name + "-again", home=home)
    try:
        wait("the shop, which was on screen", lambda: again.title() == "Corner Shop", 12)
        check("both tabs", {"Daily News", "Corner Shop"} <= set(tab_titles(again)))
    finally:
        again.quit()


# MARK: the page


@test
def ad_blocker(app):
    app.go("http://news.test")
    state = shown(app, "news.test")
    wait("the ad slot hidden", lambda: app.server.report("news.test")["ad"] == "none")
    check("the tracker was never asked for", not app.server.asked("doubleclick.net"))
    check("the ad slot is hidden", state["ad"] == "none" or app.server.report("news.test")["ad"] == "none")


@test
def ad_blocker_off_for_a_site(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+comma")
    app.press("the Privacy page", role="push button", name="Privacy")
    app.press("the per-site switch", role="push button", name="Block on news.test")
    app.key("Escape")
    wait("the tracker loaded once the site is let off", lambda: app.server.asked("doubleclick.net"), 10)


@test
def hiding_something_for_good(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+shift+h")
    app.see("the hint", role="label", contains="Click anything to hide it")
    app.click_page(300, 100)
    wait("the cookie bar hidden", lambda: app.server.report("news.test")["cookie"] == "none", 10)
    app.key("Escape")
    wait("written down", lambda: "news.test" in (app.read("hidden.json") or {}))
    app.key("ctrl+r", pause=1.5)
    wait("still hidden after a reload", lambda: app.server.report("news.test")["cookie"] == "none")
    app.key("ctrl+shift+u")
    app.see("the hidden list", role="label", contains="We use cookies")
    app.press("Restore all", role="push button", name="Restore all")
    wait("the cookie bar back", lambda: app.server.report("news.test")["cookie"] != "none", 10)


@test
def reading_mode(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+shift+r")
    wait("the article alone", lambda: app.server.report("news.test")["reader"], 10)


@test
def find_on_page(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+f")
    app.type("needle")
    app.see("the count", role="label", contains="of 3")
    app.key("Return")
    app.see("the second match", role="label", name="2 of 3")
    app.key("Escape")
    app.gone("the find field", role="label", contains="of 3")


@test
def zooming(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+equal")
    app.see("the new size", role="label", name="110%")
    wait("remembered for the site", lambda: app.prefs().get("zooms", {}).get("news.test"))
    app.key("ctrl+0")
    app.see("back to the start", role="label", name="100%")


@test
def a_site_asking_permission(app):
    app.go(f"http://localhost:{app.server.port}/ask")
    wait("the page", lambda: app.title() == "Asking")
    app.click_page(400, 300)
    app.see("the question", role="label", name="localhost wants to send you notifications")
    app.press("Allow", role="push button", name="Allow")
    wait("the page told yes", lambda: (app.server.report("localhost") or {}).get("notify") == "granted", 10)
    check("the answer remembered", app.prefs().get("permissions", {}).get("localhost notifications") is True)


@test
def a_page_that_fails(app):
    app.go("http://dead.test")
    app.see("the trouble", role="push button", name="Try again", timeout=15)


@test
def link_address_under_the_pointer(app):
    app.go("http://news.test")
    shown(app, "news.test")
    px, py = app.page_origin()
    app.focus()
    app.move(px + 90, py + 30)
    app.see("the link's address", role="label", name="http://news.test/next")


# MARK: panels


@test
def history_panel(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.go("http://shop.test")
    shown(app, "shop.test")
    app.key("ctrl+y")
    app.see("the panel", role="label", name="History")
    app.see("today's heading", role="label", name="Today")
    app.see("a page", role="label", name="Daily News")
    app.type("shop")
    app.gone("the news filtered out", role="label", name="Daily News")
    app.see("the shop left", role="label", name="Corner Shop")
    app.key("Escape")
    app.gone("the panel gone", role="label", name="History")


@test
def bookmarks(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+shift+b")
    app.see("the card", role="label", name="Bookmarked")
    app.press("Done", role="push button", name="Done")
    wait("kept in the file", lambda: any(b.get("url") == "http://news.test/" for b in (app.read("bookmarks.json") or [])))
    app.press("the bookmarks door", role="push button", name="Bookmarks")
    app.see("the page in the list", role="label", name="Daily News")
    app.press("Manage", role="push button", name="Manage Bookmarks…")
    app.see("the panel", role="label", name="1 bookmark")


@test
def downloading_a_file(app):
    app.go("http://files.test")
    target = os.path.join(app.downloads, "report.bin")
    wait("the file in Downloads", lambda: os.path.exists(target) and os.path.getsize(target) == 10000, 15)
    app.see("the word that it arrived", role="label", name="Downloaded report.bin")
    app.key("ctrl+shift+j")
    app.see("the downloads panel", role="label", name="report.bin")
    wait("remembered", lambda: (app.read("downloads.json") or [{}])[0].get("name") == "report.bin")


@test
def settings_change_the_look(app):
    app.key("ctrl+comma")
    app.see("settings", role="label", name="Settings")
    app.press("Dark", role="push button", name="Dark")
    wait("dark written down", lambda: app.prefs().get("look") == "dark")
    app.press("Tabs page", role="push button", name="Tabs")
    app.see("tab settings", role="label", name="Tabs in a sidebar")


# MARK: the chrome


@test
def reload_sits_in_the_right_corner(app):
    app.go("http://news.test")
    shown(app, "news.test")
    back = app.see("back", name="Back   Ctrl+[")
    forward = app.see("forward", name="Forward   Ctrl+]")
    reload = app.see("reload", name="Reload   Ctrl+R")
    bx, _, _, _ = app.box(back)
    fx, _, _, _ = app.box(forward)
    rx, _, rw, _ = app.box(reload)
    check(f"back stays left of forward ({bx} < {fx})", bx < fx)
    check(f"forward stays left of reload ({fx} < {rx})", fx < rx)
    title = app.see("the page row", role="label", name="Daily News")
    row = title.get_parent() or title
    _, _, row_w, _ = app.box(row)
    row_right = app.box(row)[0] + row_w
    reload_right = rx + rw
    check(
        f"reload meets the header's right edge ({reload_right} vs {row_right})",
        abs(reload_right - row_right) <= 24,
    )
    app.server.forget()
    app.press("reload", name="Reload   Ctrl+R")
    wait("reload asks for the page again", lambda: app.server.asked("news.test", "/"), 8)


@test
def sidebar_row_has_no_close_button(app):
    app.go("http://news.test")
    shown(app, "news.test")
    # Neither a row nor the sidebar header has a close button.
    titles = app.nodes(role="label", name="Daily News")
    check("the row", titles)
    row = titles[0]
    parent = row.get_parent()
    names = []
    if parent is not None:
        for i in range(parent.get_child_count()):
            child = parent.get_child_at_index(i)
            if child is None:
                continue
            role = child.get_role_name() or ""
            if "button" in role.lower():
                names.append(child.get_name() or "")
    check("a close button on a row", not any("close" in name.lower() for name in names))
    check("a close button in the sidebar", not app.nodes(name="Close"))


@test
def sidebar_row_shows_favicon(app):
    app.go("http://news.test")
    shown(app, "news.test")
    wait("the favicon request", lambda: app.server.asked("news.test", "/favicon.ico"), 8)
    app.see("the site icon on the row", role="image", name="Site icon")
    check("a blank row has no bullet", "•" not in app.texts())


@test
def ctrl_t_keeps_adding_tabs(app):
    for _ in range(10):
        app.key("ctrl+t", pause=0.05)
    wait("eleven tabs after ten Ctrl+T", lambda: tab_titles(app).count("New Tab") == 11)
    app.press("the new-tab control", role="push button", name="New tab")
    wait("the button inserts another", lambda: tab_titles(app).count("New Tab") == 12)
    check("a blank row has no bullet", "•" not in app.texts())


def address_box(app):
    for role in ("text", "entry", "text box"):
        found = app.nodes(role=role, name="Address")
        if found:
            return found[0]
    return app.see("the address field", name="Address")


@test
def address_field_sits_at_the_top(app):
    # A fresh window is a blank tab, so the field is already up.
    field = address_box(app)
    _, top, _, _ = app.box(field)
    check(f"the field starts near the top ({top})", top < 80)
    app.type("hello")
    time.sleep(0.6)
    field = address_box(app)
    # The accessible text sits inside the pill's padding. The pill is its parent.
    pill = field.get_parent() or field
    x, y, w, h = app.box(pill)
    _, _, window_w, _ = app.box(app._frame())
    title = app.see("the blank row", role="label", name="New Tab")
    row = title.get_parent()
    rx, _, rw, _ = app.box(row)
    gap = x - (rx + rw)
    right = window_w - (x + w)
    check(f"twelve pixels clear of the sidebar ({gap})", 8 <= gap <= 40)
    check(f"the enlarged pill reaches the other side ({right})", 4 <= right <= 40)
    check(f"still at the top ({y})", y < 80)
    rows = [n for n in app.nodes(role="label") if (n.get_name() or "") == "hello"]
    check("a suggestion", rows)
    below = [n for n in rows if app.box(n)[1] > y]
    check("the suggestion is under the field", below)
    rx, ry, _, rh = app.box(below[0])
    check(f"the suggestion extends below the field ({ry + rh} > {y + h})", ry + rh > y + h)
    check(f"the suggestion is not left of the field ({rx} >= {x - 4})", rx >= x - 4)


@test
def address_pill_rests_on_a_page(app):
    app.go("http://news.test")
    shown(app, "news.test")
    field = address_box(app)
    _, top, _, _ = app.box(field)
    check(f"the pill stays up over the page ({top})", top < 100)
    pill = field.get_parent() or field
    _, _, w, h = app.box(pill)
    check(f"the resting pill is short ({h})", h < 64)
    check(f"the resting pill stays narrow ({w})", 200 <= w <= 460)
    app.key("ctrl+l")
    app.type("hello")
    time.sleep(0.6)
    field = address_box(app)
    pill = field.get_parent() or field
    x, y, w, h = app.box(pill)
    _, _, window_w, _ = app.box(app._frame())
    title = app.see("the page row", role="label", name="Daily News")
    row = title.get_parent()
    rx, _, rw, _ = app.box(row)
    gap = x - (rx + rw)
    right = window_w - (x + w)
    check(f"typing widens it clear of the sidebar ({gap})", 8 <= gap <= 40)
    check(f"the enlarged pill reaches the other side ({right})", 4 <= right <= 40)
    rows = [n for n in app.nodes(role="label") if (n.get_name() or "") == "hello"]
    below = [n for n in rows if app.box(n)[1] > y]
    check("a suggestion slides out under the pill", below)
    ry = app.box(below[0])[1]
    check(f"the suggestion is below the pill ({ry} > {y + h - 4})", ry + 4 > y + h)


@test
def sidebar_shows_memory(app):
    def figure():
        return any(t.endswith((" KB", " MB", " GB")) for t in app.texts())

    wait("a memory figure in the sidebar", figure)


@test
def folding_and_peeking(app):
    def page_width():
        return max((app.box(n)[2] for n in app.nodes(role="document web", showing=False)), default=0)

    app.go("http://news.test")
    shown(app, "news.test")
    narrow = page_width()
    app.key("ctrl+s", pause=1)
    wait("the page taking the column's room", lambda: page_width() > narrow + 200)
    # The window's own left edge: past its 5 px shadow, within Wisp's 6.
    app.move(9, 400)
    time.sleep(0.2)
    app.move(7, 402)
    time.sleep(0.8)
    app.click(60, 100)  # the "New tab" row, in the column peeking out
    wait("a new tab from the peeking column", lambda: app.title() == "New Tab")
    app.key("Escape")
    app.move(900, 400)
    app.key("ctrl+s", pause=1)
    wait("the column back", lambda: page_width() == narrow)


@test
def tabs_across_the_top(app):
    app.go("http://news.test")
    shown(app, "news.test")
    app.key("ctrl+shift+s", pause=1)
    wait("the strip written down", lambda: app.prefs().get("sidebar") is False)
    title = app.see("the tab in the strip", role="label", name="Daily News")
    x, y, w, h = app.box(title)
    check(f"the tab sits along the top ({x},{y})", y < 40)
    app.key("ctrl+shift+s", pause=1)
    wait("back down the side", lambda: app.prefs().get("sidebar") is True)


@test
def one_instance(app):
    import subprocess

    subprocess.run([app.proc.args[0], "http://shop.test/"], env=app.env, timeout=10)
    wait("the link came to this window", lambda: app.title() == "Corner Shop", 10)
    subprocess.run([app.proc.args[0], "--private"], env=app.env, timeout=10)
    app.see("the private tab", role="label", name="A tab that keeps nothing")


def main():
    os.makedirs(ARTIFACTS, exist_ok=True)
    wanted = sys.argv[1:]
    server = Server()
    passed, failed = [], []
    for fn in TESTS:
        if wanted and not any(w in fn.__name__ for w in wanted):
            continue
        app = None
        start = time.time()
        try:
            app = App(server, fn.__name__)
            fn(app)
            passed.append(fn.__name__)
            print(f"  ok    {fn.__name__}  ({time.time() - start:.1f}s)", flush=True)
        except Exception as e:
            failed.append(fn.__name__)
            shot = app.shot("failed") if app else ""
            print(f"  FAIL  {fn.__name__}: {e}  {shot}", flush=True)
            traceback.print_exc(limit=3)
        finally:
            if app:
                app.close()
            server.forget()
    server.stop()
    print(f"\n{len(passed)} passed, {len(failed)} failed")
    if failed:
        print("failed: " + ", ".join(failed))
        sys.exit(1)


if __name__ == "__main__":
    main()
