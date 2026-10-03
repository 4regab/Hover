"""Packaged backend integration: only a fake ACP tool in the disposable sandbox."""
import base64, json, os, queue, subprocess, sys, threading, time
from pathlib import Path
app, root = map(Path, sys.argv[1:])
assert root.name.startswith('hover-sandbox.') and str(root).startswith('/private/tmp/'), root
project = root/'project'; project.mkdir(exist_ok=True)
# A git repo with one change, for the Files and Diff panels.
(project/'app.js').write_text('a\n')
# None of the user's git config (the sandbox can't read it, and it mustn't change the test).
git_env={**os.environ,'GIT_CONFIG_GLOBAL':'/dev/null','GIT_CONFIG_NOSYSTEM':'1'}
for c in (['init','-q'],['add','.'],['-c','user.email=t@t','-c','user.name=t','commit','-qm','init']): subprocess.run(['/usr/bin/git',*c],cwd=project,check=True,capture_output=True,env=git_env)
(project/'app.js').write_text('a\nb\n')
# A stand-in executable exercises actual process launch, approval, stream, resume
# and cancellation without credentials, external network, or a real coding agent.
fake = root/'fake-bin'/'codex-acp'
fake.write_text('''#!/usr/bin/python3
import json,sys,subprocess,os
pending=None
# The project, as session/new names it: one agent process serves every folder, so its own
# working folder is not the project's.
cwd=None
for line in sys.stdin:
 m=json.loads(line); method=m.get('method'); ident=m.get('id'); p=m.get('params',{})
 if method is None and ident==900 and pending is not None:
  assert m['result']['outcome']['optionId']=='yes',m
  # What the desk's panels read: a thought, a command with output, an edit with a
  # diff, a subagent, a page fetched, a local server, and a computer-use screenshot.
  def up(u): print(json.dumps({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':'fake-session','update':u}}),flush=True)
  up({'sessionUpdate':'agent_thought_chunk','content':{'type':'text','text':'Plan: run the build, then check it.'}})
  for u in [
   {'toolCallId':'run-1','kind':'execute','title':'npm run dev','rawInput':{'command':'npm run dev'},'status':'completed','rawOutput':{'stdout':'ready on http://localhost:5173/','exit_code':0}},
   {'toolCallId':'edit-2','kind':'edit','title':'Edit app.js','locations':[{'path':cwd+'/app.js'}],'status':'completed','content':[{'type':'diff','path':cwd+'/app.js','oldText':'a\\n','newText':'a\\nb\\n'}]},
   {'toolCallId':'agent-1','kind':'other','title':'Task','rawInput':{'subagent_type':'explore','description':'Find the config','prompt':'Look for config files'},'status':'completed','rawOutput':'Found config.json'},
   {'toolCallId':'fetch-1','kind':'fetch','title':'Fetch docs','rawInput':{'url':'https://example.com/docs'},'status':'completed'},
   {'toolCallId':'cua-1','kind':'other','title':'cua-driver: screenshot','rawInput':{},'status':'completed'}]:
   up(dict(sessionUpdate='tool_call',**u))
  print(json.dumps({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':'fake-session','update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':'Sandbox reply'}}}}),flush=True)
  print(json.dumps({'jsonrpc':'2.0','id':pending,'result':{'stopReason':'end_turn'}}),flush=True)
  pending=None;continue
 if method=='initialize': result={'protocolVersion':1,'agentCapabilities':{'loadSession':True}}
 elif method=='session/new':
  cwd=p.get('cwd')
  # With computer use on, every session is handed the stand-in cua-driver over stdio.
  open(os.environ['HOVER_SANDBOX_ROOT']+'/mcp-servers.json','w').write(json.dumps(p.get('mcpServers')))
  result={'sessionId':'fake-session'}
 elif method=='session/load': result={}
 elif method=='session/cancel':
  if pending is not None:print(json.dumps({'jsonrpc':'2.0','id':pending,'result':{'stopReason':'cancelled'}}),flush=True)
  pending=None;continue
 elif method=='session/prompt':
  pending=ident
  child=subprocess.Popen(['/bin/sleep','60'])
  open(os.environ['HOVER_SANDBOX_ROOT']+'/fixture-child.pid','w').write(str(child.pid))
  print(json.dumps({'jsonrpc':'2.0','id':900,'method':'session/request_permission','params':{'sessionId':'fake-session','toolCall':{'toolCallId':'edit-1','kind':'edit','title':'Sandbox edit','rawInput':{'path':'fixture.txt'}},'options':[{'optionId':'yes','name':'Allow','kind':'allow_once'},{'optionId':'no','name':'Deny','kind':'reject_once'}]}}),flush=True)
  continue
 else: result={}
 if ident is not None: print(json.dumps({'jsonrpc':'2.0','id':ident,'result':result}),flush=True)
''')
fake.chmod(0o700)
codex = root/'fake-bin'/'codex'; codex.write_text('#!/bin/sh\necho "Logged in (sandbox fixture)"\n'); codex.chmod(0o700)
# A stand-in Cua Driver on PATH, found before any real install, so no real driver or
# CuaDriver.app is started: it answers the version, daemon and permission checks.
cua = root/'fake-bin'/'cua-driver'
cua.write_text('''#!/bin/sh
case "$1" in
 --version) echo "cua-driver 0.0.0-sandbox";;
 status) echo "Cua Driver daemon is running";;
 permissions) echo '{"accessibility":true,"screen_recording":true,"source":{"attribution":"driver-daemon"}}';;
 *) exit 2;;
esac
''')
cua.chmod(0o700)
(root/'home').mkdir(exist_ok=True)
# A home of its own: the user's is unreadable here, and the backend starts its tools in it.
env = dict(os.environ, HOME=str(root/'home'), PATH=str(root/'fake-bin')+':/usr/bin:/bin', HOVER_DATA_DIR=str(root/'integration-data'), GIT_CONFIG_GLOBAL='/dev/null', GIT_CONFIG_NOSYSTEM='1')
key = base64.b64encode(bytes(range(32))).decode()
def launch():
 p=subprocess.Popen([str(app/'Contents/Resources/hover-guardian'),str(app/'Contents/Resources/backend/hover-backend')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,env=env)
 q=queue.Queue()
 def read():
  for line in p.stdout:
   try:q.put(json.loads(line))
   except ValueError:pass
 threading.Thread(target=read,daemon=True).start()
 # Its log, read as it comes: a full stderr pipe stops the backend mid-write.
 log=[]
 def drain():
  for line in p.stderr: log.append(line.rstrip()); del log[:-40]
 threading.Thread(target=drain,daemon=True).start()
 p.log=log
 def send(m):p.stdin.write(json.dumps(m)+'\n');p.stdin.flush()
 def until(predicate):
  end=time.monotonic()+25
  while time.monotonic()<end:
   try: m=q.get(timeout=max(.01,end-time.monotonic()))
   except queue.Empty:
    if p.poll() is not None: raise AssertionError('Backend exited: '+p.stderr.read())
    raise AssertionError('Backend timeout; its log:\n'+'\n'.join(log))
   if m.get('type')=='toast':raise AssertionError(m)
   if predicate(m):return m
  raise AssertionError('Backend timeout; its log:\n'+'\n'.join(log))
 send({'type':'initialize','key':key});until(lambda m:m.get('type')=='initialized')
 return p,send,until
p,send,until=launch()
try:
 send({'type':'getSettings'})
 assert until(lambda m:m.get('type')=='preferences')['noticeSeen'] is False
 send({'type':'saveSettings','noticeSeen':False,'hover':True})
 first=until(lambda m:m.get('type')=='preferences')
 assert first['noticeSeen'] is False and first['computerUse'] is False,first
 assert first['kiroAutoCompact'] is False and first['kiroCompactAt']==80,first
 send({'type':'saveSettings','noticeSeen':True,'hover':False,'maxRunning':4,'quotaItems':[],'computerUse':True,'kiroAutoCompact':True,'kiroCompactAt':60,'tools':[{'id':'codex','access':'always','idle':15,'hideSteps':True}]})
 preferences=until(lambda m:m.get('type')=='preferences')
 assert preferences['noticeSeen'] is True and preferences['hover'] is False and preferences['maxRunning']==4 and preferences['computerUse'] is True,preferences
 assert preferences['kiroAutoCompact'] is True and preferences['kiroCompactAt']==60,preferences
 send({'type':'computerUse'})
 cu=until(lambda m:m.get('type')=='computerUse' and m['checked'])
 assert cu['on'] and cu['installed'] and cu['ready'] and cu['permissions']=='granted' and cu['version']=='cua-driver 0.0.0-sandbox',cu
 codex=next(t for t in preferences['tools'] if t['id']=='codex')
 assert codex['access']=='always' and codex['idle']==15 and codex['hideSteps'] is True,codex
 send({'type':'ready'})
 until(lambda m:m.get('type')=='state' and any(t['id']=='codex' and t['ready'] for t in m['tools']))
 send({'type':'new','tool':'codex','folder':str(project),'prompt':'Sandbox secret','access':'always'})
 waiting=until(lambda m:m.get('type')=='state' and m['sessions'] and m['sessions'][0]['stage']=='waiting')
 session=waiting['sessions'][0]
 send({'type':'answer','id':session['id'],'ask':session['ask']['id'],'answer':'allow'})
 state=until(lambda m:m.get('type')=='state' and m['sessions'] and m['sessions'][0]['stage']=='done')
 s=state['sessions'][0];assert s['turns'][0]['answer']=='Sandbox reply',s
 kinds=[x['k'] for x in s['turns'][0]['steps']]
 assert 'thought' in kinds and 'agent' in kinds and 'run' in kinds and 'edit' in kinds,kinds
 def desk(what):
  send({'type':'desk','id':s['id'],'what':what})
  d=until(lambda m:m.get('type')=='desk' and m.get('what')==what)['data']
  assert 'error' not in d,(what,d); return d
 probe=desk('probe'); assert probe['folder'] and probe['git'] and probe['commands']>=1 and probe['agents']==1 and probe['pages']>=2,probe
 term=desk('terminal'); assert 'localhost:5173' in json.dumps(term),term
 files=desk('files'); assert 'app.js' in json.dumps(files),files
 diff=desk('diff'); assert '+b' in json.dumps(diff),diff
 agents=desk('agents'); assert agents['agents'][0]['name']=='explore' and 'Found config' in (agents['agents'][0]['out'] or ''),agents
 pages=desk('browser'); urls=json.dumps(pages); assert 'example.com/docs' in urls and 'localhost:5173' in urls,pages
 print('Desk panels passed: probe, terminal, files, diff, subagents, browser pages and the thought step.')
 servers=json.loads((root/'mcp-servers.json').read_text())
 # Behind Hover's guard (background only), which is written where the agent can read it.
 assert len(servers)==1 and servers[0]['name']=='cua-driver' and servers[0]['command']=='/usr/bin/perl' and servers[0]['env']==[],servers
 guard,exe,mode=servers[0]['args']; assert exe==str(cua) and mode=='mcp' and guard.endswith('/cua/guard.pl') and Path(guard).is_file(),servers
 send({'type':'reply','id':s['id'],'text':'Continue'})
 waiting=until(lambda m:m.get('type')=='state' and m['sessions'][0]['stage']=='waiting')
 send({'type':'answer','id':s['id'],'ask':waiting['sessions'][0]['ask']['id'],'answer':'allow'})
 until(lambda m:m.get('type')=='state' and len(m['sessions'][0]['turns'])==2 and m['sessions'][0]['stage']=='done')
 send({'type':'shutdown'});p.wait(timeout=15);assert p.returncode==0
 raw=b''.join(f.read_bytes() for f in (root/'integration-data/agents').glob('*.dat'))
 assert b'Sandbox secret' not in raw
 bad=subprocess.run([str(app/'Contents/Resources/hover-guardian'),str(app/'Contents/Resources/backend/hover-backend')],input=json.dumps({'type':'initialize','key':base64.b64encode(bytes([255])*32).decode()})+'\n',capture_output=True,text=True,env=env,timeout=15)
 assert 'backendFailure' in bad.stdout and 'cannot decrypt' in bad.stdout,bad.stdout
 assert raw==b''.join(f.read_bytes() for f in (root/'integration-data/agents').glob('*.dat'))
 p,send,until=launch()
 send({'type':'getSettings'})
 persisted=until(lambda m:m.get('type')=='preferences')
 assert persisted==preferences,(persisted,preferences)
 send({'type':'ready'})
 until(lambda m:m.get('type')=='state' and len(m['history'])==1)
 send({'type':'history','key':s['key']})
 transcript=until(lambda m:m.get('type')=='transcript');assert len(transcript['session']['turns'])==2
 send({'type':'delete','key':s['key']});until(lambda m:m.get('type')=='state' and not m['history'])
 # Stop a waiting turn, then lose the UI while another approval is pending.
 send({'type':'refresh'})
 until(lambda m:m.get('type')=='state' and any(t['id']=='codex' and t['ready'] for t in m['tools']))
 send({'type':'new','tool':'codex','folder':str(project),'prompt':'Cancel me','access':'always'})
 waiting=until(lambda m:m.get('type')=='state' and m['sessions'] and m['sessions'][0]['stage']=='waiting')
 send({'type':'stop','id':waiting['sessions'][0]['id']})
 until(lambda m:m.get('type')=='state' and m['sessions'][0]['stage']=='stopped')
 send({'type':'new','tool':'codex','folder':str(project),'prompt':'UI crash','access':'always'})
 until(lambda m:m.get('type')=='state' and any(s['stage']=='waiting' for s in m['sessions']))
 child=int((root/'fixture-child.pid').read_text())
 # EOF simulates loss of the UI connection and must terminate the helper.
 p.stdin.close();p.wait(timeout=15);assert p.returncode==0
 try:os.kill(child,0)
 except ProcessLookupError:pass
 else:raise AssertionError('Fixture descendant survived host EOF')
 print('Packaged backend passed: fake-tool launch, approvals, reply, cancellation, encrypted history, restart, deletion and crash cleanup.')
finally:
 if p.poll() is None:
  p.stdin.close();p.wait(timeout=15)
