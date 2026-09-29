#!/usr/bin/env python3
"""A stand-in agent for the Linux benchmark, speaking what port/tools/FakeAcp speaks
(the same updates at the same rate), so measuring needs no .NET. Installed as
`kiro-cli` on the benchmark's PATH: `whoami` says signed in, `acp` serves ACP over
stdio. FAKEACP_SECONDS (default 30) and FAKEACP_RATE (updates a second, default 20)."""
import json, os, sys, threading, time

if len(sys.argv) > 1 and sys.argv[1] == "whoami":
    print("Logged in as bench@example.com")
    sys.exit(0)

seconds = float(os.environ.get("FAKEACP_SECONDS", "30"))
rate = float(os.environ.get("FAKEACP_RATE", "20"))
answer = "Done. The bench task is finished.\n\nNothing was changed."
gate = threading.Lock()
cancelled = set()
n = 0

def send(m):
    with gate:
        sys.stdout.write(json.dumps(m) + "\n")
        sys.stdout.flush()

def update(sid, u):
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": sid, "update": u}})

def prompt(id, sid):
    kinds = ["read", "search", "edit", "execute"]
    total = int(seconds * rate)
    for i in range(total):
        with gate:
            if sid in cancelled:
                cancelled.discard(sid)
                sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "cancelled"}}) + "\n")
                sys.stdout.flush()
                return
        if i % 10 == 0:
            update(sid, {"sessionUpdate": "tool_call", "toolCallId": f"t{i}", "kind": kinds[i // 10 % 4], "title": f"Step {i // 10}", "status": "in_progress",
                         "locations": [{"path": f"src/file{i // 10}.rs"}]})
        elif i % 10 == 5:
            update(sid, {"sessionUpdate": "tool_call_update", "toolCallId": f"t{i - 5}", "status": "completed"})
        elif i % 10 == 7:
            update(sid, {"sessionUpdate": "usage_update", "used": 1000 + i * 40, "size": 200000})
        else:
            update(sid, {"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "thinking "}})
        time.sleep(1 / rate)
    update(sid, {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": answer}})
    send({"jsonrpc": "2.0", "id": id, "result": {"stopReason": "end_turn"}})

for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    id, method, p = m.get("id"), m.get("method"), m.get("params") or {}
    sid = p.get("sessionId")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": True}}})
    elif method == "session/new":
        n += 1
        send({"jsonrpc": "2.0", "id": id, "result": {"sessionId": f"fake-{os.getpid()}-{n}", "configOptions": []}})
    elif method in ("session/load", "session/set_config_option"):
        send({"jsonrpc": "2.0", "id": id, "result": {}})
    elif method == "session/prompt":
        threading.Thread(target=prompt, args=(id, sid or ""), daemon=True).start()
    elif method == "session/cancel":
        with gate:
            cancelled.add(sid or "")
    elif id is not None:
        send({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}})
