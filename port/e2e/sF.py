"""The history after a restart: the transcript, and a reply waking it with session/load."""
from drive import *
D = os.path.join(WORK, "dF")
h = Hover(D)
try:
    h.cmd("toggle"); time.sleep(1.5)
    new_task(h, "Kiro", "remember me md")
    check("first run done", wait(lambda: "Done! ✓" in labels(h), 20))
    time.sleep(1)
    h.quit()
    print("files:", sorted(os.listdir(D + "/agents")))
    # restart on the same data
    open(LOG, "a").write("kiro-cli mark {\"restart\": true}\n")
    h.start(); time.sleep(0.5)
    h.cmd("toggle"); time.sleep(1.5)
    h.tap("menuBtn"); time.sleep(0.4); h.tap("histBtn"); time.sleep(1.0)
    h.shot("sF-history"); crop("sF-history")
    row = h.find(name="remember me md", contains=True, secs=3)
    check("history lists the old session after a restart", row is not None, str([nm for n,i,nm,r in h.nodes() if nm][:20]))
    if row:
        h.click_node(row); time.sleep(1.5)
        h.shot("sF-transcript"); crop("sF-transcript")
        check("the transcript opens", h.find("drawer", secs=2) is not None)
        h.tap("input"); h.type("and one more"); h.key("Return")
        ok = wait(lambda: sent(h, "session/load"), 10)
        check("a reply wakes it with session/load", ok)
        if ok: print("load:", ok[-1][1]["params"])
        check("the reply is prompted after load", wait(lambda: any("one more" in json.dumps(j) for _, j in prompts(h)), 10))
        time.sleep(4)
        h.shot("sF-woken"); crop("sF-woken")
        print([nm for n,i,nm,r in h.nodes() if r == "label"][:30])
finally:
    print("quit code", h.quit()); h.close()
summary()
