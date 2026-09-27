"""Writes Hover's icon: assets/logo.svg and src/Hover/Assets/hover.ico.

    python assets/make-icon.py        (needs Pillow: pip install pillow)

The mark is a round H: a white disc on a black tile, cut by a slot down from the
top and one up from the bottom. The upper slot is the notch the workspace drops
from. Each .ico frame is drawn at its own size with the tile edges, slots and
crossbar on whole pixels, so 16 and 24 px stay sharp instead of being blurred
copies of the large icon.
"""
import io
import math
import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
# 16/20/24/32 are the tray at 100-200 % scaling; 256 is Explorer and the installer.
SIZES = (16, 20, 24, 32, 40, 48, 64, 256)


def half_up(v):
    return math.floor(v + 0.5)


def geometry(size):
    """Tile inset, tile corner, disc radius and slot half-width, in pixels. At 256 px
    they are 16, 52, 80 and 16; smaller sizes round the tile and slots to pixels."""
    inset = max(1, half_up(size / 16))
    half = size / 2 - inset
    return inset, half * 52 / 112, half * 80 / 112, max(1, half_up(half / 7))


def frame(size):
    ss = 4096 // size                     # draw large, then average down
    inset, corner, r, w = geometry(size)
    c = size / 2
    n = size * ss

    def p(v):
        return round(v * ss)

    tile = Image.new("L", (n, n), 0)
    ImageDraw.Draw(tile).rounded_rectangle(
        (p(inset), p(inset), p(size - inset) - 1, p(size - inset) - 1), radius=p(corner), fill=255)

    disc = Image.new("L", (n, n), 0)
    d = ImageDraw.Draw(disc)
    d.ellipse((p(c - r), p(c - r), p(c + r) - 1, p(c + r) - 1), fill=255)
    # Two slots with round ends; the crossbar left between them is 2w tall.
    for end, edge in ((c - 2 * w, 0), (c + 2 * w, n)):
        d.rectangle((p(c - w), min(p(end), edge), p(c + w) - 1, max(p(end), edge)), fill=0)
        d.ellipse((p(c - w), p(end - w), p(c + w) - 1, p(end + w) - 1), fill=0)

    alpha, white = tile.reduce(ss), disc.reduce(ss)
    # Straight alpha, as .ico wants: grey is the white share of what is covered.
    grey = Image.new("L", alpha.size)
    grey.putdata([0 if a == 0 else min(255, round(255 * v / a))
                  for a, v in zip(alpha.getdata(), white.getdata())])
    return Image.merge("RGBA", (grey, grey, grey, alpha))


def dib(im):
    """A 32-bit bitmap plus its 1-bit AND mask, bottom row first: how .ico stores
    frames under 256 px, and what every Windows icon reader accepts."""
    w, h = im.size
    flipped = im.transpose(Image.Transpose.FLIP_TOP_BOTTOM)
    pixels = flipped.tobytes("raw", "BGRA")
    stride = (w + 31) // 32 * 4
    mask = bytearray(stride * h)
    for i, a in enumerate(flipped.getchannel("A").getdata()):
        if a == 0:
            y, x = divmod(i, w)
            mask[y * stride + x // 8] |= 0x80 >> (x % 8)
    header = struct.pack("<IiiHHIIiiII", 40, w, 2 * h, 1, 32, 0, len(pixels) + len(mask), 0, 0, 0, 0)
    return header + pixels + bytes(mask)


def ico(frames):
    blobs = []
    for im in frames:
        if im.width < 256:
            blobs.append(dib(im))
        else:
            buf = io.BytesIO()
            im.save(buf, "PNG", optimize=True)
            blobs.append(buf.getvalue())
    head = struct.pack("<HHH", 0, 1, len(frames))
    offset = 6 + 16 * len(frames)
    for im, blob in zip(frames, blobs):
        head += struct.pack("<BBBBHHII", im.width % 256, im.height % 256, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    return head + b"".join(blobs)


def svg():
    c, r, w = 128, 80, 16
    top = c - math.sqrt(r * r - w * w)    # where the slots leave the rim
    t, b = f"{top:.3f}", f"{2 * c - top:.3f}"
    mark = (f"M{c - w},{t} V{c - 2 * w} A{w},{w} 0 0 0 {c + w},{c - 2 * w} V{t} "
            f"A{r},{r} 0 0 1 {c + w},{b} V{c + 2 * w} A{w},{w} 0 0 0 {c - w},{c + 2 * w} V{b} "
            f"A{r},{r} 0 0 1 {c - w},{t} Z")
    return f"""<svg width="256" height="256" viewBox="0 0 256 256" xmlns="http://www.w3.org/2000/svg" role="img" aria-label="Hover">
  <!-- Written by make-icon.py: edit that, not this. A round H — a white disc on a
       black tile, cut by a slot from the top (the notch) and one from the bottom. -->
  <rect x="16" y="16" width="224" height="224" rx="52" fill="#000"/>
  <path d="{mark}" fill="#fff"/>
</svg>
"""


if __name__ == "__main__":
    (ROOT / "assets" / "logo.svg").write_text(svg(), encoding="utf-8")
    ico_path = ROOT / "src" / "Hover" / "Assets" / "hover.ico"
    ico_path.write_bytes(ico([frame(s) for s in SIZES]))

    # Read it back: every size is there, and at 16 px the slot, crossbar and stem
    # land on the pixels they should.
    back = Image.open(ico_path)
    assert back.info["sizes"] == {(s, s) for s in SIZES}, back.info["sizes"]
    small = back.ico.getimage((16, 16)).convert("RGBA")
    assert small.getpixel((7, 4)) == (0, 0, 0, 255), "top slot"
    assert small.getpixel((7, 7)) == (255, 255, 255, 255), "crossbar"
    assert small.getpixel((4, 8)) == (255, 255, 255, 255), "left stem"
    assert small.getpixel((0, 0))[3] == 0, "outside the tile"
    print(f"wrote {ico_path.relative_to(ROOT)} ({ico_path.stat().st_size} bytes) and assets/logo.svg")
