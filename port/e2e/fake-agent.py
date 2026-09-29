#!/usr/bin/env python3
"""A scenario-driven stand-in ACP agent for Hover's end-to-end run. Installed as
kiro-cli, codex-acp, cursor-agent (and codex) via symlinks; the name picks the tool.
Every message received is logged to $E2E_LOG as `<tool> <json>` lines.
The prompt picks the scenario by keyword: edit, ask, slow, fail, crash, md, mermaid."""
import json, os, sys, threading, time

tool = os.path.basename(sys.argv[0])
args = sys.argv[1:]
LOG = os.environ["E2E_LOG"]

def log(tag, obj):
    with open(LOG, "a") as f:
        f.write(f"{tool} {tag} {json.dumps(obj)}\n")

log("argv", args)
if tool == "kiro-cli" and args[:1] == ["whoami"]:
    print("Logged in as e2e@example.com"); sys.exit(0)
if tool == "kiro-cli" and args[:1] == ["chat"]:
    # kiro-cli chat --no-interactive /usage
    print("┃  | KIRO FREE ┃\n┃ Monthly credits: ┃\n┃ ████████ 42% (resets on 10/01) ┃\n┃ (21.00 of 50 covered in plan) ┃")
    sys.exit(0)
if tool == "codex" and args[:2] == ["login", "status"]:
    print("Logged in using ChatGPT"); sys.exit(0)
if tool == "cursor-agent" and args[:1] == ["status"]:
    print("Logged in as e2e@example.com"); sys.exit(0)

gate = threading.Lock()
cancelled = set()
waiting = {}  # request id -> Event, answer
n = 0
nreq = [1000]
sessions = {}

def send(m):
    log("out", m)
    with gate:
        sys.stdout.write(json.dumps(m) + "\n")
        sys.stdout.flush()

def update(sid, u):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": u}})

def say(sid, text):
    for i in range(0, len(text), 40):
        update(sid, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text[i:i + 40]}})
        time.sleep(0.01)

def is_cancelled(sid):
    with gate:
        if sid in cancelled:
            cancelled.discard(sid); return True
    return False

def config():
    opts = [{"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "fast",
             "options": [{"value": "fast", "name": "Fast"}, {"value": "smart", "name": "Smart"}]}]
    if tool == "kiro-cli":
        opts.append({"id": "autopilot", "name": "Autopilot", "type": "select", "currentValue": "on",
                     "options": [{"value": "on", "name": "On"}, {"value": "off", "name": "Off"}]})
        opts.append({"id": "mode", "name": "Agent", "category": "mode", "type": "select", "currentValue": "vibe",
                     "options": [{"value": "vibe", "name": "Vibe"}, {"value": "spec", "name": "Spec"}]})
    elif tool == "codex-acp":
        opts.append({"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "agent",
                     "options": [{"value": v, "name": v} for v in ["read-only", "agent", "agent-full-access"]]})
    else:
        opts.append({"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": "agent",
                     "options": [{"value": v, "name": v} for v in ["agent", "plan", "ask"]]})
    opts.append({"id": "effort", "name": "Effort", "category": "thought_level", "type": "select", "currentValue": "medium",
                 "options": [{"value": v, "name": v.title()} for v in ["low", "medium", "high"]]})
    return opts

