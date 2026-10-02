#!/usr/bin/python3
"""A stand-in Codex ACP agent for Hover's background E2E run (tests/macos/e2e).

It works the way a real agent does, against Hover's real backend: it asks before a
command, hands work to two subagents, drives Hover's built-in browser through the MCP
server Hover gave the session (the relay, the socket, the host's WKWebView), reports
computer-use steps (only as steps: no real app is ever touched), runs a command and
edits a file, and answers in Markdown. A reply gets a short second turn."""
import json, os, subprocess, sys, threading, time

out_lock = threading.Lock()
def send(m):
    with out_lock:
        sys.stdout.write(json.dumps(m) + '\n'); sys.stdout.flush()
def up(u): send({'jsonrpc': '2.0', 'method': 'session/update', 'params': {'sessionId': 'e2e-session', 'update': u}})
def call(tid, kind, title, status, raw_in=None, raw_out=None, **extra):
    u = {'sessionUpdate': 'tool_call' if status == 'in_progress' else 'tool_call_update', 'toolCallId': tid, 'kind': kind, 'title': title, 'status': status}
    if raw_in is not None: u['rawInput'] = raw_in
    if raw_out is not None: u['rawOutput'] = raw_out
    u.update(extra); up(u)
def say(text): up({'sessionUpdate': 'agent_message_chunk', 'content': {'type': 'text', 'text': text}})
log = open(os.environ['HOVER_E2E_ROOT'] + '/agent.log', 'a')
def note(*a): print(*a, file=log, flush=True)

servers, permission, waiting = [], {}, {}
folder = os.getcwd()
turns = 0

