#!/usr/bin/env python3
"""Measure-Hover.ps1's scenarios (BENCHMARK.md section 2) for the native build on Linux.

    python3 port/bench/measure-hover.py [--runs 5] [--out results.json] [--quick]

Runs `native/target/release/hover` on its own X server (Xvfb) with a fresh data folder
holding the 200-turn history fixture (`hover-data write`), and port/bench/fake-acp.py as
kiro-cli. Drives it through HOVER_BENCH's stdin channel (the shortcut's own path, the
history panel's), and samples /proc once a second.

Metrics, and how they map to Windows's (section 3):
  private WS      Private_Clean + Private_Dirty of /proc/<pid>/smaps_rollup
  private commit  RssAnon + VmSwap of /proc/<pid>/status (anonymous memory: what
                  PrivateMemorySize64 counts)
  handles         open file descriptors
  CPU             utime + stime from /proc/<pid>/stat
  GPU memory      none separate: the GPU here is software (lavapipe), so its memory is
                  in the process's own; on Windows it is reported apart
Application is the hover process (it spawns nothing for drawing); Providers are every
process under it (the agents), reported apart. --quick shortens S2's 45 s and S6's
loops for a smoke run; the reported results use the full steps.
"""
import argparse, json, os, shutil, statistics, subprocess, sys, tempfile, threading, time

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BIN = os.path.join(REPO, "native", "target", "release")
TICK = os.sysconf("SC_CLK_TCK")
PAGE = os.sysconf("SC_PAGE_SIZE")


def smaps(pid):
    out = {}
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                k, _, v = line.partition(":")
                if v.strip().endswith("kB"):
                    out[k] = int(v.split()[0]) * 1024
    except OSError:
        pass
    return out


def status(pid):
    out = {}
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                k, _, v = line.partition(":")
                parts = v.split()
                if len(parts) == 2 and parts[1] == "kB":
                    out[k] = int(parts[0]) * 1024
    except OSError:
        pass
    return out


def cpu(pid):
    try:
        with open(f"/proc/{pid}/stat") as f:
            s = f.read().rsplit(")", 1)[1].split()
        return (int(s[11]) + int(s[12])) / TICK
    except OSError:
        return 0.0


def fds(pid):
    try:
        return len(os.listdir(f"/proc/{pid}/fd"))
    except OSError:
        return 0


def children(pid):
    """Every process under pid (the providers)."""
    kids, frontier = [], [pid]
    all_pids = [int(p) for p in os.listdir("/proc") if p.isdigit()]
    parent = {}
    for p in all_pids:
        try:
            with open(f"/proc/{p}/stat") as f:
                parent[p] = int(f.read().rsplit(")", 1)[1].split()[1])
        except OSError:
            pass
    # Tools lead their own process group (setsid) but stay Hover's children.
    while frontier:
        cur = frontier.pop()
        for p, pp in parent.items():
            if pp == cur:
                kids.append(p)
                frontier.append(p)
    return kids


def sample(pid):
    s, st = smaps(pid), status(pid)
    app = {"ws": s.get("Private_Clean", 0) + s.get("Private_Dirty", 0), "commit": st.get("RssAnon", 0) + st.get("VmSwap", 0), "handles": fds(pid), "cpu": cpu(pid)}
    prov = {"ws": 0, "commit": 0, "cpu": 0.0, "n": 0}
    for k in children(pid):
        ks, kt = smaps(k), status(k)
        prov["ws"] += ks.get("Private_Clean", 0) + ks.get("Private_Dirty", 0)
        prov["commit"] += kt.get("RssAnon", 0) + kt.get("VmSwap", 0)
        prov["cpu"] += cpu(k)
        prov["n"] += 1
    return {"t": time.time(), "app": app, "prov": prov}


