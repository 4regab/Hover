import json, os, re, shutil, subprocess, sys, threading, time, queue, base64, tempfile

REPO = os.environ.get("HOVER_REPO") or os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
FAKE_AGENT = os.environ.get("FAKE_AGENT") or f"{REPO}/target/release/fake-agent"
FAKE_OC = os.environ.get("FAKE_OPENCODE") or f"{REPO}/target/release/fake-opencode"
FAKE_GH = os.environ.get("FAKE_GH") or "/tmp/fakegh"
KEY = bytes(range(32))

class Sandbox:
    def __init__(self, name):
        self.root = tempfile.mkdtemp(prefix=f"hb-{name}-")
        for d in ["data", "home", "bin", "project", "bin/script"]:
            os.makedirs(os.path.join(self.root, d), exist_ok=True)
        for n in ["codex-acp", "kiro-cli", "cua-driver"]:
            shutil.copy(FAKE_AGENT, os.path.join(self.root, "bin", n))
        shutil.copy(FAKE_OC, os.path.join(self.root, "bin", "opencode"))
        shutil.copy(FAKE_GH, os.path.join(self.root, "bin", "gh"))
        open(os.path.join(self.root, "gitconfig"), "w").write("")
        self.env_git = {"GIT_CONFIG_GLOBAL": os.path.join(self.root, "gitconfig"), "GIT_CONFIG_NOSYSTEM": "1",
                        "GIT_AUTHOR_NAME": "Test", "GIT_AUTHOR_EMAIL": "t@example.com", "GIT_COMMITTER_NAME": "Test", "GIT_COMMITTER_EMAIL": "t@example.com"}
        p = self.project
        open(os.path.join(p, "app.js"), "w").write("a\n")
        self.git("init", "-q"); self.git("add", "."); self.git("commit", "-qm", "init")
        open(os.path.join(p, "app.js"), "w").write("a\nb\n")
    @property
    def project(self): return os.path.join(self.root, "project")
    @property
    def data(self): return os.path.join(self.root, "data")
    def git(self, *a):
        subprocess.run(["git", *a], cwd=self.project, env={**os.environ, **self.env_git}, check=True, capture_output=True)
    def gh_script(self, key, lines):
        open(os.path.join(self.root, "bin/script", key + ".txt"), "w").write("\n".join(lines))
    def start(self, exe):
        home = os.path.join(self.root, "home")
        env = {**os.environ, **self.env_git, "HOVER_DATA_DIR": self.data, "PATH": os.path.join(self.root, "bin") + ":/usr/bin:/bin",
               "HOME": home, "USERPROFILE": home, "XDG_RUNTIME_DIR": home}
        for k in ["CODEX_HOME", "CLAUDE_CONFIG_DIR"]: env.pop(k, None)
        return Run(subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env), self)

class Run:
    def __init__(self, proc, sb):
        self.p, self.sb, self.q, self.seen = proc, sb, queue.Queue(), []
        threading.Thread(target=self._read, daemon=True).start()
    def _read(self):
        for line in self.p.stdout:
            try: self.q.put(json.loads(line))
            except Exception as e: self.q.put({"type": "NOT-JSON", "line": line.decode(errors="replace")})
        self.q.put(None)
    def send(self, m):
        self.p.stdin.write((json.dumps(m, ensure_ascii=False) + "\n").encode()); self.p.stdin.flush()
    def send_raw(self, s):
        self.p.stdin.write((s + "\n").encode()); self.p.stdin.flush()
    def until(self, what, f, timeout=40):
        end = time.time() + timeout
        while True:
            left = end - time.time()
            if left <= 0: raise TimeoutError(f"timed out waiting for {what}; last: {[json.dumps(x)[:160] for x in self.seen[-4:]]}")
            try: m = self.q.get(timeout=left)
            except queue.Empty: continue
            if m is None: raise EOFError(f"backend ended while waiting for {what}")
            self.seen.append(m)
            if f(m): return m
    def msg(self, t): return self.until(t, lambda m: m.get("type") == t)
    def state(self, what, f): return self.until(what, lambda m: m.get("type") == "state" and f(m))
    def drain(self, quiet=1.5):
        out = []
        while True:
            try: m = self.q.get(timeout=quiet)
            except queue.Empty: return out
            if m is None: return out
            self.seen.append(m); out.append(m)
    def close(self):
        try: self.p.stdin.close()
        except Exception: pass
        try: return self.p.wait(timeout=20)
        except Exception: self.p.kill(); return -9

def norm(x, root):
    s = json.dumps(x, ensure_ascii=False, sort_keys=False)
    s = s.replace(root, "<ROOT>")
    s = re.sub(r'[0-9a-f]{32}', '<KEY>', s)
    return json.loads(s)

def numbers_to_placeholder(x, keys=("t0", "at", "ms", "took", "woke")):
    if isinstance(x, dict):
        return {k: ("<N>" if k in keys and isinstance(v, (int, float)) else numbers_to_placeholder(v, keys)) for k, v in x.items()}
    if isinstance(x, list): return [numbers_to_placeholder(v, keys) for v in x]
    return x

def diff(a, b, path=""):
    out = []
    if type(a) != type(b): return [f"{path}: {json.dumps(a)[:100]} != {json.dumps(b)[:100]}"]
    if isinstance(a, dict):
        if list(a.keys()) != list(b.keys()):
            ka, kb = list(a.keys()), list(b.keys())
            if set(ka) != set(kb): out.append(f"{path}: keys differ: only rust {sorted(set(ka)-set(kb))}, only go {sorted(set(kb)-set(ka))}")
            else: out.append(f"{path}: key ORDER differs: {ka} vs {kb}")
        for k in a:
            if k in b: out += diff(a[k], b[k], f"{path}.{k}")
    elif isinstance(a, list):
        if len(a) != len(b): out.append(f"{path}: length {len(a)} != {len(b)}")
        for i, (x, y) in enumerate(zip(a, b)): out += diff(x, y, f"{path}[{i}]")
    elif a != b:
        out.append(f"{path}: {json.dumps(a)[:100]} != {json.dumps(b)[:100]}")
    return out
