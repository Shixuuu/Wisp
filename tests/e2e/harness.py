"""End-to-end harness: the real browser, driven from outside.

- A small web server stands in for the internet. Wisp is pointed at it as
  its HTTP proxy, so made-up sites (news.test, shop.test) and a made-up ad
  server (doubleclick.net) all land here. Pages report what they see back
  to the server, so a test can ask "is the cookie bar hidden?" of the page
  itself.
- Wisp runs with a throwaway home, on whatever display is set (Xvfb in CI).
- Keys and clicks go in through xdotool, as a person's would.
- What is on screen is read back through the accessibility tree (AT-SPI),
  and what was saved through the files Wisp writes.
"""

import base64
import http.server
import json
import os
import shutil
import signal
import subprocess
import tempfile
import threading
import time
import urllib.parse

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BINARY = os.environ.get("WISP", os.path.join(ROOT, "target", "debug", "wisp"))
ARTIFACTS = os.environ.get("E2E_ARTIFACTS", os.path.join(ROOT, "target", "e2e"))
HAVE_XDOTOOL = shutil.which("xdotool") is not None


def hypr(*args):
    """Talk to the compositor on the user's bus.

    The tests themselves run on a private session bus, which hyprctl cannot
    use. WISP_HYPR_BUS is that user bus, captured before the private one.
    """
    env = dict(os.environ)
    bus = os.environ.get("WISP_HYPR_BUS")
    if bus:
        env["DBUS_SESSION_BUS_ADDRESS"] = bus
    return subprocess.run(["hyprctl", *args], capture_output=True, text=True, env=env)

ARTICLE = " ".join(["Pacman is the package manager of Arch Linux, simple and fast."] * 12)

PAGE = """<!doctype html><html><head><meta charset="utf-8"><title>{title}</title>
<style>body{{font:16px sans-serif;margin:0;padding:80px 20px 20px}}
#next{{position:fixed;left:10px;top:10px;width:180px;height:40px;background:#cde;display:block}}
#blank{{position:fixed;left:200px;top:10px;width:180px;height:40px;background:#dec;display:block}}
</style></head><body>
<a id="next" href="/next">Next page</a>
<a id="blank" href="http://shop.test/" target="_blank">Shop</a>
<div id="cookie" style="background:#fc0;padding:20px">We use cookies <button>OK</button></div>
<ins class="adsbygoogle" id="ad" style="display:block;height:60px;background:red">AD</ins>
<script src="http://doubleclick.net/ad.js"></script>
<h1>{title}</h1>
<article><p>{article}</p><p>{article}</p><p>{article}</p><p>{article}</p></article>
<p style="height:2000px">The needle is here. Needle again. And needle.</p>
<script>
function report(extra) {{
  var cookie = document.getElementById('cookie'), ad = document.getElementById('ad');
  var s = {{ host: location.host, path: location.pathname,
    cookie: cookie ? getComputedStyle(cookie).display : 'gone',
    ad: ad ? getComputedStyle(ad).display : 'gone',
    reader: !!document.getElementById('wisp-reader'),
    zoom: Math.round(100 * window.outerWidth / window.innerWidth) }};
  for (var k in (extra || {{}})) s[k] = extra[k];
  new Image().src = '/__report?' + encodeURIComponent(JSON.stringify(s));
}}
setInterval(report, 250);
{extra}
</script></body></html>"""


