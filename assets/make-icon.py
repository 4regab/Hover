"""Writes Hover's icon, app/assets/hover.ico, and the header's hover-mark.png
from the logo in assets/hover.png.

    python assets/make-icon.py        (needs Pillow: pip install pillow)

Each .ico frame is scaled down from hover.png at its own size, so Windows shows
the tray's 16-32 px frames as they are instead of shrinking the large one into a
blur. hover-mark.png is the same logo without its dark square. To change the logo,
replace hover.png and run this; don't edit the outputs.
"""
import io
import struct
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
# 16/20/24/32 are the tray at 100-200 % scaling; 256 is Explorer and the installer.
SIZES = (16, 20, 24, 32, 40, 48, 64, 256)


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


def mark(logo):
    """The logo on a clear background, for the workspace header, where its dark
    square shows as a box on the panel. How far a pixel is from the background
    colour is how much of the H covers it. The H's own colour is then un-mixed from
    the background, so the edges carry no dark fringe on a light theme."""
    bg = logo.crop((0, 0, 64, 64)).resize((1, 1), Image.Resampling.BOX).getpixel((0, 0))
    raw, out = logo.tobytes(), bytearray()
    for i in range(0, len(raw), 3):
        p = raw[i:i + 3]
        # The background's grain stays within 6 levels; the H's darkest blue is
        # about 180 levels from it.
        a = min(1.0, max(0.0, (max(abs(v - b) for v, b in zip(p, bg)) - 6) / 170))
        if a == 0:
            out += b"\0\0\0\0"
        else:
            out += bytes(min(255, max(0, round((v - (1 - a) * b) / a))) for v, b in zip(p, bg))
            out.append(round(255 * a))
    clear = Image.frombytes("RGBA", logo.size, bytes(out))
    # 22 px in the header; 128 covers every display scaling.
    return clear.resize((128, 128), Image.Resampling.LANCZOS)


if __name__ == "__main__":
    logo = Image.open(ROOT / "assets" / "hover.png").convert("RGB")
    ico_path = ROOT / "app" / "assets" / "hover.ico"
    ico_path.write_bytes(ico([logo.convert("RGBA").resize((s, s), Image.Resampling.LANCZOS) for s in SIZES]))
    mark_path = ROOT / "app" / "assets" / "hover-mark.png"
    mark(logo).save(mark_path, optimize=True)

    # Read it back: every size is there, and the large frame is still the logo
    # (blue crossbar in the middle, dark background in the corner).
    back = Image.open(ico_path)
    assert back.info["sizes"] == {(s, s) for s in SIZES}, back.info["sizes"]
    big = back.ico.getimage((256, 256)).convert("RGB")
    r, g, b = big.getpixel((128, 128))
    assert b > 150 and b > r + 100, f"crossbar {r, g, b}"
    assert max(big.getpixel((2, 2))) < 60, "background"
    # The mark: clear around the H and between its stems, solid blue in the middle.
    clear = Image.open(mark_path)
    assert clear.getpixel((2, 2))[3] == 0 and clear.getpixel((64, 30))[3] == 0, "background left in"
    assert clear.getpixel((64, 64))[3] == 255, "crossbar not solid"
    print(f"wrote {ico_path.relative_to(ROOT)} ({ico_path.stat().st_size} bytes) and "
          f"{mark_path.relative_to(ROOT)} ({mark_path.stat().st_size} bytes) from assets/hover.png")
