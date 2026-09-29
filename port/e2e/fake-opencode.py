#!/usr/bin/env python3
"""A stand-in "opencode" for Hover's end-to-end run: `--version`, and `serve` with the
routes, Basic auth and event stream Hover uses (OpenCodeHost). Every request is logged
to $E2E_LOG as `opencode in {...}`. The prompt picks what the "model" does by keyword:
edit, ask (a command to approve), question (tabs or spaces), slow (runs until
aborted); anything else answers in one line."""
import base64, itertools, json, os, sys, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs, unquote

LOG = os.environ.get("E2E_LOG", "/dev/null")
args = sys.argv[1:]

def log(tag, obj):
    with open(LOG, "a") as f:
        f.write(f"opencode {tag} {json.dumps(obj)}\n")

log("argv", args)
if args[:1] == ["--version"]:
    print("1.18.31"); sys.exit(0)
if args[:1] != ["serve"]:
    sys.exit(2)

PASSWORD = os.environ.get("OPENCODE_SERVER_PASSWORD", "")
AUTH = "Basic " + base64.b64encode(("opencode:" + PASSWORD).encode()).decode()
PROVIDERS = {"providers": [{"id": "p", "name": "Prov", "models": {
    "a": {"id": "a", "name": "A B", "variants": {"low": {}, "high": {}}, "limit": {"context": 10000}},
    "m": {"id": "m", "name": "M", "limit": {"context": 10000}}}}], "default": {"p": "m"}}
AGENTS = [{"name": "build", "mode": "primary", "permission": [{"permission": "*", "pattern": "*", "action": "allow"}]},
          {"name": "plan", "mode": "primary", "permission": [{"permission": "edit", "pattern": "*", "action": "deny"}]}]
lock = threading.Lock()
streams = []
sessions = {}   # id -> {"busy": bool, "messages": [...]}
waiting = {}    # request id -> (kind, session, event, box)
ids = itertools.count(1)

def push(ev):
    line = ("data: " + json.dumps(ev) + "\n\n").encode()
    with lock:
        for w in list(streams):
            try: w.write(line); w.flush()
            except Exception: streams.remove(w)

def status(sid, t): push({"type": "session.status", "properties": {"sessionID": sid, "status": {"type": t}}})

def finish(sid, mid, text, tool=None):
    am = "msg_zz" + mid[6:]
    push({"type": "message.updated", "properties": {"sessionID": sid, "info": {"id": am, "parentID": mid, "role": "assistant", "sessionID": sid,
          "providerID": "p", "modelID": "a", "tokens": {"input": 900, "output": 100, "cache": {"read": 0, "write": 0}}}}})
    if tool:
        push({"type": "message.part.updated", "properties": {"sessionID": sid, "part": {"id": "prt_t" + mid[-4:], "messageID": am, "sessionID": sid, "type": "tool",
              "tool": tool[0], "callID": "call_" + mid[-6:], "state": {"status": "completed", "input": tool[1], "title": tool[2], "output": tool[3] if len(tool) > 3 else None}}}})
    push({"type": "message.part.updated", "properties": {"sessionID": sid, "part": {"id": "prt_x" + mid[-4:], "messageID": am, "sessionID": sid, "type": "text", "text": text}}})
    sessions[sid]["busy"] = False
    status(sid, "idle")

def ask(kind, sid, props):
    rid = ("per_" if kind == "permission" else "que_") + str(next(ids))
    ev, box = threading.Event(), {}
    waiting[rid] = (kind, sid, ev, box)
    push({"type": kind + ".asked", "properties": dict(props, id=rid, sessionID=sid)})
    while not ev.wait(0.2):
        if not sessions[sid]["busy"]: return None
    return box.get("answer")

