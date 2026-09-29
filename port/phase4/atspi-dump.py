#!/usr/bin/env python3
"""The AT-SPI tree of a running Hover (Linux's UI Automation), for SCREENS.md's ids.

    dbus-run-session -- python3 port/phase4/atspi-dump.py OUT.txt

Starts the AT-SPI bus, then hover (HOVER_BENCH=1, on the DISPLAY given) opens the
office through its bench channel, and the tree is walked: role, name and accessible
id of every node. The ids are Slint's `accessible-id`s, which AccessKit gives AT-SPI
as the accessible id, as UIA gets them as AutomationId.
"""
import os, subprocess, sys, time
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

out = sys.argv[1]
reg = subprocess.Popen(["/usr/libexec/at-spi-bus-launcher", "--launch-immediately"], stderr=subprocess.DEVNULL)
time.sleep(1.5)
env = dict(os.environ, HOVER_BENCH="1")
env.pop("WAYLAND_DISPLAY", None)
h = subprocess.Popen([sys.argv[2] if len(sys.argv) > 2 else "native/target/release/hover"], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)


def wait(prefix, secs=30):
    end = time.time() + secs
    while time.time() < end:
        line = h.stdout.readline()
        if line.startswith(prefix):
            return line
    raise TimeoutError(prefix)


def walk(node, depth, lines):
    try:
        ident = node.get_accessible_id() or ""
    except Exception:
        ident = ""
    lines.append(f"{'  ' * depth}{node.get_role_name()} | {node.get_name() or ''} | {ident}")
    for i in range(node.get_child_count()):
        c = node.get_child_at_index(i)
        if c is not None and depth < 40:
            walk(c, depth + 1, lines)


try:
    wait("bench visible", 60)
    h.stdin.write("toggle\n"); h.stdin.flush()
    wait("bench toggled")
    time.sleep(4)
    desk = Atspi.get_desktop(0)
    lines = []
    for i in range(desk.get_child_count()):
        app = desk.get_child_at_index(i)
        if app is not None:
            walk(app, 0, lines)
    open(out, "w").write("\n".join(lines) + "\n")
    print(len(lines), "nodes;", sum(1 for l in lines if not l.endswith("| ")), "with an id")
finally:
    try:
        h.stdin.write("quit\n"); h.stdin.flush(); h.wait(10)
    except Exception:
        h.kill()
    reg.kill()
