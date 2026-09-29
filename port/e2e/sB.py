"""Approvals: over the bot's head, in the notch's island and card (Enter allows, Esc denies), and a reply that denies."""
from drive import *
h = Hover(os.path.join(WORK, "dB"))
def answers(): return [j for t, tag, j in h.agent_log() if tag == "answer"]
def perms(): return [j for t, tag, j in h.agent_log() if tag == "in" and "result" in j and isinstance(j.get("result"), dict) and "outcome" in j["result"]]
try:
    h.cmd("toggle"); time.sleep(1.5)
    # 1: over the head, Run
    new_task(h, "Kiro", "ask to run it", access="Ask first")
    so = sent(h, "session/set_config_option", "kiro-cli"); print("config:", [j["params"].get("configId") + "=" + j["params"].get("value") for _, j in so])
    check("Ask first turns Kiro's autopilot off", any(j["params"]["configId"] == "autopilot" and j["params"]["value"] == "off" for _, j in so))
    btn = h.find(name="Run", role="button", secs=15)
    check("question over the head", btn is not None)
    h.shot("sB-over"); crop("sB-over")
    print([l for l in h.dump() if "button" in l])
    if btn: h.click_node(btn)
    check("Run answered allow_once", wait(lambda: any(a.get("run") == "allow-once" for a in answers()), 8), str(answers()))
    check("task ends done", wait(lambda: "Done! ✓" in labels(h), 10))
    # 2: in the notch: collapse, island asks, Review, Enter
    time.sleep(1)
    new_task(h, "Kiro", "ask again please", access=None)
    wait(lambda: h.find(name="Run", role="button", secs=0.2), 15)
    h.key("Escape"); time.sleep(1.2)
    h.focus_helper(); time.sleep(0.3)
    h.shot("sB-island"); Image.open(OUT + "/sB-island.png").crop((660, 0, 1260, 80)).save(OUT + "/c-sB-island.png")
    print([l for l in h.dump() if "button" in l or "label" in l][:20])
    rv = h.find(name="Review", secs=3)
    check("island shows Review", rv is not None)
    if rv: h.click_node(rv); time.sleep(1.0)
    h.shot("sB-card"); Image.open(OUT + "/sB-card.png").crop((560, 0, 1360, 300)).save(OUT + "/c-sB-card.png")
    print([l for l in h.dump() if "button" in l or "label" in l][:30])
    print("focus", h.focus())
    h.key("Return"); h.move(1800, 900); time.sleep(1.0)
    check("Enter on the card allows", wait(lambda: len([a for a in answers() if a.get("run") == "allow-once"]) >= 2, 6), str(answers()))
    check("focus back to the other app", wait(lambda: h.focus() == h.helper, 3), str(h.focus()))
    # 3: Esc on the card denies
    time.sleep(1.5)
    h.cmd("toggle"); time.sleep(1.2)
    new_task(h, "Kiro", "ask a third time")
    wait(lambda: h.find(name="Run", role="button", secs=0.2), 15)
    h.key("Escape"); time.sleep(1.2); h.focus_helper()
    rv = h.find(name="Review", secs=3)
    if rv: h.click_node(rv); time.sleep(1.0)
    h.key("Escape"); h.move(1800, 900); time.sleep(1.0)
    check("Esc on the card denies", wait(lambda: any(a.get("run") == "reject-once" for a in answers()), 6), str(answers()))
    # 4: reply in chat counts as deny
    time.sleep(1.5)
    print(h.cmd("state"))
    print(h.cmd("toggle")); print(h.cmd("state")); time.sleep(1.2)
    print(h.cmd("state"))
    h.shot("sB-p4"); print(h.dump()[:3])
    new_task(h, "Kiro", "ask a fourth time")
    wait(lambda: h.find(name="Run", role="button", secs=0.2), 15)
    tags = [n for n, i, nm, r in h.nodes() if r == "button" and nm in ("Pip","Nova","Juno","Moss","Ada","Rex")]
    print("tags", [t.get_name() for t in tags])
    h.shot("sB-4"); crop("sB-4")
    d = h.find("drawer", secs=1)
    if d is None:
        # open the asking one: the newest bot
        h.click_node(tags[-1]); time.sleep(1)
    h.shot("sB-chat"); crop("sB-chat")
    h.tap("input"); h.type("no, use yarn instead"); h.key("Return"); time.sleep(1.5)
    n_rej = len([a for a in answers() if a.get("run") == "reject-once"])
    check("a reply denies", n_rej >= 2, str(answers()))
    check("the reply is sent as the next prompt", wait(lambda: any("yarn" in json.dumps(j) for _, j in prompts(h)), 8))
    for a in answers(): print(a)
finally:
    print("quit code", h.quit()); h.close()
summary()
