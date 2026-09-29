"""OpenCode (55111fc) and the note before the first task: the note, OpenCode in the picker,
a task over its server (Basic auth, the folder named), the model pill's menu with the
model's own variants, a question answered in the chat and one answered from the notch's
Review, an approval, Stop, and its Settings page."""
from drive import *
h = Hover(os.path.join(WORK, "dM"), notice=True)
def oc(): return [j for t, tag, j in h.agent_log() if t == "opencode" and tag == "in"]
def oc_prompts(): return [j for j in oc() if j["path"].endswith("/prompt_async")]
def oc_replies(kind): return [j for j in oc() if j["path"].startswith("/" + kind + "/")]
def bubble(text, secs=20): return wait(lambda: any(text in l for l in labels(h)), secs)
try:
    h.cmd("toggle"); time.sleep(1.5)
    # The note, in place of the office, until Got it.
    check("the note shows before the first task", h.find("KiroNotice", secs=5) is not None)
    h.shot("sM-notice"); crop("sM-notice")
    h.tap("KiroNoticeOk"); time.sleep(0.8)
    check("Got it opens the office", h.find("KiroNotice", secs=1) is None and h.find("fabMain", secs=3) is not None)
    st = json.load(open(os.path.join(WORK, "dM", "settings.json"), encoding="utf-8-sig"))
    check("Got it is kept", st.get("KiroNoticeSeen") is True, str(st.get("KiroNoticeSeen")))
    # OpenCode in the picker, and a task on its server.
    h.tap("fabMain"); time.sleep(0.6)
    check("OpenCode is the fifth choice", h.find(name="OpenCode", role="button", secs=3) is not None)
    h.shot("sM-pick"); crop("sM-pick")
    h.tap("fabMain"); time.sleep(0.5)
    new_task(h, "OpenCode", "edit a file please")
    check("prompt sent to opencode serve", wait(lambda: oc_prompts(), 30), str(oc()[-3:]))
    check("done", bubble("Done! ✓", 20))
    check("every call signed in and names the folder", all(j["auth"] for j in oc()) and all(j["directory"] for j in oc() if j["path"] not in ("/global/health",)),
          str([j for j in oc() if not j["auth"]][:2]))
    # The pill's menu: the models OpenCode offered, and the picked one's own variants.
    h.tap("fabMain"); time.sleep(0.5)
    h.tap(name="OpenCode", role="button"); time.sleep(0.6)
    h.tap("nModel"); time.sleep(0.6)
    h.shot("sM-menu"); crop("sM-menu")
    check("the menu lists OpenCode's models", h.find(name="A B · Prov", secs=3) is not None, str([l for l in h.dump() if "list-item" in l][:8]))
    h.tap(name="A B · Prov"); time.sleep(0.6)
    h.tap("nModel"); time.sleep(0.6)
    check("the model's variants show as its effort", h.find(name="High", secs=3) is not None)
    h.tap(name="High"); time.sleep(0.4)
    h.shot("sM-variants"); crop("sM-variants")
    h.key("Escape"); time.sleep(0.4)
    st = json.load(open(os.path.join(WORK, "dM", "settings.json"), encoding="utf-8-sig"))
    check("the pick is the tool's default", (st.get("Agents") or {}).get("opencode", {}).get("Model") == "p/a" and st["Agents"]["opencode"].get("Effort") == "high", str(st.get("Agents")))
    new_task(h, "OpenCode", "answer my question")
    check("the next task sends the model and its variant", wait(lambda: len(oc_prompts()) >= 2, 20) and oc_prompts()[-1]["body"].get("model") == {"providerID": "p", "modelID": "a"}
          and oc_prompts()[-1]["body"].get("variant") == "high", str(oc_prompts()[-1:]))
    # A question over the head: Answer… opens its chat; a pick, then Answer.
    over = h.find(name="Answer…", role="button", secs=15)
    check("the question over the bot's head", over is not None)
    h.shot("sM-question-over"); crop("sM-question-over")
    if over: h.click_node(over); time.sleep(1.0)
    check("Answer… opens the chat", h.find("drawer", secs=3) is not None)
    tabs = h.find(name="Tabs", secs=3)
    if tabs: h.click_node(tabs); time.sleep(0.5)
    h.shot("sM-question-chat"); crop("sM-question-chat")
    h.tap(name="Answer", role="button"); time.sleep(0.8)
    check("the answer is the picked label", wait(lambda: any(j["path"].endswith("/reply") and j["body"] == {"answers": [["Tabs"]]} for j in oc_replies("question")), 8), str(oc_replies("question")))
    check("the agent carries on with it", bubble("Done! ✓", 15))
    h.tap("dClose"); time.sleep(0.6)
    # From the notch: Skip and Review on the island; Review opens the chat, and a reply
    # in one's own words answers it.
    new_task(h, "OpenCode", "another question")
    wait(lambda: h.find(name="Answer…", role="button", secs=0.2), 15)
    h.key("Escape"); time.sleep(1.2); h.focus_helper()
    h.shot("sM-island"); Image.open(OUT + "/sM-island.png").crop((660, 0, 1260, 80)).save(OUT + "/c-sM-island.png")
    check("the island offers Skip for a question", h.find(name="Skip", secs=3) is not None)
    rv = h.find(name="Review", secs=3)
    if rv: h.click_node(rv); time.sleep(1.5)
    check("Review opens the office at its chat", h.find("drawer", secs=4) is not None, h.cmd("state"))
    h.tap("input"); h.type("Spaces please\n"); time.sleep(0.8)
    check("a reply is the answer in one's own words", wait(lambda: any(j["body"] == {"answers": [["Spaces please"]]} for j in oc_replies("question")), 8), str(oc_replies("question")[-1:]))
    h.tap("dClose"); time.sleep(0.6)
    # An approval under Ask first: Run over the head is OpenCode's "once".
    new_task(h, "OpenCode", "ask to install", access="Ask first")
    run = h.find(name="Run", role="button", secs=15)
    check("the approval over the head", run is not None)
    if run: h.click_node(run); time.sleep(0.8)
    check("Run answers once", wait(lambda: any(j["body"] == {"reply": "once"} for j in oc_replies("permission")), 8), str(oc_replies("permission")))
    check("the run ends", bubble("Done! ✓", 15))
    # Stop: the server is asked to abort, and the run ends stopped.
    new_task(h, "OpenCode", "slow work")
    wait(lambda: len(oc_prompts()) >= 5, 15)
    tags = [n for n, i, nm, r in h.nodes() if r == "button" and nm in ("Pip", "Nova", "Juno", "Moss", "Ada", "Rue")]
    h.click_node(tags[-1]); time.sleep(1.0)
    h.tap("send"); time.sleep(1.5)
    check("Stop aborts that session", wait(lambda: any(j["path"].endswith("/abort") for j in oc()), 8))
    check("and it ends stopped", wait(lambda: h.find(name="Stop this run", secs=0.2) is None, 10))
    h.tap("dClose"); time.sleep(0.6)
    # Its Settings page.
    h.tap("menuBtn"); time.sleep(0.5); h.tap("setBtn"); time.sleep(1.0)
    h.tap(name="OpenCode", secs=3); time.sleep(0.8)
    check("Settings has an OpenCode page with its agents", h.find("OpenCodeAgent", secs=3) is not None, str([l for l in h.dump() if "OpenCode" in l][:6]))
    h.shot("sM-settings"); crop("sM-settings")
finally:
    print("quit code", h.quit()); h.close()
summary()