def ask(sid, call):
    nreq[0] += 1
    rid = nreq[0]
    ev = threading.Event()
    waiting[rid] = [ev, None]
    send({"jsonrpc": "2.0", "id": rid, "method": "session/request_permission", "params": {"sessionId": sid, "toolCall": call,
          "options": [{"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                      {"optionId": "allow-always", "name": "Always", "kind": "allow_always"},
                      {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}]}})
    while not ev.wait(0.1):
        if sid in cancelled:
            break
    r = waiting.pop(rid)[1] or {}
    o = r.get("outcome", {})
    return o.get("optionId") if o.get("outcome") == "selected" else "cancelled"

def done(id, reason="end_turn"):
    send({"jsonrpc": "2.0", "id": id, "result": {"stopReason": reason}})

def prompt(id, sid, text):
    folder = sessions.get(sid, "/tmp")
    low = text.lower()
    update(sid, {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Thinking about it."}})
    update(sid, {"sessionUpdate": "usage_update", "used": 42000, "size": 200000})
    time.sleep(0.3)
    if "crash" in low:
        sys.stderr.write("fatal: the stand-in crashed on purpose\n"); sys.stderr.flush()
        os._exit(7)
    if "fail" in low:
        send({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "Internal error: the model is overloaded"}}); return
    if "read" in low:
        update(sid, {"sessionUpdate": "tool_call", "toolCallId": "r1", "kind": "read", "title": "Read README.md", "status": "in_progress",
                     "locations": [{"path": f"{folder}/README.md"}]})
        time.sleep(0.2)
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "r1", "status": "completed"})
    if "edit" in low:
        call = {"toolCallId": "e1", "kind": "edit", "title": "Edit src/app.ts", "status": "pending",
                "locations": [{"path": f"{folder}/src/app.ts"}],
                "content": [{"type": "diff", "path": f"{folder}/src/app.ts", "oldText": "export function tidy(files) {\n  return files;\n}\n",
                             "newText": "export function tidy(files) {\n  return files\n    .map(sortImports)\n    .filter(Boolean);\n}\n"}]}
        update(sid, dict(call, sessionUpdate="tool_call"))
        if "ask" in low:
            got = ask(sid, call)
            log("answer", {"edit": got})
            if got in ("reject-once", "cancelled"):
                update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "e1", "status": "failed"})
                if is_cancelled(sid): done(id, "cancelled"); return
                say(sid, "The edit was refused, so nothing changed."); done(id); return
        time.sleep(0.3)
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "e1", "status": "completed", "content": call["content"]})
    if "run" in low or "ask" in low:
        call = {"toolCallId": "x1", "kind": "execute", "title": "Run npm install three@0.171.0", "status": "pending",
                "rawInput": {"command": "npm install three@0.171.0"}}
        update(sid, dict(call, sessionUpdate="tool_call"))
        if "ask" in low:
            got = ask(sid, call)
            log("answer", {"run": got})
            if got in ("reject-once", "cancelled"):
                update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "x1", "status": "failed"})
                if is_cancelled(sid): done(id, "cancelled"); return
                say(sid, f"You said no to the install ({got})."); done(id); return
        time.sleep(0.3)
        update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": "x1", "status": "completed",
                     "rawOutput": {"output": "added 1 package in 2s\n\n1 package is looking for funding", "exitCode": 0}})
    if "slow" in low:
        for i in range(200):
            if is_cancelled(sid): done(id, "cancelled"); return
            if i % 10 == 0:
                update(sid, {"sessionUpdate": "tool_call", "toolCallId": f"s{i}", "kind": "search", "title": f"Search step {i // 10}", "status": "in_progress"})
            if i % 10 == 5:
                update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": f"s{i - 5}", "status": "completed"})
            time.sleep(0.1)
    if "md" in low:
        say(sid, "## Summary\n\nHere is **bold**, `code`, and a [link](https://example.com).\n\n- one\n- two\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n")
    if "mermaid" in low:
        say(sid, "```mermaid\nflowchart TD\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Ship]\n  B -->|no| D[Fix]\n```\n")
    say(sid, f"Done: {text.strip()[:60]}")
    done(id)

def serve():
    global n
    for line in sys.stdin:
        try:
            m = json.loads(line)
        except ValueError:
            continue
        log("in", m)
        id, method, p = m.get("id"), m.get("method"), m.get("params") or {}
        sid = p.get("sessionId")
        if method is None and id in waiting:
            waiting[id][1] = m.get("result")
            waiting[id][0].set()
        elif method == "initialize":
            send({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}})
        elif method == "session/new":
            n += 1
            s = f"{tool}-{os.getpid()}-{n}"
            sessions[s] = p.get("cwd")
            send({"jsonrpc": "2.0", "id": id, "result": {"sessionId": s, "configOptions": config()}})
        elif method == "session/load":
            sessions[sid] = p.get("cwd")
            update(sid, {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "(replayed)"}})
            update(sid, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "(replayed answer)"}})
            send({"jsonrpc": "2.0", "id": id, "result": {"configOptions": config()}})
        elif method == "session/set_config_option":
            send({"jsonrpc": "2.0", "id": id, "result": {}})
        elif method == "session/prompt":
            text = " ".join(b.get("text", "") for b in p.get("prompt", []) if b.get("type") == "text")
            threading.Thread(target=prompt, args=(id, sid or "", text), daemon=True).start()
        elif method == "session/cancel":
            with gate:
                cancelled.add(sid or "")
        elif id is not None:
            send({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}})

if tool in ("kiro-cli",) and args[:1] != ["acp"]:
    sys.exit(0)
serve()