class Server:
    """The internet, as far as Wisp can tell."""

    def __init__(self):
        self.requests = []
        self.reports = {}
        self.lock = threading.Lock()
        server = self

        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *args):
                pass

            def handle(self):
                try:
                    super().handle()
                except (ConnectionError, TimeoutError):
                    pass  # Wisp hung up first, as browsers do.

            def do_GET(self):
                url = urllib.parse.urlsplit(self.path)
                host = url.hostname or self.headers.get("Host", "").split(":")[0]
                with server.lock:
                    server.requests.append((host, url.path, url.query))
                if host == "dead.test":
                    # Hang up without a word: a real network failure.
                    self.close_connection = True
                    return
                status, kind, body, headers = server.respond(host, url.path, url.query)
                data = body.encode() if isinstance(body, str) else body
                self.send_response(status)
                self.send_header("Content-Type", kind)
                self.send_header("Content-Length", str(len(data)))
                for k, v in headers.items():
                    self.send_header(k, v)
                self.end_headers()
                self.wfile.write(data)

        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def respond(self, host, path, query):
        html = "text/html; charset=utf-8"
        if path == "/__report":
            state = json.loads(urllib.parse.unquote(query))
            with self.lock:
                self.reports.setdefault(host, {}).update(state)
            return 204, "text/plain", b"", {}
        if path == "/favicon.ico":
            png = base64.b64decode(
                "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="
            )
            return 200, "image/png", png, {}
        if host == "doubleclick.net":
            return 200, "application/javascript", "window.adLoaded = true;", {}
        if host == "search.test":
            words = urllib.parse.parse_qs(query).get("q", [""])[0]
            return 200, html, PAGE.format(title=f"Results for {words}", article="Found.", extra=""), {}
        if host == "files.test":
            return 200, "application/octet-stream", b"wisp" * 2000, {"Content-Disposition": 'attachment; filename="report.bin"'}
        if path == "/ask":
            # Served from localhost: WebKit lets only secure origins ask.
            # On a click: WebKit refuses a page that asks unprompted.
            extra = "document.addEventListener('click', function () { Notification.requestPermission().then(function (r) { report({notify: r}); }); });"
            return 200, html, PAGE.format(title="Asking", article="Asks.", extra=extra), {}
        names = {"news.test": "Daily News", "shop.test": "Corner Shop", "docs.test": "Arch Docs", "blog.test": "A Blog"}
        title = names.get(host, host)
        if path == "/next":
            title += " — next"
        return 200, html, PAGE.format(title=title, article=ARTICLE, extra=""), {}

    def asked(self, host, path=None):
        with self.lock:
            return [r for r in self.requests if r[0] == host and (path is None or r[1] == path)]

    def report(self, host):
        with self.lock:
            return self.reports.get(host)

    def forget(self):
        with self.lock:
            self.requests.clear()
            self.reports.clear()

    def stop(self):
        self.httpd.shutdown()


class Failure(AssertionError):
    pass


# Newer AT-SPI names some roles differently; these are the same thing.
ROLES = {"button": "push button"}


def wait(what, check, timeout=8.0, every=0.15):
    """Until `check()` is truthy; that value is returned."""
    end = time.time() + timeout
    last = None
    while time.time() < end:
        try:
            last = check()
        except Exception as e:  # the tree changes under our feet
            last = e
        if last and not isinstance(last, Exception):
            return last
        time.sleep(every)
    raise Failure(f"timed out waiting for {what} (last: {last!r})")


