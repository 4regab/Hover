"""End-to-end harness: Xvfb + AT-SPI bus + hover, driven by real X input (XTEST)
and found through AT-SPI (the accessible ids). Run scenarios under dbus-run-session."""
import json, os, subprocess, sys, time, shutil
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
from Xlib import X, XK, display as xdisplay
from Xlib.ext import xtest
from PIL import Image

E2E = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(E2E, "..", ".."))
HOVER = os.path.join(REPO, "native/target/release/hover")
# Data folders, the agent's log, the project the tasks run in and the shots: all here.
WORK = os.environ.get("E2E_WORK", os.path.join(E2E, "work"))
OUT = os.path.join(WORK, "out")
LOG = os.path.join(WORK, "agent.log")
PROJ = os.path.join(WORK, "proj")

results = []

def check(name, ok, detail=""):
    results.append((name, bool(ok), detail))
    print(("PASS " if ok else "FAIL ") + name + (f": {detail}" if detail else ""), flush=True)
    return ok


class Hover:
    def __init__(self, data, env=None, fresh=True, disp=":9"):
        self.disp = disp
        if fresh and os.path.exists(data): shutil.rmtree(data)
        os.makedirs(data, exist_ok=True)
        os.makedirs(OUT, exist_ok=True)
        os.makedirs(os.path.join(PROJ, "src"), exist_ok=True)
        os.makedirs(os.path.join(WORK, "rt"), mode=0o700, exist_ok=True)
        self.data = data
        self.xvfb = subprocess.Popen(["Xvfb", disp, "-screen", "0", "1920x1080x24", "+extension", "GLX", "+extension", "RANDR"],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.5)
        os.environ["DISPLAY"] = disp
        self.bus = subprocess.Popen(["/usr/libexec/at-spi-bus-launcher", "--launch-immediately"], stderr=subprocess.DEVNULL)
        time.sleep(1.0)
        self.env = dict(os.environ, HOVER_BENCH="1", HOVER_DATA_DIR=data, E2E_LOG=LOG, XDG_RUNTIME_DIR=os.path.join(WORK, "rt"),
                        E2E_PROJ=PROJ, E2E_WORK=WORK,
                        PATH=os.path.join(WORK, "bin") + ":" + os.environ["PATH"], HOME=os.path.join(data, "home"))
        os.makedirs(self.env["HOME"], exist_ok=True)
        self.env.pop("WAYLAND_DISPLAY", None)
        if env: self.env.update(env)
        self.logf = open(os.path.join(OUT, "hover.log"), "a")
        self.start()
        self.d = xdisplay.Display(disp)
        # Another app with the keyboard (no WM here, so focus would follow the pointer).
        s = self.d.screen()
        self.helper = s.root.create_window(0, 600, 700, 400, 0, s.root_depth, background_pixel=0x303848,
                                           event_mask=X.KeyPressMask | X.FocusChangeMask)
        self.helper.map(); self.d.sync(); time.sleep(0.3)
        self.focus_helper()

    def focus_helper(self):
        self.helper.set_input_focus(X.RevertToParent, X.CurrentTime); self.d.sync()

    def focus(self):
        return self.d.get_input_focus().focus

    def start(self):
        self.p = subprocess.Popen([HOVER], env=self.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.logf, text=True, bufsize=1)
        self.wait_line("bench visible", 60)
        time.sleep(0.8)

    def wait_line(self, prefix, secs=30):
        end = time.time() + secs
        while time.time() < end:
            line = self.p.stdout.readline()
            if not line:
                if self.p.poll() is not None: raise RuntimeError(f"hover exited {self.p.returncode}")
                continue
            if line.startswith(prefix): return line.strip()
        raise TimeoutError(prefix)

    def cmd(self, c, expect=None):
        # Replies come in order, but frames and reopen timings are printed in between.
        replies = {"toggle": "bench toggled", "state": "bench state", "running": "bench running", "stopall": "bench ok", "history": "bench ok"}
        self.p.stdin.write(c + "\n"); self.p.stdin.flush()
        return self.wait_line(expect or replies.get(c.split()[0], "bench"))

    def quit(self):
        try:
            self.p.stdin.write("quit\n"); self.p.stdin.flush(); self.p.wait(15)
        except Exception:
            self.p.kill()
        return self.p.returncode

    def close(self):
        if self.p.poll() is None: self.quit()
        self.bus.kill(); self.xvfb.kill()
        # The a11y bus's own daemons outlive the launcher and keep dbus-run-session waiting.
        for n in ["at-spi-bus-laun", "at-spi2-registr"]: subprocess.run(["pkill", "-x", n])
        subprocess.run(["pkill", "-f", "dbus-daemon.*accessibility"])
        time.sleep(0.5)

    # ---- AT-SPI
    def app(self):
        desk = Atspi.get_desktop(0)
        for i in range(desk.get_child_count()):
            a = desk.get_child_at_index(i)
            if a is not None and a.get_name() == "hover": return a
        return None

    def nodes(self):
        out = []
        def walk(n, depth):
            try: ident = n.get_accessible_id() or ""
            except Exception: ident = ""
            try: name = n.get_name() or ""
            except Exception: name = ""
            out.append((n, ident, name, n.get_role_name()))
            if depth > 40: return
            for i in range(n.get_child_count()):
                c = n.get_child_at_index(i)
                if c is not None: walk(c, depth + 1)
        a = self.app()
        if a: walk(a, 0)
        return out

    def find(self, ident=None, name=None, role=None, secs=5, contains=False):
        end = time.time() + secs
        while True:
            for n, i, nm, r in self.nodes():
                if ident is not None and i != ident: continue
                if name is not None and (name not in nm if contains else nm != name): continue
                if role is not None and r != role: continue
                if ident is None and name is None and role is None: continue
                try:
                    if not n.get_state_set().contains(Atspi.StateType.SHOWING) and not n.get_state_set().contains(Atspi.StateType.VISIBLE): continue
                except Exception: pass
                return n
            if time.time() > end: return None
            time.sleep(0.2)

    def dump(self, path=None):
        lines = [f"{r} | {nm} | {i} | {self.rect(n)}" for n, i, nm, r in self.nodes()]
        if path: open(path, "w").write("\n".join(lines))
        return lines

    def rect(self, n):
        try:
            e = n.get_extents(Atspi.CoordType.SCREEN)
            return (e.x, e.y, e.width, e.height)
        except Exception:
            return None

    # ---- input
    def move(self, x, y):
        xtest.fake_input(self.d, X.MotionNotify, x=int(x), y=int(y)); self.d.sync()

    def click(self, x, y, button=1, dbl=False):
        self.move(x, y); time.sleep(0.08)
        # What a window manager does on a click: the ordinary window under it gets the
        # keyboard (override-redirect ones, the notch, manage their own).
        child = self.d.screen().root.query_pointer().child
        if child and button == 1:
            try:
                if not child.get_attributes().override_redirect: child.set_input_focus(X.RevertToParent, X.CurrentTime); self.d.sync()
            except Exception: pass
        for _ in range(2 if dbl else 1):
            xtest.fake_input(self.d, X.ButtonPress, button); self.d.sync(); time.sleep(0.04)
            xtest.fake_input(self.d, X.ButtonRelease, button); self.d.sync(); time.sleep(0.06)
        time.sleep(0.25)

    def click_node(self, n, dx=0.5, dy=0.5):
        r = self.rect(n)
        assert r and r[2] > 0, f"no extents for {n.get_name()}"
        self.click(r[0] + r[2] * dx, r[1] + r[3] * dy)

    def tap(self, ident=None, name=None, secs=5, **kw):
        n = self.find(ident=ident, name=name, secs=secs, **kw)
        if n is None: raise LookupError(f"not found: {ident or name}")
        self.click_node(n)
        return n

    def key(self, name, mods=()):
        codes = [self.d.keysym_to_keycode(XK.string_to_keysym(m)) for m in mods]
        k = self.d.keysym_to_keycode(XK.string_to_keysym(name))
        for c in codes: xtest.fake_input(self.d, X.KeyPress, c)
        xtest.fake_input(self.d, X.KeyPress, k); xtest.fake_input(self.d, X.KeyRelease, k)
        for c in reversed(codes): xtest.fake_input(self.d, X.KeyRelease, c)
        self.d.sync(); time.sleep(0.05)

    def type(self, text):
        for ch in text:
            if ch == " ": self.key("space")
            elif ch == "\n": self.key("Return")
            elif ch.isupper(): self.key(ch, ("Shift_L",))
            elif ch.isalnum(): self.key(ch)
            else:
                names = {".": "period", ",": "comma", "-": "minus", "/": "slash", "@": "at", "?": "question", "!": "exclam", ":": "colon"}
                sym = XK.string_to_keysym(names[ch])
                kc = self.d.keysym_to_keycode(sym)
                shift = self.d.keycode_to_keysym(kc, 0) != sym
                self.key(names[ch], ("Shift_L",) if shift else ())
            time.sleep(0.02)
        time.sleep(0.2)

    def shot(self, name, box=None):
        root = self.d.screen().root
        g = root.get_geometry()
        raw = root.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xffffffff)
        im = Image.frombytes("RGB", (g.width, g.height), raw.data, "raw", "BGRX")
        if box: im = im.crop(box)
        p = os.path.join(OUT, name + ".png"); im.save(p)
        return im

    def agent_log(self):
        if not os.path.exists(LOG): return []
        out = []
        for l in open(LOG):
            t, tag, j = l.rstrip("\n").split(" ", 2)
            out.append((t, tag, json.loads(j)))
        return out


