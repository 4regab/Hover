"""A task with a read, an edit, a command, Markdown and Mermaid; the chat opens from the bot's name."""
from drive import *
h = Hover(os.path.join(WORK, "dA"))
try:
    h.cmd("toggle"); time.sleep(1.5)
    new_task(h, "Kiro", "Read then edit and run, md mermaid")
    check("prompt sent", wait(lambda: prompts(h, "kiro-cli"), 15))
    check("done tag", wait(lambda: "Done! ✓" in labels(h), 20))
    h.tap(name="Pip", role="button"); time.sleep(1.2)
    check("drawer opens on tag click", h.find("drawer", secs=2) is not None)
    h.shot("sA-drawer"); crop("sA-drawer")
    # scroll the thread up to see the steps
    h.move(1300, 200)
    for _ in range(6): h.click(1300, 200, button=4); time.sleep(0.1)
    time.sleep(0.6); h.shot("sA-drawer-up"); crop("sA-drawer-up")
    print("\n".join(l for l in h.dump() if "drawer" in l or "|  |" not in l)[:3000])
finally:
    print("quit code", h.quit()); h.close()
summary()