class App:
    """One run of Wisp with its own home."""

    def __init__(self, server, name, home=None, settings=None, args=()):
        self.server = server
        self.name = name
        self.home = home or tempfile.mkdtemp(prefix="wisp-e2e-")
        self.owns_home = home is None
        config = os.path.join(self.home, ".config", "wisp")
        os.makedirs(config, exist_ok=True)
        path = os.path.join(config, "settings.json")
        if not os.path.exists(path):
            base = {"engine": "custom", "custom_engine": "http://search.test/?q=%s"}
            base.update(settings or {})
            with open(path, "w") as f:
                json.dump(base, f)
        self.downloads = os.path.join(self.home, "Downloads")
        os.makedirs(self.downloads, exist_ok=True)
        with open(os.path.join(config, "..", "user-dirs.dirs"), "w") as f:
            f.write('XDG_DOWNLOAD_DIR="$HOME/Downloads"\n')
        self.env = dict(os.environ)
        proxy = f"http://127.0.0.1:{server.port}"
        for k in ("HTTPS_PROXY", "https_proxy", "NO_PROXY", "no_proxy", "ALL_PROXY", "all_proxy"):
            self.env.pop(k, None)
        self.env.update({
            "HOME": self.home,
            "XDG_CONFIG_HOME": os.path.join(self.home, ".config"),
            "XDG_DATA_HOME": os.path.join(self.home, ".local", "share"),
            "XDG_CACHE_HOME": os.path.join(self.home, ".cache"),
            "http_proxy": proxy,
            "HTTP_PROXY": proxy,
            # xdotool only sees X11. Without it, Wayland plus wtype drives the same window.
            "GDK_BACKEND": "x11" if HAVE_XDOTOOL else "wayland",
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS": "1",
        })
        if HAVE_XDOTOOL:
            self.env["LIBGL_ALWAYS_SOFTWARE"] = "1"
        self.log = open(os.path.join(ARTIFACTS, f"{name}.log"), "w")
        # WISP_WRAP="gdb -batch -ex run -ex bt --args" runs it under a debugger.
        wrap = os.environ.get("WISP_WRAP", "").split()
        self.wrapped = bool(wrap)
        self.proc = subprocess.Popen([*wrap, BINARY, *args], env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
        self.frame = wait("the window", self._frame, timeout=20)
        self.window = wait("the X window", self._xwindow, timeout=10)
        self._home_ws = None
        time.sleep(0.8)

    # MARK: finding things

    def _application(self):
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(i)
            try:
                if app and (app.get_process_id() == self.proc.pid or (self.wrapped and app.get_name() == "wisp")):
                    return app
            except Exception:
                continue
        return None

    def _frame(self):
        app = self._application()
        if app and app.get_child_count() > 0:
            return app.get_child_at_index(0)
        return None

    def _xwindow(self):
        pid = self._application().get_process_id() if self.wrapped else self.proc.pid
        if HAVE_XDOTOOL:
            out = subprocess.run(["xdotool", "search", "--pid", str(pid)], capture_output=True, text=True).stdout.split()
            for wid in out:
                geo = subprocess.run(["xdotool", "getwindowgeometry", "--shell", wid], capture_output=True, text=True).stdout
                values = dict(line.split("=") for line in geo.split() if "=" in line)
                if int(values.get("WIDTH", 0)) > 400:
                    self.origin = (int(values["X"]), int(values["Y"]))
                    return wid
            return None
        listed = hypr("clients", "-j")
        if listed.returncode != 0 or not listed.stdout.strip():
            return None
        for client in json.loads(listed.stdout):
            size = client.get("size") or [0, 0]
            if client.get("pid") == pid and size[0] > 400:
                at = client.get("at") or [0, 0]
                self.origin = (int(at[0]), int(at[1]))
                self.size = (int(size[0]), int(size[1]))
                return str(client.get("address"))
        return None

    def nodes(self, role=None, name=None, contains=None, showing=True):
        found = []

        def walk(node, depth=0):
            if depth > 40 or node is None:
                return
            try:
                r, n = node.get_role_name(), node.get_name() or ""
                states = node.get_state_set()
                count = node.get_child_count()
            except Exception:
                return
            if showing and not states.contains(Atspi.StateType.SHOWING):
                return
            if (role is None or ROLES.get(r, r) == ROLES.get(role, role)) and (name is None or n == name) and (contains is None or contains in n):
                found.append(node)
            for i in range(count):
                try:
                    walk(node.get_child_at_index(i), depth + 1)
                except Exception:
                    pass

        # Every window the app has: the main one and its popovers and menus.
        app = self._application()
        for i in range(app.get_child_count() if app else 0):
            walk(app.get_child_at_index(i))
        return found

    def texts(self, role="label"):
        return [n.get_name() for n in self.nodes(role=role)]

    def has(self, role=None, name=None, contains=None):
        return bool(self.nodes(role, name, contains))

    def see(self, what, role=None, name=None, contains=None, timeout=8.0):
        try:
            return wait(what, lambda: self.nodes(role, name, contains), timeout)[0]
        except Failure:
            # What was on screen instead, for reading the failure from a log.
            shown = [(n.get_role_name(), n.get_name()) for n in self.nodes() if n.get_name()]
            print(f"    on screen: {shown}", flush=True)
            raise

    def gone(self, what, role=None, name=None, contains=None, timeout=8.0):
        return wait(what, lambda: not self.nodes(role, name, contains), timeout)

    def title(self):
        return self._frame().get_name()

    def box(self, node):
        e = node.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
        return e.x, e.y, e.width, e.height

    # MARK: doing things

    def _active_pid(self):
        raw = hypr("activewindow", "-j").stdout
        if not raw.strip():
            return None
        try:
            return json.loads(raw).get("pid")
        except json.JSONDecodeError:
            return None

    def focus(self):
        if HAVE_XDOTOOL:
            subprocess.run(["xdotool", "windowactivate", "--sync", self.window], capture_output=True)
            subprocess.run(["xdotool", "windowfocus", "--sync", self.window], capture_output=True)
            return
        if self._home_ws is None:
            current = hypr("activeworkspace", "-j").stdout
            if current.strip():
                self._home_ws = json.loads(current).get("id")
        pid = self.proc.pid
        hypr("dispatch", f'hl.dsp.focus({{ window = "pid:{pid}" }})')
        time.sleep(0.2)
        if self._active_pid() != pid:
            hypr("dispatch", f'hl.dsp.window.move({{ window = "pid:{pid}", workspace = "9", follow = true }})')
            time.sleep(0.3)
        if self._active_pid() != pid:
            raise Failure(f"refusing to type: the focused window is not this Wisp ({pid})")

    def key(self, *keys, pause=0.35, clear=True):
        self.focus()
        for k in keys:
            if HAVE_XDOTOOL:
                subprocess.run(["xdotool", "key", *(["--clearmodifiers"] if clear else []), k], check=True)
            else:
                _wtype_key(k)
            time.sleep(0.12)
        time.sleep(pause)

    def type(self, text, pause=0.4):
        self.focus()
        if HAVE_XDOTOOL:
            subprocess.run(["xdotool", "type", "--delay", "40", text], check=True)
        else:
            subprocess.run(["wtype", "-d", "20", text], check=True)
        time.sleep(pause)

    def hold(self, key):
        if not HAVE_XDOTOOL:
            raise Failure("holding a key needs xdotool")
        subprocess.run(["xdotool", "keydown", key], check=True)

    def release(self, key):
        if not HAVE_XDOTOOL:
            raise Failure("releasing a key needs xdotool")
        subprocess.run(["xdotool", "keyup", key], check=True)

    def move(self, x, y):
        ox, oy = self.origin
        if HAVE_XDOTOOL:
            subprocess.run(["xdotool", "mousemove", str(ox + x), str(oy + y)], check=True)
        else:
            Atspi.generate_mouse_event(ox + x, oy + y, "abs")
        time.sleep(0.2)

    def click(self, x, y, button=1, times=1):
        self.focus()
        ox, oy = self.origin
        for _ in range(times):
            if HAVE_XDOTOOL:
                self.move(x, y)
                subprocess.run(["xdotool", "click", "--repeat", "1", "--delay", "80", str(button)], check=True)
            else:
                Atspi.generate_mouse_event(ox + x, oy + y, f"b{button}c")
            time.sleep(0.2)
        time.sleep(0.25)

    def click_node(self, node, button=1, times=1):
        if not HAVE_XDOTOOL:
            self.focus()
            e = node.get_component_iface().get_extents(Atspi.CoordType.SCREEN)
            cx, cy = e.x + e.width // 2, e.y + e.height // 2
            for _ in range(times):
                Atspi.generate_mouse_event(cx, cy, "abs")
                Atspi.generate_mouse_event(cx, cy, f"b{button}p")
                time.sleep(0.05)
                Atspi.generate_mouse_event(cx, cy, f"b{button}r")
                time.sleep(0.2)
            time.sleep(0.25)
            return
        x, y, w, h = self.box(node)
        self.click(x + w // 2, y + h // 2, button, times)

    def press(self, what, role=None, name=None, contains=None, button=1):
        """Press a button the way a screen reader does; click anything else.

        Where GTK says a thing is on screen isn't reliable across versions
        for the layers laid over the page, but a button's action always is.
        """
        node = self.see(what, role, name, contains)
        action = node.get_action_iface()
        if button == 1 and ROLES.get(node.get_role_name(), node.get_role_name()) == "push button" and action:
            action.do_action(0)
            time.sleep(0.45)
        else:
            self.click_node(node, button)

    def go(self, typed):
        """Type into the address field and press Return.

        The first window of a session sometimes misses the first chord while
        the desktop portal is still starting, so a navigation that is still
        on the blank tab is typed again.
        """
        for _ in range(2):
            self.key("ctrl+l")
            wait("the address field", lambda: self.nodes(name="Address"), 8)
            self.type(typed)
            self.key("Return", pause=0.4)
            time.sleep(0.7)
            if self.title() not in (None, "", "New Tab", "Wisp"):
                return

    def page_origin(self):
        """Where the page starts in the window: right of the column."""
        side = self.prefs().get("side_width", 232)
        return (int(side) + 1 if self.prefs().get("sidebar", True) else 0), (0 if self.prefs().get("sidebar", True) else 52)

    def click_page(self, x, y, button=1):
        px, py = self.page_origin()
        self.click(px + x, py + y, button)

    def _pointer_bin(self):
        binary = os.path.join(ROOT, "target", "e2e", "virtual_pointer")
        source = os.path.join(ROOT, "tests", "e2e", "virtual_pointer.c")
        if not os.path.exists(binary) or os.path.getmtime(source) > os.path.getmtime(binary):
            os.makedirs(os.path.dirname(binary), exist_ok=True)
            subprocess.run(["gcc", "-O2", "-o", binary, source, "-lwayland-client"], check=True)
        return binary

    def _pointer_env(self):
        env = dict(os.environ)
        listed = hypr("monitors", "-j")
        if listed.returncode == 0 and listed.stdout.strip():
            monitor = json.loads(listed.stdout)[0]
            scale = float(monitor.get("scale") or 1) or 1
            width = int(round(monitor["width"] / scale))
            height = int(round(monitor["height"] / scale))
            env["POINTER_EXTENT"] = f"{width},{height}"
        return env

    def _screen(self, x, y):
        self._xwindow()
        return self.origin[0] + int(x), self.origin[1] + int(y)

    def pointer_click(self, x, y):
        """A real compositor click at window coordinates."""
        if HAVE_XDOTOOL:
            self.click(x, y)
            return
        self.focus()
        sx, sy = self._screen(x, y)
        subprocess.run([self._pointer_bin(), "click", str(sx), str(sy)], check=True, env=self._pointer_env())
        time.sleep(0.35)

    def drag(self, node, x, y, steps=16):
        """Press `node` and release at window coordinates `(x, y)`.

        On X11 the motion is xdotool's, in screen pixels. On Wayland it is a
        virtual pointer the compositor delivers, because AT-SPI mouse events
        do not become Wayland pointer events there. `steps` is unused; both
        paths move across a few frames well under a fifth of a second.
        """
        del steps
        self.focus()
        bx, by, bw, bh = self.box(node)
        if HAVE_XDOTOOL:
            ox, oy = self.origin
            sx, sy = ox + bx + max(bw, 1) // 2, oy + by + max(bh, 1) // 2
            ex, ey = ox + int(x), oy + int(y)
            subprocess.run(["xdotool", "mousemove", str(sx), str(sy)], check=True)
            subprocess.run(["xdotool", "mousedown", "1"], check=True)
            for i in range(1, 9):
                mx, my = sx + (ex - sx) * i // 8, sy + (ey - sy) * i // 8
                subprocess.run(["xdotool", "mousemove", str(mx), str(my)], check=True)
            subprocess.run(["xdotool", "mouseup", "1"], check=True)
            return
        sx, sy = self._screen(bx + max(bw, 1) // 2, by + max(bh, 1) // 2)
        ex, ey = self.origin[0] + int(x), self.origin[1] + int(y)
        subprocess.run(
            [self._pointer_bin(), "drag", str(sx), str(sy), str(ex), str(ey)],
            check=True,
            env=self._pointer_env(),
        )
        time.sleep(0.02)

    # MARK: what was written down

    def file(self, *parts):
        return os.path.join(self.home, ".local", "share", "wisp", *parts)

    def read(self, name):
        path = self.file(name)
        if not os.path.exists(path):
            return None
        with open(path) as f:
            return json.load(f)

    def prefs(self):
        with open(os.path.join(self.home, ".config", "wisp", "settings.json")) as f:
            return json.load(f)

    def shot(self, label):
        path = os.path.join(ARTIFACTS, f"{self.name}-{label}.png")
        subprocess.run(["import", "-window", "root", path], capture_output=True)
        return path

    def quit(self):
        if self.proc.poll() is None:
            self.key("ctrl+q", pause=0.2)
            try:
                self.proc.wait(5)
            except subprocess.TimeoutExpired:
                self.proc.send_signal(signal.SIGKILL)
                self.proc.wait()
        self.log.close()

    def close(self):
        # A test that failed holding a key must not leave it held.
        if HAVE_XDOTOOL:
            subprocess.run(["xdotool", "keyup", "ctrl", "shift", "alt", "super"], capture_output=True)
        self.quit()
        if not HAVE_XDOTOOL and self._home_ws is not None:
            hypr("dispatch", f'hl.dsp.focus({{ workspace = "{self._home_ws}" }})')
        if self.owns_home:
            shutil.rmtree(self.home, ignore_errors=True)


def _wtype_key(combo):
    """Turn an xdotool chord (`ctrl+shift+s`, `Return`) into a wtype invocation."""
    parts = combo.split("+")
    key, mods = parts[-1], parts[:-1]
    args = []
    for mod in mods:
        args += ["-M", mod]
    if len(key) == 1:
        args.append(key)
    else:
        args += ["-k", key]
    for mod in reversed(mods):
        args += ["-m", mod]
    subprocess.run(["wtype", *args], check=True)
