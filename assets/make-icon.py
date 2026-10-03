"""Writes Hover's brand pictures from the logo in assets/hover.svg: assets/hover.png,
app/assets/hover.ico, app/assets/hover-mark.png and assets/social-preview.png.

    python assets/make-icon.py        (needs: pip install pillow cairosvg)

Each .ico frame is scaled down from the logo at its own size, so Windows shows the
tray's 16-32 px frames as they are instead of shrinking the large one into a blur.
hover-mark.png is the logo alone on a clear background, cropped tight, for the
workspace header and the Linux icon. social-preview.png is the 1280x640 picture for
GitHub's social preview and link cards (upload it under Settings > Social preview).
To change the logo, edit hover.svg and run this; don't edit the outputs.
"""
import io
import struct
from pathlib import Path

import cairosvg
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
BG = (22, 22, 22)        # #161616, the square behind the logo
BLUE = (26, 115, 255)    # #1a73ff, the logo's blue
# 16/20/24/32 are the tray at 100-200 % scaling; 256 is Explorer and the installer.
SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
FONTS = ROOT / "app" / "assets"


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


def logo():
    """hover.svg at 1024 px, on a clear background."""
    png = cairosvg.svg2png(url=str(ROOT / "assets" / "hover.svg"), output_width=1024, output_height=1024)
    return Image.open(io.BytesIO(png)).convert("RGBA")


def on_square(clear):
    """The logo on its dark square, as hover.png and the .ico frames."""
    sq = Image.new("RGBA", clear.size, BG + (255,))
    sq.alpha_composite(clear)
    return sq


def mark(clear):
    """The logo alone, cropped tight to a square with a little air, 256 px (22 px in
    the header; the Linux icon is this file too)."""
    l, t, r, b = clear.getchannel("A").getbbox()
    side, pad = max(r - l, b - t), 0.04
    side = round(side * (1 + 2 * pad))
    cx, cy = (l + r) // 2, (t + b) // 2
    box = (cx - side // 2, cy - side // 2, cx - side // 2 + side, cy - side // 2 + side)
    return clear.crop(box).resize((256, 256), Image.Resampling.LANCZOS)


def social(clear):
    """1280x640: the logo and name on the left, what it does under them, and a
    notch at the top edge, which is where Hover lives. Kept 60 px inside the edges,
    where GitHub and link cards crop."""
    W, H = 1280, 640
    # Near-black, with a blue glow behind the logo and a faint one in the far corner.
    glow = Image.new("RGB", (W, H), (0, 0, 0))
    g = ImageDraw.Draw(glow)
    g.ellipse((-80, 100, 560, 740), fill=(14, 46, 108))
    g.ellipse((860, -260, 1460, 240), fill=(10, 28, 64))
    im = ImageChops.add(Image.new("RGB", (W, H), (11, 11, 13)), glow.filter(ImageFilter.GaussianBlur(120)))
    d = ImageDraw.Draw(im, "RGBA")

    # The notch: a black pill hanging from the top edge, with the logo's blue dot.
    nx, nw, nh = (W - 360) // 2, 360, 48
    d.rounded_rectangle((nx, -30, nx + nw, nh), radius=24, fill=(0, 0, 0), outline=(255, 255, 255, 26), width=1)
    d.ellipse((nx + 24, 16, nx + 36, 28), fill=BLUE)
    small = ImageFont.truetype(str(FONTS / "Inter-Medium.ttf"), 15)
    d.text((nx + 50, 22), "Kiro is working", font=small, fill=(235, 235, 240), anchor="lm")
    d.text((nx + nw - 24, 22), "0:42", font=small, fill=(140, 140, 150), anchor="rm")

    # Logo, name, tagline.
    size = 290
    big = mark(clear).resize((size, size), Image.Resampling.LANCZOS)
    im.paste(big, (90, 186), big)
    x = 90 + size + 64
    name = ImageFont.truetype(str(FONTS / "InterDisplay-SemiBold.ttf"), 132)
    tag = ImageFont.truetype(str(FONTS / "Inter-Regular.ttf"), 34)
    tools = ImageFont.truetype(str(FONTS / "Inter-Medium.ttf"), 24)
    d.text((x, 262), "Hover", font=name, fill=(255, 255, 255), anchor="ls")
    d.text((x, 330), "AI coding agents at work in a notch", font=tag, fill=(190, 192, 200), anchor="ls")
    d.text((x, 376), "at the top of your screen.", font=tag, fill=(190, 192, 200), anchor="ls")
    # The tools it drives, as quiet pills.
    px = x
    for t in ("Kiro", "Codex", "Cursor", "OpenCode", "Claude Code"):
        w = d.textlength(t, font=tools) + 36
        d.rounded_rectangle((px, 420, px + w, 466), radius=23, fill=(255, 255, 255, 14), outline=(255, 255, 255, 36), width=1)
        d.text((px + w / 2, 443), t, font=tools, fill=(220, 222, 230), anchor="mm")
        px += w + 12
    d.text((x, 540), "Windows  ·  Linux  ·  MIT", font=small, fill=(120, 122, 132), anchor="ls")
    return im


if __name__ == "__main__":
    clear = logo()
    square = on_square(clear)
    square.convert("RGB").save(ROOT / "assets" / "hover.png", optimize=True)
    ico_path = ROOT / "app" / "assets" / "hover.ico"
    ico_path.write_bytes(ico([square.resize((s, s), Image.Resampling.LANCZOS) for s in SIZES]))
    mark_path = ROOT / "app" / "assets" / "hover-mark.png"
    mark(clear).save(mark_path, optimize=True)
    social_path = ROOT / "assets" / "social-preview.png"
    social(clear).save(social_path, optimize=True)

    # Read it back: every size is there, and the large frame is still the logo
    # (blue body in the middle, dark background in the corner).
    back = Image.open(ico_path)
    assert back.info["sizes"] == {(s, s) for s in SIZES}, back.info["sizes"]
    big = back.ico.getimage((256, 256)).convert("RGB")
    r, g, b = big.getpixel((128, 160))
    assert b > 200 and b > r + 100, f"body {r, g, b}"
    assert max(big.getpixel((2, 2))) < 60, "background"
    # The mark: clear in the corner, blue body near the bottom centre.
    m = Image.open(mark_path)
    assert m.getpixel((2, 2))[3] == 0, "background left in"
    assert m.getpixel((128, 170))[3] == 255, "body not solid"
    assert Image.open(social_path).size == (1280, 640)
    for p in (ico_path, mark_path, social_path, ROOT / "assets" / "hover.png"):
        print(f"wrote {p.relative_to(ROOT)} ({p.stat().st_size} bytes)")
