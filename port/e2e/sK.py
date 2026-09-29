"""The Worked line opens the steps, Esc in Settings and after Back, the office dropped after 30 s and restored."""
from drive import *
D = os.path.join(WORK, "dK")
h = Hover(D)
def cropd(n): Image.open(f"{OUT}/{n}.png").crop((1100,0,1560,480)).save(f"{OUT}/c-{n}.png")
def clip():
    r = subprocess.run(["python3", "-c", "import gi;gi.require_version('Gtk','3.0')"], capture_output=True)
    return None
try:
    h.cmd("toggle"); time.sleep(1.5)
    new_task(h, "Kiro", "read then edit and run")
    wait(lambda: "Done! ✓" in labels(h), 20)
    h.tap(name="Pip", role="button"); time.sleep(1.2)
    h.shot("sK-0"); cropd("sK-0")
    im = Image.open(OUT + "/sK-0.png")
    # find the "Worked" line: scan the drawer for it by clicking at its row (y from the shot)
    th = [n for n, i, nm, r in h.nodes() if r == "image" and h.rect(n) and h.rect(n)[0] == 1108]
    r = h.rect(th[0]) if th else None; print("thread rect", r)
    # the Worked line sits under the first bubble; click at x=1160 across y positions until the thread grows
    before = Image.open(OUT + "/sK-0.png").crop((1108, 78, 1508, 367)).tobytes()
    h.click(1180, 78 + 75); time.sleep(0.8)
    h.shot("sK-1"); cropd("sK-1")
    after = Image.open(OUT + "/sK-1.png").crop((1108, 78, 1508, 367)).tobytes()
    check("clicking the Worked line changes the thread (steps open)", before != after)
    # Settings: open, Esc folds the notch
    h.tap("dClose"); time.sleep(0.6)
    h.tap("menuBtn"); time.sleep(0.4); h.tap("setBtn"); time.sleep(1.0)
    h.tap("SectionKiro"); time.sleep(0.8)
    h.key("Escape"); time.sleep(1.0)
    print(h.cmd("state"))
    st = h.cmd("state"); check("Esc in Settings folds the notch", "Rest" in st, st)
    h.cmd("toggle"); time.sleep(1.2)
    b = h.find("BackToOffice", secs=1)
    if b: h.click_node(b); time.sleep(0.8)
    check("Back returns to the office", h.find("menuBtn", secs=1) is not None)
    h.key("Escape"); time.sleep(1.0)
    st = h.cmd("state"); check("Esc after Back folds the notch", "Rest" in st, st)
    # office dropped after 30 s hidden, then back with the open chat
    h.cmd("toggle"); time.sleep(1.2)
    h.tap(name="Pip", role="button"); time.sleep(1.0)
    h.key("Escape"); time.sleep(0.5)  # closes the drawer
    h.tap(name="Pip", role="button"); time.sleep(1.0)
    h.cmd("toggle"); time.sleep(33)
    log = open(OUT + "/hover.log").read()
    check("office dropped after 30 s hidden", "office dropped" in log)
    h.cmd("toggle"); time.sleep(2.0)
    check("office back with the chat open", h.find("drawer", secs=3) is not None)
    h.shot("sK-back"); crop("sK-back")
finally:
    print("quit code", h.quit()); h.close()
summary()
