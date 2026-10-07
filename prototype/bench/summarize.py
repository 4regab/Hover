#!/usr/bin/env python3
"""Per app: for each marker segment, the root process's USS (hover-measure 'private', MiB, median over samples), PSS, RSS and CPU%, then median [min-max] over runs.
usage: summarize.py <runs-prefix>...   (e.g. /projects/sandbox/bench/runs-rust)"""
import csv, os, sys, json, statistics as st, subprocess, glob
def seg(d):
    rows = list(csv.DictReader(open(f'{d}/markers.csv')))
    marks = [(int(r['unix_ms']), r['text']) for r in rows if r['kind'] == 'mark']
    sends = [int(r['unix_ms']) for r in rows if r['kind'] == 'send' and r['text'].split()[0] in ('unfold', 'fold', 'toggle')]
    end = [int(r['unix_ms']) for r in rows if r['kind'] == 'end'][0]
    out = {}
    for i, (t, n) in enumerate(marks):
        t2 = marks[i + 1][0] if i + 1 < len(marks) else end
        ins = [x for x in sends if t < x < t2]
        if ins: t2 = ins[0]
        if n != '-': out[n] = (t + (2000 if t2 - t > 8000 else 0), t2)
    return out
def mem(d, a, b):
    u, p, r = [], [], []
    for x in csv.DictReader(open(f'{d}/samples.csv')):
        if x['depth'] == '0' and a <= int(x['unix_ms']) <= b:
            u.append(int(x['private']) / 2**20); p.append(int(x['pss']) / 2**20); r.append(int(x['resident']) / 2**20)
    return (st.median(u), st.median(p), st.median(r)) if u else None
def cell(v): return f"{st.median(v):.1f} [{min(v):.1f}–{max(v):.1f}]"
for pre in sys.argv[1:]:
    runs = sorted(glob.glob(pre + '-[0-9]'))
    acc = {}
    for d in runs:
        cpu = json.loads(subprocess.check_output([sys.executable, os.path.join(os.path.dirname(__file__), 'cpu_report.py'), d]))
        for n, (a, b) in seg(d).items():
            m = mem(d, a, b)
            if m: acc.setdefault(n, {'uss': [], 'pss': [], 'rss': [], 'cpu': []}); [acc[n][k].append(v) for k, v in zip(('uss', 'pss', 'rss'), m)]; acc[n]['cpu'].append(cpu.get(n, float('nan')))
    print(f"\n### {os.path.basename(pre)} ({len(runs)} runs)\n| segment | USS MiB | PSS MiB | RSS MiB | CPU % of one core |\n|---|---|---|---|---|")
    for n, v in acc.items(): print(f"| {n} | {cell(v['uss'])} | {cell(v['pss'])} | {cell(v['rss'])} | {cell(v['cpu'])} |")
    t = {}
    for d in runs:
        for r in csv.DictReader(open(f'{d}/markers.csv')):
            if r['kind'] == 'time':
                k, v = r['text'].split(); t.setdefault(k, []).append(float(v))
    print("| timing ms | " + " | ".join(f"{k}: {cell(v)}" for k, v in t.items()) + " |")
    fr = []
    for d in runs:
        for r in csv.DictReader(open(f'{d}/markers.csv')):
            if r['kind'] == 'got' and r['text'].startswith('bench frames'): fr.append(r['text'].split()[2:6])
    print("frames (count p50 p95 p99 ms) per run:", fr)
