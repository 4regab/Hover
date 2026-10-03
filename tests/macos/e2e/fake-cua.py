#!/usr/bin/python3
"""A stand-in for Cua's `cua` CLI in Hover's E2E run: Spaces in a JSON file, a viewer
URL on the local test site, an MCP server with computer tools, and teleport. It logs
every call to cua.log. No VM, container or network is ever made."""
import json, os, sys, time
root = os.environ['HOVER_E2E_ROOT']; state = root + '/spaces.json'; log = open(root + '/cua.log', 'a')
print(' '.join(sys.argv[1:]), file=log, flush=True)
def load(): return json.load(open(state)) if os.path.exists(state) else []
def save(x): json.dump(x, open(state, 'w'))
# Lume's side: each VM's status and size, as `lume get` reports them.
vms = root + '/vms.json'
def vm(name, change):
    v = json.load(open(vms)) if os.path.exists(vms) else {}
    if change is None: v.pop(name, None)
    else: v.setdefault(name, {'name': name}).update(change)
    json.dump(v, open(vms, 'w'))
a = sys.argv[1:]
if a[:1] == ['--version']: print('cua 0.3.0'); sys.exit(0)
if a[:2] == ['spaces', 'ls']: print(json.dumps({'relay_error': None, 'spaces': load()})); sys.exit(0)
# This Mac's own sandboxes with their power state (Lume's), as `sb ls --local` lists them.
if a[:2] == ['sb', 'ls']:
    v = json.load(open(vms)) if os.path.exists(vms) else {}
    print(json.dumps([{'id': x['id'], 'name': x['id'].split(':')[-1], 'runtime': 'lume', 'kind': 'vm', 'status': v.get(x['id'].split(':')[-1], {}).get('status', 'stopped')} for x in load()])); sys.exit(0)
# Where the Space's own driver answers MCP: the stand-in driver, with the desktop's token.
if a[:2] == ['sb', 'mcp'] and a[3:5] == ['env', 'config']:
    print(json.dumps({'type': 'http', 'url': '%s/s/%s/mcp' % (os.environ['HOVER_E2E_DRIVER'], a[2].split(':')[-1]), 'headers': {'x-cua-env-authorization': 'Bearer ' + ('e2e-token' if '--show-secrets' in a else '****')}})); sys.exit(0)
if a[:2] == ['spaces', 'cancel']: sys.exit(0)
if a[:2] == ['spaces', 'create']:
    name = a[a.index('--name') + 1]
    assert '--cpus' in a and '--memory-mb' in a, a
    for ph, f in [('pulling', 0.3), ('creating', 0.7), ('booting', 0.9), ('ready', 1.0)]:
        print(json.dumps({'phase': ph, 'fraction': f}), flush=True); time.sleep(0.4)
    # As the real one: no power state in the list, and the guest's hostname as its name.
    s = [x for x in load() if x['id'] != 'local:' + name] + [{'id': 'local:' + name, 'name': 'Apple-Virtual-Machine-1.local', 'os': 'macos'}]
    # Cua's own record of the sandbox: where its cua-spacesd listens, and its token.
    json.dump({'name': name, 'host': '127.0.0.1', 'api_port': 3211, 'env_token': 'e2e-token'}, open(os.environ['CUA_HOME'] + '/sandboxes/' + name + '.json', 'w'))
    save(s); vm(name, {'status': 'running', 'cpuCount': int(a[a.index('--cpus') + 1]), 'memorySize': int(a[a.index('--memory-mb') + 1]) << 20, 'display': '1024x768'}); sys.exit(0)
if a[:2] in (['spaces', 'start'], ['spaces', 'stop']):
    vm(a[2].split(':')[-1], {'status': 'running' if a[1] == 'start' else 'stopped'})
    print(json.dumps({'space': a[2], 'state': 'running' if a[1] == 'start' else 'stopped'})); sys.exit(0)
if a[:2] == ['spaces', 'delete']: save([x for x in load() if x['id'] != a[2]]); vm(a[2].split(':')[-1], None); sys.exit(0)
if a[:2] == ['spaces', 'add']:
    s = load()
    for x in s:
        if x['id'] == a[2]: x['name'] = a[a.index('--name') + 1]
    save(s); sys.exit(0)
if a[:2] == ['sb', 'exec']:
    print('exec', a[3][:300], file=log, flush=True)
    # The desktop has Apple's own apps, and nothing else until it's sent.
    if a[3].startswith('/usr/bin/open -b ') and 'com.apple.' not in a[3].split(' ')[2]: print('Unable to find application', file=sys.stderr); sys.exit(1)
    print('/Users/lume'); sys.exit(0)
if a[:2] == ['sb', 'cp']: print('copied', file=log, flush=True); sys.exit(0)
if a[:2] == ['runtime', 'setup']: sys.exit(0)
if a[:2] == ['sb', 'view']: print('Viewer for %s: %s/viewer/#ticket=e2e-ticket&files=%%2Fhome' % (a[2], os.environ['HOVER_E2E_SITE'])); sys.exit(0)
if a[:1] == ['teleport'] and 'Cua Spaces.app' not in sys.argv[0]:
    print('cua: unsupported: teleport ships with Cua Spaces', file=sys.stderr); sys.exit(2)
if a[:2] == ['teleport', 'providers']:
    print(json.dumps([{'id': 'chrome', 'display_name': 'Google Chrome / Chromium', 'app_ids': ['com.google.Chrome', 'Google Chrome'], 'installed': True, 'macos': True}])); sys.exit(0)
if a[:2] == ['teleport', 'manifest']:
    print(json.dumps({'provider_id': 'chrome', 'scope': 'full_profile', 'notes': ['Full profile includes cookies, saved logins, and history.'], 'items': [
        {'label': 'Open tabs', 'rel_path': 'tabs.json', 'count': 3, 'count_noun': 'tabs', 'sensitive': False, 'default_checked': True},
        {'label': 'Cookies', 'rel_path': 'Default/Cookies', 'sensitive': True, 'default_checked': False},
        {'label': 'Bookmarks', 'rel_path': 'Default/Bookmarks', 'sensitive': False, 'default_checked': True}]})); sys.exit(0)
if a[:2] == ['teleport', 'push']:
    # The token comes in the environment, never on the command line.
    print('push token-in-env' if os.environ.get('CUA_ENV_TOKEN') == 'e2e-token' and 'e2e-token' not in a else 'push token-missing', file=log, flush=True)
    for x in ('progress 0 4096', 'progress 4096 4096', 'teleported chrome (2 items, 4096 bytes), launched'): print(x, flush=True); time.sleep(0.2)
    sys.exit(0)
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
