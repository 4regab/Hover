import sys, json, time, base64, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from harness import *

PNG = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="
K = lambda b=KEY: base64.b64encode(b).decode()
def done_s(m): return bool(m["sessions"]) and m["sessions"][0]["stage"] == "done"
def stage_s(st): return lambda m: bool(m["sessions"]) and m["sessions"][0]["stage"] == st

def init(r, key=KEY):
    r.send({"type": "initialize", "key": K(key)}); return r.msg("initialized")
def notice(r):
    r.send({"type": "saveSettings", "noticeSeen": True}); r.msg("preferences")
def ready(r, tool):
    r.send({"type": "ready"}); return r.state("ready", lambda m: any(t["id"] == tool and t["ready"] for t in m["tools"]))

def run_all(exe, name):
    cap = {}
    # --- init edge cases
    sb = Sandbox(name + "-init"); r = sb.start(exe)
    r.send({"type": "ready"}); cap["init:pre"] = r.msg("backendFailure")
    r.send_raw("this is not json"); cap["init:badjson"] = r.msg("toast")
    r.send({"type": "initialize", "key": base64.b64encode(bytes([1, 2, 3])).decode()}); cap["init:shortkey"] = r.msg("backendFailure")
    r.send({"type": "initialize", "key": "not base64!"}); cap["init:b64"] = r.msg("backendFailure")
    init(r, bytes([7]) * 32)
    r.send({"type": "initialize", "key": K(bytes([9]) * 32)})
    r.send({"type": "getSettings"}); cap["init:prefs"] = r.msg("preferences")
    r.send([1, 2]); cap["init:array"] = r.msg("toast")
    r.close()
    # --- refusals
    sb = Sandbox(name + "-refuse"); folder = sb.project; r = sb.start(exe); init(r)
    def says(m, key):
        r.send(m); cap[key] = r.msg("toast")
    says({"type": "new", "tool": "codex", "folder": "relative/dir", "prompt": "x"}, "refuse:folder")
    says({"type": "new", "tool": "codex", "folder": folder, "prompt": "x"}, "refuse:notice")
    notice(r)
    says({"type": "new", "tool": "codex", "folder": folder, "prompt": "x"}, "refuse:notready")
    ready(r, "codex")
    says({"type": "new", "tool": "cursor", "folder": folder, "prompt": "x"}, "refuse:cursor")
    says({"type": "new", "tool": "codex", "folder": folder, "prompt": "  "}, "refuse:prompt")
    says({"type": "reply", "id": 9999, "text": "hi"}, "refuse:reply")
    says({"type": "restore", "id": 9999, "turn": 0}, "refuse:restore")
    r.close()
    # --- picture
    sb = Sandbox(name + "-image"); folder = sb.project; r = sb.start(exe); init(r); notice(r); ready(r, "codex")
    r.send({"type": "new", "tool": "codex", "folder": folder, "prompt": "Look [seconds:0.2]", "images": [PNG, {"data": PNG}, "data:text/plain;base64,AAAA", "not an image"]})
    d = r.state("done", done_s); cap["image:turn"] = d["sessions"][0]["turns"][0]["images"]
    cap["image:kept"] = len(os.listdir(os.path.join(sb.data, "kiro-images")))
    r.close()
    # --- stop and hang up
    sb = Sandbox(name + "-stop"); folder = sb.project; r = sb.start(exe); init(r); notice(r); ready(r, "codex")
    r.send({"type": "new", "tool": "codex", "folder": folder, "prompt": "Cancel me [ask:edit:a.txt]", "access": "always"})
    w = r.state("waiting", stage_s("waiting"))
    r.send({"type": "stop", "id": w["sessions"][0]["id"]})
    cap["stop:state"] = r.state("stopped", stage_s("stopped"))
    cap["stop:no-file"] = not os.path.exists(os.path.join(folder, "a.txt"))
    r.send({"type": "new", "tool": "codex", "folder": folder, "prompt": "UI crash [ask:edit:b.txt]", "access": "always"})
    r.state("second approval", lambda m: any(s["stage"] == "waiting" for s in m["sessions"]))
    cap["stop:exit"] = r.close()
    time.sleep(1.0)
    left = subprocess.run(["pgrep", "-f", os.path.join(sb.root, "bin")], capture_output=True, text=True).stdout.split()
    cap["stop:left_running"] = len(left)
    r = sb.start(exe); init(r); r.send({"type": "ready"})
    cap["stop:history"] = r.state("history", lambda m: bool(m["history"]))["history"]
    r.close()
    # --- gh
    sb = Sandbox(name + "-gh")
    sb.gh_script("version", ["out=gh version 2.102.0 (2026-09-30)"])
    sb.gh_script("auth_status", ["out=github.com", "out=  \u2713 Logged in to github.com account octocat (keyring)"])
    r = sb.start(exe); init(r)
    r.send({"type": "gh"}); cap["gh:unknown"] = r.msg("gh")
    cap["gh:known"] = r.until("checked", lambda m: m.get("type") == "gh" and m.get("checked"))
    r.close()
    # --- pull request
    sb = Sandbox(name + "-pr"); folder = sb.project
    sb.gh_script("version", ["out=gh version 2.102.0 (2026-09-30)"])
    sb.gh_script("auth_status", ["out=  \u2713 Logged in to github.com account octocat (keyring)"])
    sb.gh_script("pr_view", ['err=no pull requests found for branch "main"', "exit=1"])
    sb.gh_script("pr_create", ["out=https://github.com/octo/demo/pull/7"])
    remote = os.path.join(sb.root, "remote.git"); os.makedirs(remote)
    subprocess.run(["git", "init", "-q", "--bare"], cwd=remote, env={**os.environ, **sb.env_git}, check=True)
    sb.git("branch", "-M", "main"); sb.git("remote", "add", "origin", remote); sb.git("push", "-q", "-u", "origin", "main")
    r = sb.start(exe); init(r); notice(r); ready(r, "codex")
    r.send({"type": "new", "tool": "codex", "folder": folder, "prompt": "Change it", "access": "full"})
    d = r.state("done", done_s); sid = d["sessions"][0]["id"]
    r.send({"type": "desk", "id": sid, "what": "pr"}); cap["pr:panel"] = r.until("pr", lambda m: m.get("type") == "desk" and m.get("what") == "pr")
    r.send({"type": "deskAction", "id": sid, "what": "prCreate", "args": {"title": "Add b", "body": "It adds b.", "branch": "hover/add-b", "commit": True, "draft": False}})
    cap["pr:made"] = r.msg("deskAction")
    r.send({"type": "deskAction", "id": sid, "what": "nothing"})
    r.send({"type": "desk", "id": sid, "what": "linked"}); cap["pr:linked"] = r.until("linked", lambda m: m.get("type") == "desk" and m.get("what") == "linked")
    r.send({"type": "shutdown"}); cap["pr:exit"] = r.close()
    # --- opencode
    sb = Sandbox(name + "-oc"); folder = sb.project; r = sb.start(exe); init(r); notice(r)
    rd = ready(r, "opencode"); cap["oc:tool"] = [t for t in rd["tools"] if t["id"] == "opencode"][0]
    r.send({"type": "new", "tool": "opencode", "folder": folder, "prompt": "Say hello", "access": "full"})
    d = r.state("done", done_s); cap["oc:session"] = d["sessions"][0]
    cap["oc:tool_after"] = [t for t in d["tools"] if t["id"] == "opencode"][0]
    r.send({"type": "shutdown"}); r.close()
    # --- quotas
    sb = Sandbox(name + "-q"); r = sb.start(exe); init(r)
    r.send({"type": "saveSettings", "quotaItems": ["kiro", "codex"]}); cap["quotas"] = r.msg("quotas")
    r.close()
    # --- setModel
    sb = Sandbox(name + "-m"); r = sb.start(exe); init(r)
    r.send({"type": "setModel", "tool": "kiro", "model": "claude-sonnet-5", "effort": "low"}); cap["model:1"] = r.msg("state")
    r.send({"type": "setModel", "tool": "kiro", "model": "", "effort": None}); cap["model:2"] = r.msg("state")
    r.close()
    return cap, sb

