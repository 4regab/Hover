import sys, json, time, base64, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *

def run_all(exe, name):
    sb = Sandbox(name)
    cap = {}   # name -> message
    r = sb.start(exe)
    r.send({"type": "initialize", "key": base64.b64encode(KEY).decode()})
    cap["initialized"] = r.msg("initialized")
    r.send({"type": "getSettings"}); cap["prefs1"] = r.msg("preferences")
    r.send({"type": "saveSettings", "noticeSeen": False, "hover": True}); cap["prefs2"] = r.msg("preferences")
    tools = [{"id": "codex", "access": "always", "idle": 15, "hideSteps": True}]
    r.send({"type": "saveSettings", "noticeSeen": True, "hover": False, "maxRunning": 4, "quotaItems": [], "computerUse": True, "agentBrowser": False, "tools": tools})
    cap["prefs3"] = r.msg("preferences")
    r.send({"type": "saveSettings", "maxRunning": 99, "sandbox": False, "kiroAutoCompact": True, "kiroCompactAt": 150, "discordPresence": False, "quotaItems": ["claude", "bogus", "kiro"]})
    cap["prefs4"] = r.until("prefs4", lambda m: m.get("type") == "preferences" and m.get("maxRunning") == 6)
    r.send({"type": "saveSettings", "maxRunning": 3, "sandbox": True, "kiroAutoCompact": False, "quotaItems": []})
    r.until("prefs5", lambda m: m.get("type") == "preferences" and m.get("maxRunning") == 3)
    r.send({"type": "ready"})
    cap["ready_state"] = r.state("codex ready", lambda m: any(t["id"] == "codex" and t["ready"] for t in m["tools"]))
    # a bad line and a command before init handled elsewhere; now a task with an ask
    folder = sb.project
    r.send({"type": "new", "tool": "codex", "folder": folder, "prompt": "Sandbox secret [ask:edit:fixture.txt]", "access": "always"})
    cap["waiting"] = r.state("waiting", lambda m: m["sessions"] and m["sessions"][0]["stage"] == "waiting")
    s = cap["waiting"]["sessions"][0]; sid = s["id"]; ask = s["ask"]
    r.send({"type": "answer", "id": sid, "ask": ask["id"], "answer": "allow"})
    cap["done"] = r.state("done", lambda m: m["sessions"] and m["sessions"][0]["stage"] == "done")
    for what, arg in [("probe", None), ("terminal", None), ("agents", None), ("browser", None), ("nonsense", None), ("files", None), ("diff", None), ("file", "app.js"), ("file", "../secret.txt"), ("pr", None), ("linked", None)]:
        m = {"type": "desk", "id": sid, "what": what}
        if arg: m["arg"] = arg
        r.send(m)
        cap[f"desk:{what}:{arg}"] = r.until(what, lambda x, w=what: x.get("type") == "desk" and x.get("what") == w)
    r.send({"type": "reply", "id": sid, "text": "Continue [ask:edit:second.txt]"})
    w2 = r.state("second ask", lambda m: m["sessions"] and m["sessions"][0]["stage"] == "waiting")
    r.send({"type": "answer", "id": sid, "ask": w2["sessions"][0]["ask"]["id"], "answer": "allow"})
    cap["two"] = r.state("two turns done", lambda m: m["sessions"] and len(m["sessions"][0]["turns"]) == 2 and m["sessions"][0]["stage"] == "done")
    ended = [m for m in r.seen if m.get("type") == "ended"]
    cap["ended0"] = ended[0] if ended else None
    cap["ended_count"] = len(ended)
    r.send({"type": "gh"}); cap["gh"] = r.msg("gh")
    r.send({"type": "computerUse"}); cap["computerUse"] = r.msg("computerUse")
    r.send({"type": "spaces"}); cap["spaces"] = r.msg("spaces")
    r.send({"type": "setModel", "tool": "codex", "model": "gpt-x", "effort": "high"})
    cap["setModel"] = r.state("model", lambda m: any(t["id"] == "codex" and t["model"] == "gpt-x" for t in m["tools"]))
    r.send({"type": "setup", "tool": "codex", "step": "cancel"})
    r.send({"type": "bogus"})
    r.send_raw("this is not json")
    cap["badline"] = r.msg("toast")
    r.send({"type": "new", "tool": "codex", "folder": "/nonexistent-folder", "prompt": "x"})
    cap["badfolder"] = r.msg("toast")
    r.send({"type": "shutdown"})
    code = r.close()
    cap["exit_code"] = code
    hist = b"".join(open(os.path.join(sb.data, "agents", f), "rb").read() for f in sorted(os.listdir(os.path.join(sb.data, "agents"))) if f.endswith(".dat"))
    cap["history_sealed"] = (len(hist) > 0 and b"Sandbox secret" not in hist)
    # a different key
    bad = sb.start(exe)
    bad.send({"type": "initialize", "key": base64.b64encode(bytes([255]) * 32).decode()})
    cap["badkey"] = bad.msg("backendFailure")
    cap["badkey_exit"] = bad.close()
    # restart
    r = sb.start(exe)
    r.send({"type": "initialize", "key": base64.b64encode(KEY).decode()}); r.msg("initialized")
    r.send({"type": "ready"})
    st = r.state("history", lambda m: len(m["history"]) == 1)
    cap["restart_state"] = st
    key = st["history"][0]["key"]
    r.send({"type": "history", "key": key}); cap["transcript"] = r.msg("transcript")
    r.send({"type": "reply", "key": key, "text": "Once more [seconds:0.2]"})
    cap["woke"] = r.state("woken", lambda m: m["sessions"] and len(m["sessions"][0]["turns"]) == 3 and m["sessions"][0]["stage"] == "done")
    r.send({"type": "delete", "key": key})
    cap["empty"] = r.state("empty", lambda m: not m["history"] and not m["sessions"])
    r.close()
    # first-before-init and a bad line before init
    pre = sb.start(exe)
    pre.send({"type": "ready"}); cap["pre_init"] = pre.msg("backendFailure")
    pre.close()
    return cap, sb