def wait(pred, secs=15, step=0.2):
    end = time.time() + secs
    while time.time() < end:
        v = pred()
        if v: return v
        time.sleep(step)
    return pred()


def summary():
    ok = sum(1 for r in results if r[1])
    print(f"== {ok}/{len(results)} passed")
    for r in results:
        if not r[1]: print("  FAIL", r[0], r[2])


def crop(name):
    Image.open(f"{OUT}/{name}.png").crop((360, 0, 1560, 480)).save(f"{OUT}/c-{name}.png")


def new_task(h, tool, text, access=None, folder=True):
    if h.find("newtask", secs=0.3) is None:
        h.tap("fabMain"); time.sleep(0.5)
        h.tap(name=tool, role="button"); time.sleep(0.5)
    if access:
        h.tap("nAccess"); time.sleep(0.5)
        h.tap(name=access, contains=True); time.sleep(0.4)
    h.tap("nInput"); h.type(text)
    if folder and "proj" not in (h.find("nFolder").get_name() or ""):
        h.tap("nFolder"); time.sleep(1.0)
    h.tap("nGo"); time.sleep(0.5)


def prompts(h, tool=None):
    return [(t, j) for t, tag, j in h.agent_log() if tag == "in" and j.get("method") == "session/prompt" and (tool is None or t == tool)]


def sent(h, method, tool=None):
    return [(t, j) for t, tag, j in h.agent_log() if tag == "in" and j.get("method") == method and (tool is None or t == tool)]


def labels(h):
    return [nm for n, i, nm, r in h.nodes() if r == "label"]
