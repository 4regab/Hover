"""Codex and Cursor: what each tool access sets (mode), Read only on Cursor."""
from drive import *
h = Hover(os.path.join(WORK, "dD"))
def cfg(tool):
    return [(j["params"]["sessionId"], j["params"]["configId"], j["params"]["value"]) for _, j in sent(h, "session/set_config_option", tool)]
try:
    h.cmd("toggle"); time.sleep(1.5)
    h.tap("fabMain"); time.sleep(0.5); h.tap(name="Codex", role="button"); time.sleep(0.5)
    h.tap("nAccess"); time.sleep(0.5)
    print("codex access options:", [nm for n, i, nm, r in h.nodes() if r in ("menu item", "radio menu item", "button", "check menu item") ][:14])
    h.shot("sD-access"); crop("sD-access")
    h.key("Escape"); time.sleep(0.4)
    new_task(h, "Codex", "codex full run")
    new_task(h, "Codex", "codex ask first run", access="Ask first")
    new_task(h, "Codex", "codex ask always", access="Ask always")
    time.sleep(3)
    print("codex cfg", cfg("codex-acp"))
    c = cfg("codex-acp")
    modes = [v for s, k, v in c if k == "mode"]
    check("codex Full -> agent-full-access, Ask first -> read-only (no workspace-write offered), Ask always -> read-only", modes == ["agent-full-access", "read-only", "read-only"], str(modes))
    time.sleep(3)
    # Cursor: read only -> ask mode
    new_task(h, "Cursor", "cursor read only", access="Read only")
    time.sleep(3)
    print("cursor cfg", cfg("cursor-agent"))
    check("cursor Read only -> mode ask", ("mode", "ask") in [(k, v) for s, k, v in cfg("cursor-agent")])
    print("argv", [(t, j) for t, tag, j in h.agent_log() if tag == "argv"])
    time.sleep(4)
    h.shot("sD-office"); crop("sD-office")
    print(labels(h))
finally:
    print("quit code", h.quit()); h.close()
summary()
