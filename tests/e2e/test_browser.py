"""Wisp, end to end: every test starts the real browser with a fresh home
and drives it with keys and clicks. Run with tests/e2e/run.sh."""

import os
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
    app.go("http://news.test")
    shown(app, "news.test")
    app.press("the tab", role="label", name="Daily News", button=3)
    app.see("its menu", role="menu")
    # Pin comes first; GTK 4.14 leaves menu items nameless.
    app.nodes(role="menu item")[0].get_action_iface().do_action(0)
    wait("the pin written down", lambda: any(t.get("pin") == "N" for t in (app.read("session.json") or {}).get("tabs", [])))
    app.see("the pin's letter", role="label", name="N")


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
    app.press("the Privacy page", role="label", name="Privacy")
    row = app.see("the per-site switch", role="label", name="Block on news.test")
    x, y, w, h = app.box(row)
    switches = [n for n in app.nodes(role="push button") if abs(app.box(n)[1] - y) < 30 and app.box(n)[0] > x + w]
    check("the switch is there", switches)
    app.click_node(switches[0])
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
    app.press("Tabs page", role="label", name="Tabs")
    app.see("tab settings", role="label", name="Tabs in a sidebar")


# MARK: the chrome


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
