"""Writes wheels-<triple>.txt: the exact CPython 3.12 wheels Phonon's runtime installs, one
line each (name version size sha256 url). A build-time tool, never run on a user's machine.

    python make-lock.py        (needs uv on PATH and the network)

uv resolves the set (pylock.toml, PEP 751). Every wheel but torch is then taken from PyPI's
own JSON, so its URL, size and sha256 are PyPI's. Linux torch is the CPU build from
download.pytorch.org (PyPI's pulls in CUDA); its sha256 is that index's, its size a HEAD.
"""
import json
import pathlib
import subprocess
import sys
import tempfile
import tomllib
import urllib.request

HERE = pathlib.Path(__file__).parent
TORCH = "2.14.1"
WANTS = ["fermion-research==0.2.5", "safetensors", "soundfile", "scipy", "zstandard"]
TARGETS = {
    "x86_64-pc-windows-msvc": f"torch=={TORCH}",
    "x86_64-unknown-linux-gnu": f"torch=={TORCH}+cpu",
    "aarch64-unknown-linux-gnu": f"torch=={TORCH}+cpu",
}


def pypi(name, version, filename):
    with urllib.request.urlopen(f"https://pypi.org/pypi/{name}/{version}/json") as r:
        for u in json.load(r)["urls"]:
            if u["filename"] == filename:
                return u["size"], u["digests"]["sha256"], u["url"]
    sys.exit(f"{filename} is not on PyPI")


def size_of(url):
    # Cloudflare there refuses urllib's own User-Agent.
    with urllib.request.urlopen(urllib.request.Request(url, method="HEAD", headers={"User-Agent": "curl/8"})) as r:
        return int(r.headers["Content-Length"])


for triple, torch in TARGETS.items():
    tmp = pathlib.Path(tempfile.mkdtemp())
    (tmp / "req.in").write_text("\n".join(WANTS + [torch]) + "\n")
    lock = tmp / "pylock.toml"
    extra = [] if "windows" in triple else ["--extra-index-url", "https://download.pytorch.org/whl/cpu", "--index-strategy", "unsafe-best-match"]
    subprocess.run(["uv", "pip", "compile", str(tmp / "req.in"), "--python-version", "3.12", "--python-platform", triple,
                    "--no-header", "-o", str(lock), *extra], check=True)
    lines = [f"# {triple}, CPython 3.12. Made by make-lock.py; don't edit by hand.", "# name version size sha256 url"]
    for p in tomllib.loads(lock.read_text())["packages"]:
        wheels = p.get("wheels") or sys.exit(f"{p['name']} has no wheel")
        # soundfile ships an any-platform wheel next to the one that bundles libsndfile.
        w = ([w for w in wheels if not w["url"].endswith("-any.whl")] or wheels)[0]
        filename = w["url"].rsplit("/", 1)[1].replace("%2B", "+")
        if p["name"] == "torch" and "pytorch.org" in w["url"]:
            row = (size_of(w["url"]), w["hashes"]["sha256"], w["url"])
        else:
            row = pypi(p["name"], p["version"], filename)
        lines.append(f"{p['name']} {p['version']} {row[0]} {row[1]} {row[2]}")
    (HERE / f"wheels-{triple}.txt").write_text("\n".join(lines) + "\n", newline="\n")
    print(triple, len(lines) - 2, "wheels")