class Hover:
    def __init__(self, data, env):
        self.lines, self.cv = [], threading.Condition()
        self.t0 = time.time()
        self.p = subprocess.Popen([os.path.join(BIN, "hover")], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        for line in self.p.stdout:
            with self.cv:
                self.lines.append((time.time(), line.strip()))
                self.cv.notify_all()

    def send(self, cmd):
        self.p.stdin.write(cmd + "\n")
        self.p.stdin.flush()

    def wait(self, prefix, timeout=30, after=0):
        end = time.time() + timeout
        with self.cv:
            while True:
                for t, l in self.lines[after:]:
                    if l.startswith(prefix):
                        return t, l
                left = end - time.time()
                if left <= 0:
                    raise TimeoutError(prefix)
                self.cv.wait(left)

    def ask(self, cmd, prefix, timeout=30):
        n = len(self.lines)
        self.send(cmd)
        return self.wait(prefix, timeout, n)[1]

    def quit(self):
        try:
            self.send("quit")
            self.p.wait(15)
        except Exception:
            self.p.kill()


def samples(h, secs):
    out = []
    for _ in range(int(secs)):
        out.append(sample(h.p.pid))
        time.sleep(1)
    return out


def summary(ss):
    med = lambda k, g="app": statistics.median(s[g][k] for s in ss) if ss else 0
    dcpu = (ss[-1]["app"]["cpu"] - ss[0]["app"]["cpu"]) if len(ss) > 1 else 0
    return {"private_ws_mb": round(med("ws") / 2**20, 1), "private_commit_mb": round(med("commit") / 2**20, 1), "handles": int(med("handles")),
            "cpu_s": round(dcpu, 3), "providers_ws_mb": round(med("ws", "prov") / 2**20, 1), "providers": int(max(s["prov"]["n"] for s in ss)) if ss else 0}


def frames(h, secs=10):
    l = h.ask(f"frames {secs}", "bench frames").split()
    return {"office_frames": int(l[2]), "p50_ms": float(l[3]), "p95_ms": float(l[4]), "p99_ms": float(l[5]), "notch_frames": int(l[6])}


def run_once(args, run):
    work = tempfile.mkdtemp(prefix="hover-bench-")
    data, proj, tools, rt = [os.path.join(work, d) for d in ("data", "project", "bin", "run")]
    for d in (proj, tools, rt):
        os.makedirs(d, mode=0o700)
    shutil.copy(os.path.join(REPO, "port", "bench", "fake-acp.py"), os.path.join(tools, "kiro-cli"))
    os.chmod(os.path.join(tools, "kiro-cli"), 0o755)
    w = subprocess.run([os.path.join(BIN, "hover-data"), "write", data, proj, "200", os.path.join(REPO, "native", "golden", "fixtures", "rich.md")], capture_output=True, text=True)
    key = w.stdout.strip().split()[-1]
    disp = f":{50 + run}"
    xv = subprocess.Popen(["Xvfb", disp, "-screen", "0", "1920x1080x24", "+extension", "GLX", "+extension", "RANDR"], stderr=subprocess.DEVNULL)
    time.sleep(1.5)
    env = dict(os.environ, DISPLAY=disp, HOVER_DATA_DIR=data, XDG_RUNTIME_DIR=rt, HOVER_BENCH="1", PATH=tools + ":" + os.environ["PATH"],
               FAKEACP_SECONDS="30", FAKEACP_RATE="20", HOME=work)
    env.pop("WAYLAND_DISPLAY", None)
    env.pop("DBUS_SESSION_BUS_ADDRESS", None)
    r = {}
    h = Hover(data, env)
    try:
        # S1: cold start until the notch is placed and shown; memory 10 s later.
        t, _ = h.wait("bench visible", 60)
        r["S1"] = {"startup_ms": round((t - h.t0) * 1000, 1)}
        time.sleep(10)
        r["S1"].update(summary([sample(h.p.pid)]))
        # S2: open, 5 s, collapse, 45 s (the C# app's 30 s WebView2 drop plus 15), 10 s of samples.
        h.ask("toggle", "bench toggled")
        time.sleep(5)
        h.ask("toggle", "bench toggled")
        time.sleep(15 if args.quick else 45)
        idle = samples(h, 10)
        r["S2"] = summary(idle)
        r["S2"].update(frames(h, 10))
        # 60 s settled idle: CPU and presents (G3), counted from the notch's own frames.
        n0 = frames(h, 1)["notch_frames"]
        c0 = cpu(h.p.pid)
        time.sleep(20 if args.quick else 60)
        r["idle_settled"] = {"cpu_s_per_60s": round((cpu(h.p.pid) - c0) * (3 if args.quick else 1), 3), "presents_per_s": round((frames(h, 1)["notch_frames"] - n0) / (20 if args.quick else 60), 3)}
        # S3: office visible 10 s, then 10 s of samples and frame times.
        h.ask("toggle", "bench toggled")
        t, l = h.wait("bench reopen", 30, len(h.lines) - 1)
        r["S3"] = {"reopen_ms": float(l.split()[2])}
        time.sleep(10)
        r["S3"].update(summary(samples(h, 10)))
        r["S3"].update(frames(h, 10))
        c0, f0 = cpu(h.p.pid), frames(h, 60)["office_frames"]
        time.sleep(20 if args.quick else 60)
        f1 = frames(h, 60 if not args.quick else 20)["office_frames"]
        r["idle_office"] = {"cpu_s_per_60s": round((cpu(h.p.pid) - c0) * (3 if args.quick else 1), 3), "office_frames_per_s": round(f1 / (20 if args.quick else 60), 2)}
        # S4: the 200-turn session from the history; its opening time; scroll to the top and back.
        h.ask("history", "bench ok")
        n = len(h.lines)
        h.send(f"open {key}")
        t, l = h.wait("bench opened", 30, n)
        r["S4"] = {"open_ms": float(l.split()[2])}
        for dy in (1e7, -1e7):
            h.ask(f"scroll {dy}", "bench ok")
            time.sleep(1)
        time.sleep(3)
        r["S4"].update(summary(samples(h, 10)))
        r["S4"].update(frames(h, 10))
        # S5: three sessions streaming 20 updates a second for 30 s, one of them open.
        started = h.ask(f"start {proj} 3", "bench started")
        time.sleep(5)
        during = samples(h, 10)
        r["S5"] = {"started": started.split()[-1], "during": summary(during), "frames_during": frames(h, 10)}
        for _ in range(60):
            if h.ask("running", "bench running").split()[-1] == "0":
                break
            time.sleep(1)
        r["S5"]["after"] = summary(samples(h, 10))
        # S6: 20 × (collapse, 2 s, open, 2 s) with each reopen's latency; 10 × (start, stop after 3 s).
        lat = []
        loops = 5 if args.quick else 20
        if not h.ask("running", "bench running").endswith(" 0"):
            h.ask("stopall", "bench ok")
        for _ in range(loops):
            h.ask("toggle", "bench toggled")
            time.sleep(2)
            n = len(h.lines)
            h.send("toggle")
            t, l = h.wait("bench reopen", 30, n)
            lat.append(float(l.split()[2]))
            time.sleep(2)
        for _ in range(3 if args.quick else 10):
            h.ask(f"start {proj} 1", "bench started")
            time.sleep(3)
            h.ask("stopall", "bench ok")
            time.sleep(1)
        time.sleep(3)
        r["S6"] = {"reopen_ms_median": statistics.median(lat), "reopen_ms_max": max(lat), "after": summary(samples(h, 10))}
        r["S6"]["growth_vs_S3_mb"] = round(r["S6"]["after"]["private_ws_mb"] - r["S3"]["private_ws_mb"], 1)
    finally:
        h.quit()
        xv.kill()
        shutil.rmtree(work, ignore_errors=True)
    return r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--out", default="bench-results.json")
    ap.add_argument("--quick", action="store_true")
    # A full run is about seven minutes; --append lets the five be taken one at a time
    # (a sandbox's command limit is shorter than 35 minutes) into the same file.
    ap.add_argument("--append", action="store_true")
    args = ap.parse_args()
    runs = []
    if args.append and os.path.exists(args.out):
        runs = json.load(open(args.out))["runs"]
    for i in range(args.runs):
        r = run_once(args, len(runs))
        print(json.dumps(r), flush=True)
        runs.append(r)
    json.dump({"runs": runs, "median": median(runs), "host": list(os.uname()), "quick": args.quick}, open(args.out, "w"), indent=1)


def median(runs):
    """BENCHMARK.md reports each metric's median over the runs; strings keep the first run's."""
    first = runs[0]
    if isinstance(first, dict):
        return {k: median([r[k] for r in runs if k in r]) for k in first}
    if isinstance(first, (int, float)):
        return round(statistics.median(runs), 3)
    return first


if __name__ == "__main__":
    main()
