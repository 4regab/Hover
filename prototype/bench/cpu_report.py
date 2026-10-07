#!/usr/bin/env python3
"""CPU% (100 = one core) of the app process per marker segment: cpu_report.py <run-dir> (needs cpu.csv + markers.csv)"""
import csv, sys, os, json
d = sys.argv[1]
cpu = [(int(r['unix_ms']), int(r['ticks'])) for r in csv.DictReader(open(os.path.join(d, 'cpu.csv')))]
marks = [(int(r['unix_ms']), r['text']) for r in csv.DictReader(open(os.path.join(d, 'markers.csv'))) if r['kind'] == 'mark']
rows = list(csv.DictReader(open(os.path.join(d, 'markers.csv'))))
sends = [int(r['unix_ms']) for r in rows if r['kind'] == 'send' and r['text'].split()[0] in ('unfold', 'fold', 'toggle')]
end = [int(r['unix_ms']) for r in csv.DictReader(open(os.path.join(d, 'markers.csv'))) if r['kind'] == 'end'][0]
res = {}
for i, (t, name) in enumerate(marks):
    t2 = marks[i + 1][0] if i + 1 < len(marks) else end
    if name == '-' : continue
    inside = [x for x in sends if t < x < t2]
    if inside: t2 = inside[0]   # a segment ends where the office is opened or folded
    # skip the first 2 s of a segment (settling after a transition) when it is long enough
    a = t + (2000 if t2 - t > 8000 else 0)
    pts = [(x, y) for x, y in cpu if a <= x <= t2]
    if len(pts) < 2: continue
    res[name] = round((pts[-1][1] - pts[0][1]) / 100.0 / ((pts[-1][0] - pts[0][0]) / 1000.0) * 100, 1)
print(json.dumps(res))
