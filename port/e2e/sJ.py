"""The app window: a task from it, maximize, Close hides it and Hover keeps running."""
from drive import *
D = os.path.join(WORK, "dJ")
h = Hover(D)
def frames(): return [(nm, h.rect(n)) for n, i, nm, r in h.nodes() if r == "frame"]
try:
    p2 = subprocess.run([HOVER], env=h.env, capture_output=True, text=True, timeout=20); time.sleep(2)
    print("frames:", frames())
    print([l for l in h.dump() if "button" in l or "frame" in l][:30])
    # a task from the app window
    new_task(h, "Kiro", "from the window md")
    check("task from the app window", wait(lambda: prompts(h), 10))
    check("done in the app window", wait(lambda: "Done! ✓" in labels(h), 20))
    h.shot("sJ-dash"); Image.open(OUT + "/sJ-dash.png").crop((0, 0, 1200, 620)).save(OUT + "/c-sJ-dash.png")
    # the notch meanwhile: toggle shows the office in the notch
    h.cmd("toggle"); time.sleep(1.5)
    h.shot("sJ-notch"); crop("sJ-notch")
    h.cmd("toggle"); time.sleep(1)
    mx = h.find(name="Maximize", secs=1)
    if mx: h.click_node(mx); time.sleep(1.2)
    print("after maximize:", [f for f in frames() if f[0] == "Hover"], h.find(name="Restore", secs=1) is not None)
    rs = h.find(name="Restore", secs=1)
    if rs: h.click_node(rs); time.sleep(1.2)
    print("after restore:", [f for f in frames() if f[0] == "Hover"], h.find(name="Maximize", secs=1) is not None)
    cl = [n for n, i, nm, r in h.nodes() if r == "button" and nm == "Close"]
    print("close buttons:", [h.rect(n) for n in cl])
    if cl: h.click_node(cl[0]); time.sleep(1.2)
    check("title bar Close hides the app window", not [f for f in frames() if f[0] == "Hover"], str(frames()))
    check("Hover keeps running", h.p.poll() is None)
finally:
    print("quit code", h.quit()); h.close()
summary()
