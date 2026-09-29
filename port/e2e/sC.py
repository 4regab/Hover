"""Stop, a queued reply dropped by Stop, an ACP error, and a crashed tool coming back."""
from drive import *
h = Hover(os.path.join(WORK, "dC"))
def tag_names(): return [nm for n, i, nm, r in h.nodes() if r == "button" and nm in ("Pip","Nova","Juno","Moss","Ada","Rex","Kit","Bo")]
def open_newest():
    t = [n for n, i, nm, r in h.nodes() if r == "button" and nm in ("Pip","Nova","Juno","Moss","Ada","Rex")]
    h.click_node(t[-1]); time.sleep(1.0)
try:
    h.cmd("toggle"); time.sleep(1.5)
    # queue + stop
    new_task(h, "Kiro", "slow one")
    wait(lambda: prompts(h), 10); time.sleep(1.5)
    open_newest()
    check("drawer open", h.find("drawer", secs=2) is not None)
    s = h.find("send"); print("send button name while busy:", s.get_name())
    h.tap("input"); h.type("then this"); h.key("Return"); time.sleep(0.8)
    h.shot("sC-queued"); crop("sC-queued")
    check("reply queued, not sent", len(prompts(h)) == 1, str(len(prompts(h))))
    s = h.find("send"); print("send button name with draft/queue:", s.get_name())
    h.tap("send"); time.sleep(1.0)
    print("after first click:", h.find("send").get_name())
    check("stop sends session/cancel", wait(lambda: sent(h, "session/cancel"), 5))
    time.sleep(2.0)
    check("stop drops the queued reply", len(prompts(h)) == 1, str(len(prompts(h))))
    h.shot("sC-stopped"); crop("sC-stopped")
    print(labels(h)[:30])
    h.tap("dClose"); time.sleep(0.8)
    # fail
    new_task(h, "Kiro", "please fail")
    time.sleep(6); open_newest()
    h.shot("sC-failed"); crop("sC-failed")
    # crash
    h.tap("dClose"); time.sleep(0.5)
    new_task(h, "Kiro", "crash now")
    time.sleep(6); open_newest()
    h.shot("sC-crash"); crop("sC-crash")
    print(open(OUT + "/hover.log").read()[-1500:])
    # after the crash the tool comes back for the next task
    h.tap("input"); h.type("quick after crash"); h.key("Return"); time.sleep(4)
    check("tool restarted after crash", len([1 for t, tag, j in h.agent_log() if tag == "argv" and j[:1] == ["acp"]]) >= 2)
    h.shot("sC-after-crash"); crop("sC-after-crash")
finally:
    print("quit code", h.quit()); h.close()
summary()
