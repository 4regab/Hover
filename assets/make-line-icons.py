"""Writes src/Hover/Owl/Icons.cs: the workspace's line icons as WPF path data.

The icons are Lucide's (https://lucide.dev, ISC licence), fetched at a pinned
version and drawn on its 24 x 24 grid. Every SVG element (path, circle, rect,
line, polyline, polygon, ellipse) becomes one path string that WPF's
Geometry.Parse reads. Edit NAMES and run it again: python assets/make-line-icons.py
"""
import re
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

VERSION = "1.48.0"
URL = "https://raw.githubusercontent.com/lucide-icons/lucide/{v}/icons/{n}.svg"

# The name the app asks for -> the Lucide icon drawn for it.
NAMES = {
    "check": "check", "more": "ellipsis", "add": "plus", "calendar": "calendar",
    "stopwatch": "timer", "bell": "bell", "refresh": "refresh-cw", "delete": "trash",
    "copy": "copy", "forward": "arrow-right", "rename": "pencil", "return": "corner-down-left",
    "lines": "text-align-start", "close": "x",
    "chevron-down": "chevron-down", "chevron-up": "chevron-up",
    "chevron-left": "chevron-left", "chevron-right": "chevron-right",
    "clock": "clock", "bolt": "zap", "sliders": "sliders-horizontal", "ring": "circle",
    "done": "circle-check", "target": "target", "checklist": "list-checks",
    "compose": "notebook-pen", "settings": "settings", "folder": "folder",
    "warning": "triangle-alert", "photo": "image", "layout": "layout-grid", "gauge": "gauge",
    "notch": "panel-top", "view": "eye", "hide": "eye-off", "reset": "rotate-ccw",
    "cut": "scissors", "sparkles": "sparkles",
    # Command buttons, and the Buttons and Theme settings.
    "terminal": "square-terminal", "bot": "bot", "code": "code", "rocket": "rocket",
    "brain": "brain", "globe": "globe", "git": "git-branch", "database": "database",
    "cloud": "cloud", "cpu": "cpu", "bug": "bug", "flask": "flask-conical", "book": "book-open",
    "music": "music", "coffee": "coffee", "star": "star", "heart": "heart", "wrench": "wrench",
    "package": "package", "command": "command", "flame": "flame", "server": "server",
    "palette": "palette", "import": "file-down",
}

NUM = re.compile(r"[-+]?(?:\d*\.\d+|\d+\.?)(?:[eE][-+]?\d+)?")
ARGS = {"m": 2, "l": 2, "h": 1, "v": 1, "c": 6, "s": 4, "q": 4, "t": 2, "a": 7, "z": 0}


def fmt(x: float) -> str:
    s = f"{x:.3f}".rstrip("0").rstrip(".")
    return "0" if s in ("", "-0") else s


def path_d(d: str) -> str:
    """Re-emit SVG path data with every command spelled out and every number
    separated, which WPF's parser needs (it has no notion of arc flags run
    together, or of implicit repeats after a move).

    A path's opening "m" is absolute in SVG, but the elements of one icon are
    joined into a single WPF path, where it would be taken from the end of the
    previous element. So it is written as "M"; the pairs after it stay relative,
    as SVG says they are."""
    out, i, cmd, first = [], 0, None, True
    while i < len(d):
        ch = d[i]
        if ch in " ,\t\r\n":
            i += 1
            continue
        if ch.isalpha():
            cmd = ch
            i += 1
            if cmd in "zZ":
                out.append("Z")
                continue
        else:
            if cmd is None:
                raise ValueError(f"number before a command in {d!r}")
            # An implicit repeat: after a move, further pairs are lines.
            if cmd == "m":
                cmd = "l"
            elif cmd == "M":
                cmd = "L"
        n = ARGS[cmd.lower()]
        vals = []
        for k in range(n):
            while i < len(d) and d[i] in " ,\t\r\n":
                i += 1
            if cmd in "aA" and k in (3, 4):
                vals.append(d[i])
                i += 1
                continue
            m = NUM.match(d, i)
            if not m:
                raise ValueError(f"bad number at {i} in {d!r}")
            vals.append(fmt(float(m.group())))
            i = m.end()
        out.append(("M" if first and cmd == "m" else cmd) + " " + " ".join(vals))
        first = False
    return " ".join(out)


