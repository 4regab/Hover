"""Runs the Rust hover-backend and the Go one through the same scripted sessions (the
protocol tests of crates/hover-backend, replayed against both) and compares every message
they send, field by field and in order. Exits 1 on any difference that isn't one of the few
that are timing or randomness. Needs the Rust build's target/release/hover-backend,
fake-agent and fake-opencode, a fake gh (crates/hover-agents/tests/fixtures/fakegh.rs built
with rustc, FAKE_GH), the Go one (HB_GO) and git.

    python3 go/tools/backend-diff/run.py
"""
import json, os, re, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import REPO, diff, numbers_to_placeholder
import scenario_task, scenario_rest

RUST = os.environ.get("HB_RUST") or f"{REPO}/target/release/hover-backend"
GO = os.environ.get("HB_GO") or "/tmp/hb-go"

# What differs between two runs of the same program: random names, and a state captured
# before the tools' checks finished.
IGNORE = [r"^restart_state\.tools", r"^image:turn", r"^ready_state\.tools",
          r"^stop:state\.sessions\[0\]\.(act|pose|file|turns\[0\]\.(steps|woke))"]

def clean(x, root):
    s = json.dumps(x, ensure_ascii=False)
    s = s.replace(root, "<ROOT>")
    s = re.sub(r"/tmp/hb-[A-Za-z0-9_-]+", "<ROOT>", s)
    s = re.sub(r"[0-9a-f]{32}", "<KEY>", s)
    return numbers_to_placeholder(json.loads(s))

def main():
    total, seen = 0, 0
    for mod in (scenario_task, scenario_rest):
        out = {}
        for nm, exe in (("rust", RUST), ("go", GO)):
            cap, sb = mod.run_all(exe, nm)
            out[nm] = (cap, sb.root)
            print(f"{mod.__name__}: {nm} ran, {len(cap)} captures")
        (rc, rr), (gc, gr) = out["rust"], out["go"]
        for k in rc:
            seen += 1
            for line in diff(clean(rc[k], rr), clean(gc.get(k), gr), k):
                if any(re.search(p, line) for p in IGNORE):
                    continue
                total += 1
                print("DIFF", line)
    print(f"{seen} captures compared, {total} differences")
    sys.exit(1 if total else 0)

main()
