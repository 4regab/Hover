"""Three running at most, Esc in the task box, the HUD menu, the history panel, delete with the confirm."""
from drive import *
h = Hover(os.path.join(WORK, "dE"))
try:
    h.cmd("toggle"); time.sleep(1.5)
    for i in range(3): new_task(h, "Kiro", f"slow task {i}")
    time.sleep(1.5)
    h.tap("fabMain"); time.sleep(0.5); h.tap(name="Kiro", role="button"); time.sleep(0.5)
    h.tap("nInput"); h.type("a fourth")
    go = h.find("nGo")
    notes = [nm for n, i, nm, r in h.nodes() if "running" in nm]
    check("a 4th task can't start while 3 run", notes, str(notes))
    h.tap("nGo"); time.sleep(1)
    check("still 3 prompts", len(prompts(h)) == 3, str(len(prompts(h))))
    h.shot("sE-limit"); crop("sE-limit")
    h.key("Escape"); time.sleep(0.5)
    check("Esc in the task box folds only the box", h.find("fabMain", secs=1) is not None and h.find("menuBtn", secs=0.5) is not None)
    # HUD menu: time, beats, history
    h.tap("menuBtn"); time.sleep(0.5)
    h.tap("timeDay"); time.sleep(1.0); h.shot("sE-day"); crop("sE-day")
    h.tap("timeNight"); time.sleep(1.0)
    print("menu open before beats:", h.find("hudMenu", secs=0.3) is not None)
    h.tap("beats"); time.sleep(0.8)
    print("office.json:", open("" + WORK + "/dE/office.json").read(), "menu still open:", h.find("hudMenu", secs=0.3) is not None)
    b = h.find("beats"); print("beats state:", b.get_state_set().contains(Atspi.StateType.CHECKED))
    h.tap("histBtn"); time.sleep(1.0)
    h.shot("sE-history"); crop("sE-history")
    print("history:", [l for l in h.dump() if "|  |" not in l][:30])
    h.key("Escape"); time.sleep(0.5)
    # board via clicking the wall board (scene pick): use TV/board from AT-SPI? not exposed; skip
    # stop all, then delete one with the confirm
    h.cmd("stopall"); time.sleep(3)
    t = [n for n, i, nm, r in h.nodes() if r == "button" and nm in ("Pip","Nova","Juno","Moss","Ada","Rex")]
    n_before = len(t)
    h.click_node(t[0]); time.sleep(1)
    h.tap("dDel"); time.sleep(0.6)
    h.shot("sE-confirm"); crop("sE-confirm")
    print("confirm:", [l for l in h.dump() if "button" in l])
    yes = h.find(name="Delete", role="button", secs=2)
    if yes: h.click_node(yes); time.sleep(1)
    check("confirm closes", h.find("cfYes", secs=0.3) is None)
    check("drawer closes", h.find("drawer", secs=0.3) is None)
    ok = wait(lambda: len([nm for n, i, nm, r in h.nodes() if r == "button" and nm in ("Pip","Nova","Juno","Moss","Ada","Rex")]) == n_before - 1, 15)
    check("delete removes the session (the bot walks out)", ok)
    h.tap("menuBtn"); time.sleep(0.4); h.tap("histBtn"); time.sleep(1.0)
    h.shot("sE-history2"); crop("sE-history2")
    print([nm for n, i, nm, r in h.nodes() if "kept until" in nm])
    st = json.load(open("" + WORK + "/dE/settings.json", encoding="utf-8-sig")) if os.path.exists("" + WORK + "/dE/settings.json") else None
    print("settings.json:", st)
    print(os.listdir(os.path.join(WORK, "dE")), os.listdir("" + WORK + "/dE/agents") if os.path.exists("" + WORK + "/dE/agents") else None)
finally:
    print("quit code", h.quit()); h.close()
summary()