class Mcp:
    """The MCP server Hover named in session/new, started as the agent's tool would."""
    def __init__(self, spec):
        self.p = subprocess.Popen([spec['command'], *spec['args']], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.n = 0
        self.ask('initialize', {'protocolVersion': '2025-06-18', 'capabilities': {}, 'clientInfo': {'name': 'e2e-agent', 'version': '1'}})
        self.p.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n'); self.p.stdin.flush()
    def ask(self, method, params):
        self.n += 1
        self.p.stdin.write(json.dumps({'jsonrpc': '2.0', 'id': self.n, 'method': method, 'params': params}) + '\n'); self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line: raise RuntimeError('the browser server closed')
        return json.loads(line)
    def tool(self, name, args):
        r = self.ask('tools/call', {'name': name, 'arguments': args})['result']
        text = ' '.join(c.get('text', '') for c in r['content'] if c['type'] == 'text')
        note(name, '→', text[:300].replace('\n', ' | '))
        return r, text

def work(prompt_id):
    global turns
    turns += 1
    cwd = folder
    if turns > 1:
        say('Got it: **the button now says “Signed in”**. Nothing else changed.')
        send({'jsonrpc': '2.0', 'id': prompt_id, 'result': {'stopReason': 'end_turn'}}); return
    # 1. A command that asks first (the session is set to Ask first).
    ev = threading.Event(); waiting['ev'] = ev
    send({'jsonrpc': '2.0', 'id': 900, 'method': 'session/request_permission', 'params': {'sessionId': 'e2e-session',
        'toolCall': {'toolCallId': 'run-1', 'kind': 'execute', 'title': 'npm run dev', 'rawInput': {'command': 'npm run dev'}},
        'options': [{'optionId': 'yes', 'name': 'Allow', 'kind': 'allow_once'}, {'optionId': 'no', 'name': 'Deny', 'kind': 'reject_once'}]}})
    ev.wait(120)
    if permission.get('outcome') != 'yes':
        say('You turned that down, so I stopped.'); send({'jsonrpc': '2.0', 'id': prompt_id, 'result': {'stopReason': 'end_turn'}}); return
    call('run-1', 'execute', 'npm run dev', 'in_progress', {'command': 'npm run dev'})
    call('run-1', 'execute', 'npm run dev', 'completed', raw_out={'stdout': 'VITE ready\n  Local: ' + os.environ['HOVER_E2E_SITE'] + '/', 'exit_code': 0})
    # 2. Two subagents at once: the helpers at the desk.
    up({'sessionUpdate': 'agent_thought_chunk', 'content': {'type': 'text', 'text': 'Split it: one finds the form code, one writes the test.'}})
    for i, (kind, task) in enumerate([('explore', 'Find the sign-in form'), ('test-writer', 'Write a test for sign-in')]):
        call(f'agent-{i}', 'other', 'Task', 'in_progress', {'subagent_type': kind, 'description': task, 'prompt': task + ' in ' + cwd})
    time.sleep(float(os.environ.get('HOVER_E2E_HOLD', '6')))
    for i in range(2): call(f'agent-{i}', 'other', 'Task', 'completed', raw_out=['Found src/login.html', 'Wrote tests/login.test.js'][i])
    # 3. Hover's browser, through the MCP server Hover gave this session.
    spec = next((s for s in servers if s['name'] == 'hover-browser'), None)
    if spec is None:
        say('Hover gave me no browser.'); send({'jsonrpc': '2.0', 'id': prompt_id, 'result': {'stopReason': 'end_turn'}}); return
    b = Mcp(spec)
    names = [t['name'] for t in b.ask('tools/list', {})['result']['tools']]
    note('tools', names)
    steps = [('browser_open', {'url': os.environ['HOVER_E2E_SITE'] + '/index.html'}), ('browser_snapshot', {}),
             ('browser_type', {'label': 'Name', 'text': 'Ada', 'submit': True}), ('browser_wait', {'text': 'Hello, Ada!'}),
             ('browser_screenshot', {})]
    for n, (tool, args) in enumerate(steps):
        tid = f'web-{n}'
        call(tid, 'other', 'mcp__hover-browser__' + tool, 'in_progress', args)
        r, text = b.tool(tool, args)
        call(tid, 'other', 'mcp__hover-browser__' + tool, 'failed' if r.get('isError') else 'completed', raw_out=text[:2000])
        if r.get('isError'): note('FAILED', tool, text)
        time.sleep(2.0)
    b.p.stdin.close()
    # 4. Computer use on its own desktop (a Cua Space), through the server Hover gave it.
    cs = next((s for s in servers if s['name'] == 'cua-space'), None)
    if cs is None: note('NO SPACE SERVER')
    else:
        c = Mcp(cs)
        note('space tools', [t['name'] for t in c.ask('tools/list', {})['result']['tools']])
        for n, (tool, args) in enumerate([('computer_screenshot', {}), ('computer_click', {'x': 120, 'y': 80}), ('computer_type', {'text': 'Ada'})]):
            tid = f'cua-{n}'
            call(tid, 'other', 'mcp__cua-space__' + tool, 'in_progress', args)
            r, text = c.tool(tool, args)
            call(tid, 'other', 'mcp__cua-space__' + tool, 'failed' if r.get('isError') else 'completed', raw_out=text)
            time.sleep(1.2)
        c.p.stdin.close()
    time.sleep(float(os.environ.get('HOVER_E2E_HOLD', '6')) / 2)
    # 5. An edit, then the answer.
    with open(os.path.join(cwd, 'login.html'), 'a') as f: f.write('<!-- signed in -->\n')
    call('edit-1', 'edit', 'Edit login.html', 'completed', locations=[{'path': cwd + '/login.html'}],
         content=[{'type': 'diff', 'path': cwd + '/login.html', 'oldText': '<form>\n', 'newText': '<form>\n<!-- signed in -->\n'}])
    say('Done. **Sign-in works**: I opened the page in Hover’s browser, typed a name and checked the greeting.\n\n| Check | Result |\n|---|---|\n| Form submits | ✓ |\n| Greeting shows | ✓ |\n')
    send({'jsonrpc': '2.0', 'id': prompt_id, 'result': {'stopReason': 'end_turn'}})

def safe(ident):
    try: work(ident)
    except Exception as e:
        import traceback; note('CRASH', traceback.format_exc())
        say('The stand-in agent crashed: ' + str(e)); send({'jsonrpc': '2.0', 'id': ident, 'result': {'stopReason': 'end_turn'}})

def main():
  global servers, folder
  for line in sys.stdin:
      m = json.loads(line); method = m.get('method'); ident = m.get('id'); p = m.get('params', {})
      if method is None and ident == 900:
          permission['outcome'] = m.get('result', {}).get('outcome', {}).get('optionId'); waiting['ev'].set(); continue
      if method == 'initialize': result = {'protocolVersion': 1, 'agentCapabilities': {'loadSession': True}}
      elif method == 'session/new':
          servers = p.get('mcpServers') or []; folder = p.get('cwd') or folder; note('mcpServers', json.dumps(servers)); result = {'sessionId': 'e2e-session'}
      elif method == 'session/load': servers = p.get('mcpServers') or servers; result = {}
      elif method == 'session/prompt':
          threading.Thread(target=safe, args=(ident,), daemon=True).start(); continue
      elif method == 'session/cancel': continue
      else: result = {}
      if ident is not None: send({'jsonrpc': '2.0', 'id': ident, 'result': result})

main()
