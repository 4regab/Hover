#!/usr/bin/python3
"""A stand-in for Lume in Hover's E2E run: the VMs' status and size from vms.json."""
import json, os, sys
root = os.environ['HOVER_E2E_ROOT']; vms = root + '/vms.json'; log = open(root + '/cua.log', 'a')
print('lume ' + ' '.join(sys.argv[1:]), file=log, flush=True)
v = json.load(open(vms)) if os.path.exists(vms) else {}
a = sys.argv[1:]
if a[:1] == ['get']:
    if a[1] not in v: print('Error: VM not found', file=sys.stderr); sys.exit(1)
    print(json.dumps(v[a[1]])); sys.exit(0)
if a[:1] == ['set']:
    x = v.setdefault(a[1], {'name': a[1]})
    if '--cpu' in a: x['cpuCount'] = int(a[a.index('--cpu') + 1])
    if '--memory' in a: x['memorySize'] = int(a[a.index('--memory') + 1].rstrip('GB')) << 30
    if '--display' in a: x['display'] = a[a.index('--display') + 1]
    json.dump(v, open(vms, 'w')); sys.exit(0)
sys.exit(2)
