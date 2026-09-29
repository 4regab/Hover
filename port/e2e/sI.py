"""Quotas (Kiro and Codex) in Integrations and the resting island, Appearance, a second launch opening the app window."""
from drive import *
import datetime
D = os.path.join(WORK, "dI")
h = Hover(D)
home = h.env["HOME"]
os.makedirs(home + "/.codex/sessions/2026/09/29", exist_ok=True)
now = datetime.datetime.now(datetime.timezone.utc)
ts = (now - datetime.timedelta(minutes=5)).strftime("%Y-%m-%dT%H:%M:%S.000Z")
line = json.dumps({"timestamp": ts, "type": "event_msg", "payload": {"type": "token_count", "rate_limits": {"primary": {"used_percent": 37.5, "window_minutes": 300, "resets_at": int(now.timestamp()) + 7200}, "secondary": {"used_percent": 12, "window_minutes": 10080, "resets_in_seconds": 3600}}}})
open(home + "/.codex/sessions/2026/09/29/rollout-x.jsonl", "w").write(line + "\n")
def st(): return json.load(open(D + "/settings.json", encoding="utf-8-sig"))
try:
    h.cmd("toggle"); time.sleep(1.5)
    h.tap("menuBtn"); time.sleep(0.4); h.tap("setBtn"); time.sleep(1.0)
    h.tap("SectionIntegrations"); time.sleep(0.8)
    h.tap("NotchItemkiro"); time.sleep(0.5); h.tap("NotchItemcodex"); time.sleep(0.5)
    check("quota items saved", wait(lambda: set(st().get("NotchItems") or []) >= {"kiro", "codex"}, 3), str(st().get("NotchItems")))
    ok = wait(lambda: [nm for n, i, nm, r in h.nodes() if i == "QuotaStatuskiro" and "42" in nm], 20)
    print("statuses:", [(i, nm) for n, i, nm, r in h.nodes() if i.startswith("QuotaStatus")])
    check("Kiro quota read (42%)", ok)
    check("Codex quota read (38%)", [nm for n, i, nm, r in h.nodes() if i == "QuotaStatuscodex" and "38" in nm])
    h.shot("sI-integrations"); crop("sI-integrations")
    # Appearance dark
    h.tap("SectionGeneral"); time.sleep(0.8)
    for _ in range(8): h.click(1200, 350, button=5); time.sleep(0.05)
    time.sleep(0.8)
    print([i for n, i, nm, r in h.nodes() if i.startswith("Appearance") or i.startswith("Theme")])
    d = h.find("AppearanceDark", secs=1)
    if d: h.click_node(d); time.sleep(1.0)
    check("appearance dark saved", wait(lambda: st().get("Appearance") == "Dark", 3), st().get("Appearance"))
    h.shot("sI-dark"); crop("sI-dark")
    # back and close; the resting island shows the rings
    h.key("Escape"); time.sleep(1.2); h.focus_helper()
    h.shot("sI-rest"); Image.open(OUT + "/sI-rest.png").crop((760, 0, 1160, 50)).resize((800, 100)).save(OUT + "/c-sI-rest.png")
    print("rest:", [l for l in h.dump() if "HoverNotch" not in l][:10])
    # second launch opens the app window
    p2 = subprocess.run([HOVER], env=h.env, capture_output=True, text=True, timeout=20)
    print("second launch exit", p2.returncode, p2.stdout[-200:], p2.stderr[-300:])
    time.sleep(2)
    frames = [(nm, h.rect(n)) for n, i, nm, r in h.nodes() if r == "frame"]
    print("frames:", frames)
    check("second launch opens the app window", any("Hover" == nm or "notch" not in nm.lower() for nm, _ in frames if nm != "Hover notch"), str(frames))
    h.shot("sI-dash"); Image.open(OUT + "/sI-dash.png").resize((960, 540)).save(OUT + "/c-sI-dash.png")
finally:
    print("quit code", h.quit()); h.close()
summary()
