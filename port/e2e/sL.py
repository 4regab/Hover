"""Copy on a code block (the X clipboard) and a link opening in the browser."""
from drive import *
from Xlib import Xatom
D = os.path.join(WORK, "dL")
h = Hover(D, env={"BROWSER": os.path.join(WORK, "bin", "browser")})
def cropd(n): Image.open(f"{OUT}/{n}.png").crop((1100,0,1560,480)).save(f"{OUT}/c-{n}.png")
def clipboard():
    d = h.d; w = d.screen().root.create_window(0, 0, 1, 1, 0, d.screen().root_depth)
    sel = d.intern_atom("CLIPBOARD"); tgt = d.intern_atom("UTF8_STRING"); prop = d.intern_atom("E2E_CLIP")
    w.convert_selection(sel, tgt, prop, X.CurrentTime); d.flush()
    end = time.time() + 3
    while time.time() < end:
        if d.pending_events():
            e = d.next_event()
            if e.type == X.SelectionNotify:
                if e.property == X.NONE: return None
                r = w.get_full_property(prop, X.AnyPropertyType)
                return r.value.decode() if r and isinstance(r.value, bytes) else (bytes(r.value).decode() if r else None)
        else: time.sleep(0.05)
    return "timeout"
try:
    h.cmd("toggle"); time.sleep(1.5)
    new_task(h, "Kiro", "md please")
    wait(lambda: "Done! ✓" in labels(h), 20)
    h.tap(name="Pip", role="button"); time.sleep(1.2)
    h.move(1300, 200)
    for _ in range(6): h.click(1300, 200, button=4); time.sleep(0.1)
    time.sleep(0.6); h.shot("sL-0"); cropd("sL-0")
    im = Image.open(OUT + "/sL-0.png")
    # find the "Copy" chip: a light text on a small pill right of the code header; scan rows for the 'rust' header band
    print("clipboard before:", clipboard())
    h.click(1467, 285); time.sleep(0.5)
    h.shot("sL-1"); cropd("sL-1")
    c = clipboard(); print("clipboard after:", repr(c))
    check("Copy puts the code on the clipboard", c and "println!" in c, repr(c))
    h.click(1290, 201); time.sleep(1.5)
    b = open(os.path.join(WORK, "browser.log")).read() if os.path.exists(os.path.join(WORK, "browser.log")) else ""
    check("a link opens in the browser", "https://example.com" in b, b)
finally:
    print("quit code", h.quit()); h.close()
summary()