def f(el, k, default=0.0):
    return float(el.get(k, default))


def ellipse(cx, cy, rx, ry):
    return (f"M {fmt(cx - rx)} {fmt(cy)} A {fmt(rx)} {fmt(ry)} 0 1 0 {fmt(cx + rx)} {fmt(cy)} "
            f"A {fmt(rx)} {fmt(ry)} 0 1 0 {fmt(cx - rx)} {fmt(cy)} Z")


def element(el) -> str:
    tag = el.tag.split("}")[-1]
    if tag == "path":
        return path_d(el.get("d"))
    if tag == "circle":
        return ellipse(f(el, "cx"), f(el, "cy"), f(el, "r"), f(el, "r"))
    if tag == "ellipse":
        return ellipse(f(el, "cx"), f(el, "cy"), f(el, "rx"), f(el, "ry"))
    if tag == "line":
        return f"M {fmt(f(el, 'x1'))} {fmt(f(el, 'y1'))} L {fmt(f(el, 'x2'))} {fmt(f(el, 'y2'))}"
    if tag in ("polyline", "polygon"):
        pts = [fmt(float(v)) for v in NUM.findall(el.get("points"))]
        pairs = [f"{pts[j]} {pts[j + 1]}" for j in range(0, len(pts), 2)]
        s = "M " + pairs[0] + "".join(" L " + p for p in pairs[1:])
        return s + (" Z" if tag == "polygon" else "")
    if tag == "rect":
        x, y, w, h = f(el, "x"), f(el, "y"), f(el, "width"), f(el, "height")
        rx = f(el, "rx", el.get("ry", 0))
        ry = f(el, "ry", el.get("rx", 0))
        if rx == 0:
            return f"M {fmt(x)} {fmt(y)} H {fmt(x + w)} V {fmt(y + h)} H {fmt(x)} Z"
        return (f"M {fmt(x + rx)} {fmt(y)} H {fmt(x + w - rx)} A {fmt(rx)} {fmt(ry)} 0 0 1 {fmt(x + w)} {fmt(y + ry)} "
                f"V {fmt(y + h - ry)} A {fmt(rx)} {fmt(ry)} 0 0 1 {fmt(x + w - rx)} {fmt(y + h)} "
                f"H {fmt(x + rx)} A {fmt(rx)} {fmt(ry)} 0 0 1 {fmt(x)} {fmt(y + h - ry)} "
                f"V {fmt(y + ry)} A {fmt(rx)} {fmt(ry)} 0 0 1 {fmt(x + rx)} {fmt(y)} Z")
    raise ValueError(f"unhandled element {tag}")


def main():
    lines = []
    for key, name in NAMES.items():
        try:
            svg = urllib.request.urlopen(URL.format(v=VERSION, n=name)).read()
        except Exception as e:
            raise SystemExit(f"could not fetch Lucide icon '{name}' ({key}): {e}")
        root = ET.fromstring(svg)
        parts = [element(el) for el in root.iter() if el is not root]
        lines.append(f'        ["{key}"] = "{" ".join(parts)}",')

    cs = f"""// Generated by assets/make-line-icons.py from Lucide {VERSION}. Edit the script, not this file.
// Lucide is ISC licensed (some icons MIT, from Feather); the notice is in THIRD-PARTY-NOTICES.txt.

namespace Hover.Owl;

/// Line icons on a 24 x 24 grid, stroked at draw time. See Ui.Icon.
internal static class IconPaths
{{
    public static readonly IReadOnlyDictionary<string, string> Data = new Dictionary<string, string>
    {{
{chr(10).join(lines)}
    }};
}}
"""
    target = Path(__file__).parent.parent / "src" / "Hover" / "Owl" / "Icons.cs"
    target.write_text(cs, encoding="utf-8", newline="\n")
    print(f"wrote {len(lines)} icons to {target}")


if __name__ == "__main__":
    main()
