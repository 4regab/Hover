"""Region metrics of BENCHMARK.md section 4 for the office scene: mean CIEDE2000 and the
share of pixels over dE 10, for the whole scene and per region (floor and walls, the
desks with the bots, the lit wall pictures). Prints one line per region.
    python3 port/phase2/compare.py page.png native.png [diff.png]"""
import sys
import numpy as np
from PIL import Image

def lab(rgb):
    c = rgb / 255.0
    c = np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)
    m = np.array([[0.4124564, 0.3575761, 0.1804375], [0.2126729, 0.7151522, 0.0721750], [0.0193339, 0.1191920, 0.9503041]])
    xyz = c @ m.T / np.array([0.95047, 1.0, 1.08883])
    f = np.where(xyz > 216 / 24389, np.cbrt(xyz), (24389 / 27 * xyz + 16) / 116)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)

def de2000(a, b):
    L1, a1, b1 = a[..., 0], a[..., 1], a[..., 2]
    L2, a2, b2 = b[..., 0], b[..., 1], b[..., 2]
    C1, C2 = np.hypot(a1, b1), np.hypot(a2, b2)
    Cm = (C1 + C2) / 2
    G = 0.5 * (1 - np.sqrt(Cm ** 7 / (Cm ** 7 + 25 ** 7)))
    a1p, a2p = a1 * (1 + G), a2 * (1 + G)
    C1p, C2p = np.hypot(a1p, b1), np.hypot(a2p, b2)
    h1p, h2p = np.degrees(np.arctan2(b1, a1p)) % 360, np.degrees(np.arctan2(b2, a2p)) % 360
    dL, dC = L2 - L1, C2p - C1p
    dh = np.where(C1p * C2p == 0, 0, np.where(np.abs(h2p - h1p) <= 180, h2p - h1p, np.where(h2p - h1p > 180, h2p - h1p - 360, h2p - h1p + 360)))
    dH = 2 * np.sqrt(C1p * C2p) * np.sin(np.radians(dh / 2))
    Lm, Cmp = (L1 + L2) / 2, (C1p + C2p) / 2
    hm = np.where(C1p * C2p == 0, h1p + h2p, np.where(np.abs(h1p - h2p) <= 180, (h1p + h2p) / 2, np.where(h1p + h2p < 360, (h1p + h2p + 360) / 2, (h1p + h2p - 360) / 2)))
    T = 1 - 0.17 * np.cos(np.radians(hm - 30)) + 0.24 * np.cos(np.radians(2 * hm)) + 0.32 * np.cos(np.radians(3 * hm + 6)) - 0.20 * np.cos(np.radians(4 * hm - 63))
    SL = 1 + 0.015 * (Lm - 50) ** 2 / np.sqrt(20 + (Lm - 50) ** 2)
    SC, SH = 1 + 0.045 * Cmp, 1 + 0.015 * Cmp * T
    RT = -2 * np.sqrt(Cmp ** 7 / (Cmp ** 7 + 25 ** 7)) * np.sin(np.radians(60 * np.exp(-((hm - 275) / 25) ** 2)))
    return np.sqrt((dL / SL) ** 2 + (dC / SC) ** 2 + (dH / SH) ** 2 + RT * (dC / SC) * (dH / SH))

a = np.asarray(Image.open(sys.argv[1]).convert('RGB'), dtype=float)
b = np.asarray(Image.open(sys.argv[2]).convert('RGB'), dtype=float)
d = de2000(lab(a), lab(b))
# Regions of the 1104 x 424 view (x0, y0, x1, y1).
REGIONS = {'scene': (0, 0, 1104, 424), 'floor and walls': (240, 0, 870, 424), 'desks and bots': (380, 140, 760, 360),
           'wall pictures': (300, 50, 460, 150), 'tv': (770, 140, 860, 240), 'window': (670, 80, 770, 200)}
for name, (x0, y0, x1, y1) in REGIONS.items():
    r = d[y0:y1, x0:x1]
    print(f'{name:16s} mean dE {r.mean():5.2f}   over 10: {100 * (r > 10).mean():5.2f} %')
if len(sys.argv) > 3:
    Image.fromarray(np.clip(d * 12, 0, 255).astype(np.uint8)).save(sys.argv[3])
