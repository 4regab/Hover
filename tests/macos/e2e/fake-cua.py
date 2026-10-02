#!/usr/bin/python3
"""A stand-in for Cua's `cua` CLI in Hover's E2E run: Spaces in a JSON file, a viewer
URL on the local test site, an MCP server with computer tools, and teleport. It logs
every call to cua.log. No VM, container or network is ever made."""
import json, os, sys, time
root = os.environ['HOVER_E2E_ROOT']; state = root + '/spaces.json'; log = open(root + '/cua.log', 'a')
print(' '.join(sys.argv[1:]), file=log, flush=True)
def load(): return json.load(open(state)) if os.path.exists(state) else []
def save(x): json.dump(x, open(state, 'w'))
a = sys.argv[1:]
if a[:1] == ['--version']: print('cua 0.3.0'); sys.exit(0)
if a[:2] == ['spaces', 'ls']: print(json.dumps(load())); sys.exit(0)
if a[:2] == ['spaces', 'create']:
    name = a[a.index('--name') + 1]
    for ph, f in [('pulling', 0.3), ('creating', 0.7), ('booting', 0.9), ('ready', 1.0)]:
        print(json.dumps({'phase': ph, 'fraction': f}), flush=True); time.sleep(0.4)
    s = [x for x in load() if x['name'] != name] + [{'id': 'local:' + name, 'name': name, 'os': 'macos', 'power_state': 'running', 'phase': 'ready'}]
    save(s); sys.exit(0)
if a[:2] in (['spaces', 'start'], ['spaces', 'stop']):
    s = load()
    for x in s:
        if x['id'] == a[2]: x['power_state'] = 'running' if a[1] == 'start' else 'stopped'
    save(s); print(json.dumps({'space': a[2], 'state': 'running' if a[1] == 'start' else 'stopped'})); sys.exit(0)
if a[:2] == ['spaces', 'delete']: save([x for x in load() if x['id'] != a[2]]); sys.exit(0)
if a[:2] == ['runtime', 'setup']: sys.exit(0)
if a[:2] == ['sb', 'view']: print('Viewer for %s: %s/viewer/#ticket=e2e-ticket&files=%%2Fhome' % (a[2], os.environ['HOVER_E2E_SITE'])); sys.exit(0)
if a[:2] == ['teleport', 'push']:
    print('1 3', flush=True); time.sleep(0.3); print('3 3', flush=True); sys.exit(0)
if a[:1] == ['mcp']:
    space = a[a.index('--sandbox') + 1] if '--sandbox' in a else ''
    tools = [{'name': n, 'description': n, 'inputSchema': {'type': 'object'}} for n in ['computer_screenshot', 'computer_click', 'computer_type', 'send_file']]
    for line in sys.stdin:
        m = json.loads(line)
        if 'id' not in m: continue
        meth = m.get('method')
        if meth == 'initialize': r = {'protocolVersion': '2025-06-18', 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'cua', 'version': '0.3.0'}}
        elif meth == 'tools/list': r = {'tools': tools}
        elif meth == 'tools/call':
            p = m['params']; print('call', space, p['name'], json.dumps(p.get('arguments', {})), file=log, flush=True)
            r = {'content': [{'type': 'text', 'text': '%s done in %s' % (p['name'], space)}], 'isError': False}
        else: r = {}
        sys.stdout.write(json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'result': r}) + '\n'); sys.stdout.flush()
    sys.exit(0)
print('fake cua: unknown ' + ' '.join(a), file=sys.stderr); sys.exit(2)
