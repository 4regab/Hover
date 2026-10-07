#!/usr/bin/env python3
"""Sample the CPU ticks (utime+stime, whole process, all threads) of the ROOT app process only — agents/tools excluded — every 250 ms.
usage: cpu_sampler.py <comm> <out.csv>      (comm = process name as in /proc/*/comm, e.g. hoverai or HoverAvalonia)"""
import os, sys, time
comm, out = sys.argv[1], sys.argv[2]
def find():
    for d in os.listdir('/proc'):
        if d.isdigit():
            try:
                if open(f'/proc/{d}/comm').read().strip() == comm: return int(d)
            except Exception: pass
    return None
with open(out, 'w') as f:
    f.write('unix_ms,pid,ticks\n'); pid = None
    while True:
        if pid is None or not os.path.exists(f'/proc/{pid}'): pid = find()
        if pid:
            try:
                st = open(f'/proc/{pid}/stat').read().rsplit(')', 1)[1].split()
                f.write(f'{int(time.time()*1000)},{pid},{int(st[11])+int(st[12])}\n'); f.flush()
            except Exception: pass
        time.sleep(0.25)
