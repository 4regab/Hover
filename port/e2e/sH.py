"""Settings by mouse: switches, launch at login, office size, the shortcut recorder and the new shortcut."""
from drive import *
D = os.path.join(WORK, "dH")
h = Hover(D)
def st(): return json.load(open(D + "/settings.json", encoding="utf-8-sig"))
def ids(): return [(i, nm, r) for n, i, nm, r in h.nodes() if i]
try:
    h.cmd("toggle"); time.sleep(1.5)
    h.tap("menuBtn"); time.sleep(0.4); h.tap("setBtn"); time.sleep(1.2)
    # a switch, first click
    h.tap("HoverOpens"); time.sleep(1.0)
    check("one click turns Open on hover off", wait(lambda: st().get("HoverOpensWorkspace") is False, 3), str(st().get("HoverOpensWorkspace")))
    h.tap("HoverOpens"); time.sleep(1.0)
    check("and on again", wait(lambda: st().get("HoverOpensWorkspace") is True, 3))
    h.tap("LaunchAtLogin"); time.sleep(1.0)
    auto = os.path.join(h.env["HOME"], ".config/autostart")
    check("launch at login writes an autostart entry", wait(lambda: os.path.isdir(auto) and os.listdir(auto), 3), str(os.listdir(auto) if os.path.isdir(auto) else None))
    if os.path.isdir(auto) and os.listdir(auto): print(open(os.path.join(auto, os.listdir(auto)[0])).read())
    h.tap("LaunchAtLogin"); time.sleep(1.0)
    check("and off removes it", not (os.path.isdir(auto) and os.listdir(auto)))
    # size
    h.tap("WorkspaceSizeLarge"); time.sleep(1.5)
    check("office size Large saved", wait(lambda: st().get("WorkspaceSize") == "Large", 3), st().get("WorkspaceSize"))
    print("frame after Large:", [l for l in h.dump() if "HoverNotch" in l])
    h.tap("WorkspaceSizeDefault"); time.sleep(1.5)
    # shortcut record: click, press Ctrl+Alt+J
    h.tap("WorkspaceShortcut"); time.sleep(0.5)
    print("recording:", h.find("WorkspaceShortcut").get_name(), [nm for n,i,nm,r in h.nodes() if "Press" in nm or "press" in nm][:3])
    h.key("j", ("Control_L", "Alt_L")); time.sleep(1.0)
    check("shortcut rebinds to Ctrl+Alt+J", wait(lambda: st().get("ScWorkspace") == {"Key": "J", "Modifiers": "Alt, Control"}, 3), str(st().get("ScWorkspace")))
    # the new shortcut opens/closes
    h.key("Escape"); time.sleep(1.0)
    h.focus_helper(); time.sleep(0.3)
    h.key("j", ("Control_L", "Alt_L")); time.sleep(1.2)
    check("Ctrl+Alt+J opens the notch", h.find("menuBtn", secs=2) is not None or h.find("BackToOffice", secs=0.5) is not None)
    h.key("j", ("Control_L", "Alt_L")); time.sleep(1.0)
    # sections
    h.cmd("toggle"); time.sleep(1.2)
    if h.find("BackToOffice", secs=0.5) is None:
        h.tap("menuBtn"); time.sleep(0.4); h.tap("setBtn"); time.sleep(1.0)
    for sec in ["SectionIntegrations", "SectionKiro", "SectionCodex", "SectionCursor", "SectionGeneral"]:
        h.tap(sec); time.sleep(0.8); h.shot("sH-" + sec); crop("sH-" + sec)
        print(sec, [i for i, nm, r in ids() if i not in ("HoverNotch","BackToOffice","Close") and not i.startswith("Section")][:40])
finally:
    print("quit code", h.quit()); h.close()
summary()
