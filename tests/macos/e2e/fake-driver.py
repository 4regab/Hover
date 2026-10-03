#!/usr/bin/python3
"""A stand-in for the Cua Driver inside a Space (cua-spacesd's /mcp), for Hover's E2E
run: streamable HTTP on localhost, a session per connection, the token header checked,
and every tool call logged to cua.log with the desktop and the agent's own session."""
import json, os, sys, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
root = sys.argv[2]
TOOLS = [{'name': n, 'description': n, 'inputSchema': {'type': 'object', 'properties': {'session': {'type': 'string'}, 'x': {'type': 'integer'}}}}
         for n in ['computer_screenshot', 'computer_click', 'computer_type', 'kill_app', 'start_session', 'end_session']]
sessions = {}
def log(*a):
    with open(root + '/cua.log', 'a') as f: print(*a, file=f, flush=True)

class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        if self.headers.get('x-cua-env-authorization') != 'Bearer e2e-token': self.send_response(401); self.end_headers(); return
        space = self.path.split('/')[2] if self.path.startswith('/s/') else '?'
        m = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))))
        sid = self.headers.get('Mcp-Session-Id')
        meth = m.get('method')
        if meth == 'initialize':
            sid = 'mcp-' + uuid.uuid4().hex[:8]; sessions[sid] = set()
            r = {'protocolVersion': '2025-06-18', 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'cua-driver', 'version': '0.32.0'}}
        elif sid not in sessions: self.send_response(404); self.end_headers(); return
        elif 'id' not in m: self.send_response(202); self.send_header('Content-Length', '0'); self.end_headers(); return
        elif meth == 'tools/list': r = {'tools': TOOLS}
        elif meth == 'tools/call':
            p = m['params']; args = p.get('arguments', {}); label = args.get('session', '')
            if p['name'] == 'start_session': sessions[sid].add(label)
            elif label not in sessions[sid]:
                r = {'content': [{'type': 'text', 'text': 'session is not available to this transport'}], 'isError': True, 'structuredContent': {'refusal': {'code': 'session_ended'}}}
                return self.answer(m, r, sid)
            log('call', 'local:' + space, p['name'], json.dumps({k: v for k, v in args.items() if k != 'session'}), 'session=' + label)
            r = {'content': [{'type': 'text', 'text': '%s done in %s' % (p['name'], space)}], 'isError': False}
        else: r = {}
        self.answer(m, r, sid)
    def answer(self, m, r, sid):
        body = json.dumps({'jsonrpc': '2.0', 'id': m['id'], 'result': r}).encode()
        self.send_response(200); self.send_header('Content-Type', 'application/json'); self.send_header('Mcp-Session-Id', sid)
        self.send_header('Content-Length', str(len(body))); self.end_headers(); self.wfile.write(body)

ThreadingHTTPServer(('127.0.0.1', int(sys.argv[1])), H).serve_forever()
