"""Writes Hover's icon, src/Hover/Assets/hover.ico, from the logo in assets/hover.png.

    python assets/make-icon.py        (needs Pillow: pip install pillow)

Each .ico frame is scaled down from hover.png at its own size, so Windows shows
the tray's 16-32 px frames as they are instead of shrinking the large one into a
blur. To change the logo, replace hover.png and run this; don't edit the .ico.
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


if __name__ == "__main__":
    logo = Image.open(ROOT / "assets" / "hover.png").convert("RGBA")
    ico_path = ROOT / "src" / "Hover" / "Assets" / "hover.ico"
    ico_path.write_bytes(ico([logo.resize((s, s), Image.Resampling.LANCZOS) for s in SIZES]))

    # Read it back: every size is there, and the large frame is still the logo
    # (blue crossbar in the middle, dark background in the corner).
    back = Image.open(ico_path)
    assert back.info["sizes"] == {(s, s) for s in SIZES}, back.info["sizes"]
    big = back.ico.getimage((256, 256)).convert("RGB")
    r, g, b = big.getpixel((128, 128))
    assert b > 150 and b > r + 100, f"crossbar {r, g, b}"
    assert max(big.getpixel((2, 2))) < 60, "background"
    print(f"wrote {ico_path.relative_to(ROOT)} ({ico_path.stat().st_size} bytes) from assets/hover.png")