def run(sid, mid, text):
    push({"type": "message.updated", "properties": {"sessionID": sid, "info": {"id": mid, "role": "user", "sessionID": sid}}})
    sessions[sid]["busy"] = True
    status(sid, "busy")
    time.sleep(0.4)
    t = text.lower()
    if "slow" in t:
        while sessions[sid]["busy"]: time.sleep(0.1)
        return
    if "question" in t:
        a = ask("question", sid, {"questions": [{"header": "Indent", "question": "Tabs or spaces?", "options": [
            {"label": "Tabs", "description": "Indent with tab characters"}, {"label": "Spaces", "description": ""}], "multiple": False, "custom": True}]})
        if not sessions[sid]["busy"]: return
        return finish(sid, mid, "You skipped it." if a is None else "You picked " + a[0][0] + ".")
    if "ask" in t:
        a = ask("permission", sid, {"permission": "bash", "patterns": ["npm install three"], "metadata": {"command": "npm install three"}, "always": ["npm install *"]})
        if not sessions[sid]["busy"]: return
        return finish(sid, mid, "Installed." if a == "once" else "Not installed.", ("bash", {"command": "npm install three"}, "npm install three", "added 1 package"))
    if "edit" in t:
        return finish(sid, mid, "Wrote hello.txt.", ("write", {"filePath": os.path.join(os.environ.get("E2E_PROJ", "/tmp"), "hello.txt"), "content": "hi\n"}, "hello.txt"))
    finish(sid, mid, "Done: " + text[:40])

class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    def log_message(self, *a): pass

    def send(self, code, obj=None):
        body = b"" if obj is None else json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        if body: self.wfile.write(body)

    def handle_one(self, method):
        u = urlparse(self.path)
        q = parse_qs(u.query)
        n = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(n)) if n else None
        parts = [unquote(p) for p in u.path.strip("/").split("/")]
        log("in", {"method": method, "path": u.path, "directory": (q.get("directory") or [None])[0], "body": body, "auth": self.headers.get("Authorization") == AUTH})
        if self.headers.get("Authorization") != AUTH: return self.send(401, {})
        p = tuple(parts)
        if method == "GET" and p == ("global", "health"): return self.send(200, {"healthy": True, "version": "1.18.31"})
        if method == "GET" and p == ("config", "providers"): return self.send(200, PROVIDERS)
        if method == "GET" and p == ("config",): return self.send(200, {})
        if method == "GET" and p == ("agent",): return self.send(200, AGENTS)
        if method == "GET" and p in (("permission",), ("question",)): return self.send(200, [])
        if method == "GET" and p == ("event",):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(b'data: {"type":"server.connected","properties":{}}\n\n'); self.wfile.flush()
            with lock: streams.append(self.wfile)
            while self.wfile in streams: time.sleep(0.5)
            return
        if method == "POST" and p == ("session",):
            sid = "ses_%d" % next(ids)
            sessions[sid] = {"busy": False}
            return self.send(200, {"id": sid})
        if method == "GET" and p == ("session", "status"):
            return self.send(200, {s: {"type": "busy"} for s, v in sessions.items() if v["busy"]})
        if len(p) == 2 and p[0] == "session":
            if p[1] not in sessions: return self.send(404, {"name": "NotFoundError", "data": {"message": "Session not found"}})
            return self.send(200, {"id": p[1]})
        if method == "GET" and len(p) == 3 and p[2] == "message": return self.send(200, [])
        if method == "POST" and len(p) == 3 and p[2] == "prompt_async":
            threading.Thread(target=run, args=(p[1], body["messageID"], body["parts"][0]["text"]), daemon=True).start()
            return self.send(204)
        if method == "POST" and len(p) == 3 and p[2] == "abort":
            if p[1] in sessions and sessions[p[1]]["busy"]:
                sessions[p[1]]["busy"] = False
                push({"type": "session.error", "properties": {"sessionID": p[1], "error": {"name": "MessageAbortedError", "data": {"message": "aborted"}}}})
                status(p[1], "idle")
            return self.send(200, True)
        if method == "POST" and len(p) == 3 and p[0] in ("permission", "question") and p[2] in ("reply", "reject"):
            w = waiting.pop(p[1], None)
            self.send(200, True)
            if w:
                w[3]["answer"] = None if p[2] == "reject" else (body.get("reply") if p[0] == "permission" else body.get("answers"))
                push({"type": p[0] + (".rejected" if p[2] == "reject" else ".replied"), "properties": {"sessionID": w[1], "requestID": p[1]}})
                w[2].set()
            return
        return self.send(404, {"name": "NotFoundError", "data": {"message": "no route"}})

    def do_GET(self): self.handle_one("GET")
    def do_POST(self): self.handle_one("POST")
    def do_PATCH(self): self.handle_one("PATCH")

srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
srv.daemon_threads = True
print(f"opencode server listening on http://127.0.0.1:{srv.server_address[1]}", flush=True)
# Hover closes stdin; the server stops when Hover kills it.
srv.serve_forever()
