// The desk menu's panels, as T3 Code's right panel has them: Browser, Terminal,
// Files, Diff, Pull request, Linked pull requests, Agents, and Screen (T3 Code's
// Device). Pure HTML builders: main.js owns the state, asks Hover for the data
// ({type:'desk'}) and puts what these return into the side panel. Everything the
// agent or the folder wrote is escaped here; nothing it wrote becomes markup.

export const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const dur = ms => ms == null ? '' : ms < 1000 ? `${Math.round(ms)} ms` : ms < 60e3 ? `${(ms / 1000).toFixed(ms < 10e3 ? 1 : 0)} s` : `${Math.floor(ms / 60e3)}m ${String(Math.round(ms % 60e3 / 1000) % 60).padStart(2, '0')}s`;
const num = n => (n ?? 0).toLocaleString();
const svg = d => `<svg viewBox="0 0 24 24" aria-hidden="true">${d}</svg>`;

// Line icons in Lucide's style (ISC), as T3 Code's menu uses them.
export const ICONS = {
  browser: svg('<circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10Z"/>'),
  terminal: svg('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="m7 11 2-2-2-2M11 13h4"/>'),
  files: svg('<path d="M20 7h-3a2 2 0 0 1-2-2V2"/><path d="M9 18a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h7l4 4v10a2 2 0 0 1-2 2Z"/><path d="M3 7.6v12.8A1.6 1.6 0 0 0 4.6 22h9.8"/>'),
  diff: svg('<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M9 10h6M12 7v6M9 17h6"/>'),
  pr: svg('<circle cx="18" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><path d="M13 6h3a2 2 0 0 1 2 2v7M6 9v12"/>'),
  linked: svg('<path d="M9 17H7A5 5 0 0 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/>'),
  agents: svg('<path d="M12 8V4H8"/><rect x="4" y="8" width="16" height="12" rx="2"/><path d="M2 14h2M20 14h2M15 13v2M9 13v2"/>'),
  screen: svg('<rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>'),
  chevron: svg('<path d="m9 18 6-6-6-6"/>'),
  back: svg('<path d="m12 19-7-7 7-7M19 12H5"/>'),
  ext: svg('<path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>'),
  reload: svg('<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>'),
  file: svg('<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><path d="M14 2v6h6"/>'),
  folder: svg('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"/>'),
  server: svg('<rect x="2" y="3" width="20" height="8" rx="2"/><rect x="2" y="13" width="20" height="8" rx="2"/><path d="M6 7h.01M6 17h.01"/>'),
  search: svg('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>'),
  fwd: svg('<path d="m12 5 7 7-7 7M5 12h14"/>'),
  github: svg('<path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.4 5.4 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65S8.93 17.38 9 18v4"/><path d="M9 18c-4.51 2-5-2-7-2"/>'),
  check: svg('<path d="M20 6 9 17l-5-5"/>'),
  copy: svg('<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/>'),
};

// The menu's rows, in T3 Code's order and with its letters. Screen is the Device row.
export const SURFACES = [
  ['browser', 'Browser', 'B'], ['terminal', 'Terminal', 'T'], ['files', 'Files', 'F'], ['diff', 'Diff', 'D'],
  ['pr', 'Pull request', 'P'], ['linked', 'Linked pull requests', 'L'], ['agents', 'Agents', 'A'], ['screen', 'Screen', 'S'],
];

/// Whether a row can open, and why not: from Hover's probe of the desk, and what
/// the page already knows (its own steps) until the probe is back.
export function availability(p, s, steps) {
  const runs = steps.filter(x => x.k === 'run').length, edits = steps.filter(x => x.k === 'edit').length;
  const wait = 'Checking…';
  return {
    browser: [true, ''],
    terminal: [(p?.commands ?? runs) > 0, 'No commands run yet.'],
    files: [p ? p.folder : true, 'The folder isn’t there any more.'],
    diff: [p ? p.folder && (p.git || edits > 0) : true, p && !p.folder ? 'The folder isn’t there any more.' : 'No changes yet.'],
    // Open even without one: the panel sets up gh, or opens a pull request.
    pr: [p ? p.git : true, p ? 'Not a Git repository.' : wait],
    linked: [(p?.linked ?? 0) > 0, p ? 'No pull requests mentioned in this session.' : wait],
    agents: [(p?.agents ?? 0) > 0, p ? 'No subagents in this session.' : wait],
    screen: [true, ''],
  };
}

/// The line under each tile of the desk card: what is there to see, at a glance.
export function tileDetail(id, p, s, browser, pages) {
  const n = (k, one, many) => `${num(k)} ${k === 1 ? one : many}`;
  switch (id) {
    case 'browser': return s.browsing ? 'In use now' : browser?.url ? label(browser.url) : pages?.length ? label(pages[0].url) : 'Open a page';
    case 'terminal': return p?.commands ? n(p.commands, 'command', 'commands') : 'Nothing run';
    case 'files': return p?.changed ? `${num(p.changed)} changed` : p?.git ? 'No changes' : 'Browse';
    case 'diff': return p?.add || p?.del ? `+${num(p.add)} −${num(p.del)}` : p?.changed ? `${num(p.changed)} file${p.changed === 1 ? '' : 's'}` : 'Clean';
    case 'pr': return !p ? '…' : p.pr ? `#${p.pr.number} ${p.pr.isDraft ? 'draft' : p.pr.state}` : !p.git ? 'No repository' : !p.gh ? 'Set up GitHub' : !p.ghAuth ? 'Sign in' : 'Open one';
    case 'linked': return p?.linked ? n(p.linked, 'mentioned', 'mentioned') : 'None';
    case 'agents': return p?.running ? `${num(p.running)} working` : p?.agents ? n(p.agents, 'subagent', 'subagents') : 'None yet';
    case 'screen': return s.testing ? 'Live' : s.apps ? 'Desktop + its apps' : 'Desktop';
  }
  return '';
}

const empty = (icon, title, text) => `<div class="dempty">${ICONS[icon] || ''}<b>${esc(title)}</b>${text ? `<span>${esc(text)}</span>` : ''}</div>`;
const loading = () => '<div class="dempty load"><i class="spin"></i><span>Reading…</span></div>';
const failed = d => d?.error ? empty('diff', 'Couldn’t read that', d.error) : null;

// ── Terminal ────────────────────────────────────────────────────────────
export function terminalHTML(d, bot) {
  if (!d) return loading();
  if (failed(d)) return failed(d);
  if (!d.commands?.length) return empty('terminal', 'No commands yet', `What ${bot} runs shows here, with its output.`);
  return `<div class="term">${d.commands.map(c => {
    const run = c.status === 'in_progress', bad = c.status === 'failed' || (c.exit != null && c.exit !== 0);
    const st = run ? '<span class="tst run"><i class="spin"></i>running</span>' : `<span class="tst${bad ? ' bad' : ' ok'}">${c.exit != null ? `exit ${c.exit}` : bad ? 'failed' : 'done'}${c.ms ? ` · ${dur(c.ms)}` : ''}</span>`;
    return `<div class="tcmd${run ? ' run' : ''}${bad ? ' bad' : ''}"><div class="th"><span class="pr">$</span><code title="${esc(c.cmd)}">${esc(c.cmd)}</code>${st}</div>${c.out ? `<pre>${esc(c.out)}</pre>` : run ? '' : '<pre class="none">No output</pre>'}</div>`;
  }).join('')}</div>`;
}

// ── Files ───────────────────────────────────────────────────────────────
const STATUS = { M: ['M', 'Modified'], A: ['A', 'Added'], D: ['D', 'Deleted'], R: ['R', 'Renamed'], '?': ['U', 'Untracked'] };
const badgeOf = st => { const [l, t] = STATUS[st] || ['M', 'Modified']; return `<span class="fst st${l}" title="${t}">${l}</span>`; };
const pm = (a, d) => `${a ? `<span class="a">+${num(a)}</span>` : ''}${d ? `<span class="d">−${num(d)}</span>` : ''}`;
const base = p => p.split('/').pop(), dirOf = p => p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '';

export function filesHTML(d, ui, bot) {
  if (!d) return loading();
  if (failed(d)) return failed(d);
  const q = (ui.find || '').toLowerCase();
  const find = `<label class="hfind"><svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg><input id="fFind" type="search" placeholder="Find a file" aria-label="Find a file" value="${esc(ui.find || '')}"></label>`;
  const row = (path, right, cls = '') => `<button class="frow${cls}" data-file="${esc(path)}" title="${esc(path)}">${ICONS.file}<span class="fn"><b>${esc(base(path))}</b>${dirOf(path) ? `<i>${esc(dirOf(path))}</i>` : ''}</span><span class="fr">${right}</span></button>`;
  if (q) {
    const hits = d.tree.filter(p => p.toLowerCase().includes(q)).slice(0, 200);
    return find + (hits.map(p => row(p, '')).join('') || '<p class="none">Nothing matches.</p>');
  }
  const changed = d.changed?.length ? `<h4 class="sh">Changed <span>${d.changed.length}</span></h4>${d.changed.map(c => row(c.path, pm(c.add, c.del) + badgeOf(c.status), c.status === 'D' ? ' gone' : '')).join('')}` : '';
  const touched = d.touched?.length ? `<h4 class="sh">${esc(bot)} looked at <span>${d.touched.length}</span></h4>${d.touched.slice(-60).reverse().map(t => row(t.path,
    [t.edit && `<em class="ed">edited${t.edit > 1 ? ` ×${t.edit}` : ''}</em>`, t.read && `<em>read${t.read > 1 ? ` ×${t.read}` : ''}</em>`].filter(Boolean).join(''))).join('')}` : '';
  return find + changed + touched + `<h4 class="sh">All files <span>${num(d.tree.length)}${d.more ? '+' : ''}</span></h4>${treeHTML(d.tree, ui.open, new Set((d.changed || []).map(c => c.path)))}`
    + (d.more ? '<p class="none">Showing the first 5,000 files. Find one by name above.</p>' : '');
}

// The folder as a tree: folders first, each folded until opened.
function treeHTML(paths, open, changed) {
  const root = { d: new Map(), f: [] };
  for (const p of paths) {
    const parts = p.split('/'); let n = root;
    for (let i = 0; i < parts.length - 1; i++) { if (!n.d.has(parts[i])) n.d.set(parts[i], { d: new Map(), f: [] }); n = n.d.get(parts[i]); }
    n.f.push(p);
  }
  const hot = new Set(); for (const c of changed) { const parts = c.split('/'); for (let i = 1; i < parts.length; i++) hot.add(parts.slice(0, i).join('/')); }
  const walk = (n, prefix, depth) => {
    let h = '';
    for (const [name, child] of [...n.d].sort((a, b) => a[0].localeCompare(b[0]))) {
      const path = prefix + name, isOpen = open.has(path);
      h += `<button class="trow dir${isOpen ? ' open' : ''}${hot.has(path) ? ' hot' : ''}" style="--d:${depth}" data-dir="${esc(path)}" aria-expanded="${isOpen}">${ICONS.chevron}${ICONS.folder}<span>${esc(name)}</span></button>`;
      if (isOpen) h += walk(child, path + '/', depth + 1);
    }
    for (const f of n.f) h += `<button class="trow${changed.has(f) ? ' hot' : ''}" style="--d:${depth}" data-file="${esc(f)}" title="${esc(f)}"><i></i>${ICONS.file}<span>${esc(base(f))}</span></button>`;
    return h;
  };
  return `<div class="tree">${walk(root, '', 0) || '<p class="none">The folder is empty.</p>'}</div>`;
}

export function fileHTML(d, path) {
  const head = `<div class="fbar"><button class="x" data-fback aria-label="Back to the files" title="Back">${ICONS.back}</button><span class="fn"><b>${esc(base(path))}</b>${dirOf(path) ? `<i>${esc(dirOf(path))}</i>` : ''}</span>${d?.size != null ? `<em>${d.size < 1024 ? d.size + ' B' : (d.size / 1024).toFixed(d.size < 10240 ? 1 : 0) + ' KB'}</em>` : ''}</div>`;
  if (!d) return head + loading();
  if (d.error) return head + empty('file', 'Couldn’t open it', d.error);
  if (d.binary) return head + empty('file', 'Not a text file', 'Binary files aren’t shown here.');
  const lines = d.text.replace(/\r\n/g, '\n').replace(/\n$/, '').split('\n'), shown = lines.slice(0, 6000);
  return head + `<div class="code"><pre>${shown.map(l => `<span>${esc(l) || ' '}</span>`).join('')}</pre></div>`
    + (d.truncated || lines.length > shown.length ? '<p class="none">The rest of this file isn’t shown.</p>' : '');
}

// ── Diff ────────────────────────────────────────────────────────────────
export function diffHTML(d, ui) {
  if (!d) return loading();
  if (d.error) return empty('diff', 'Couldn’t read the diff', d.error);
  if (!d.files?.length) return empty('diff', 'No changes', d.git ? 'The working tree matches the last commit.' : 'Nothing edited yet.');
  const a = d.files.reduce((n, f) => n + f.add, 0), del = d.files.reduce((n, f) => n + f.del, 0);
  let budget = 4000;
  const head = `<div class="dsum"><b>${d.files.length} file${d.files.length === 1 ? '' : 's'} changed</b>${pm(a, del)}${d.branch ? `<span class="chip br">${ICONS.pr}${esc(d.branch)}</span>` : ''}</div>`
    + (d.git ? '' : '<p class="note">Not a Git repository: these are the parts of each edit the session kept.</p>')
    + (d.truncated ? '<p class="note">The diff is long; the end isn’t shown.</p>' : '');
  return head + d.files.map((f, i) => {
    const key = f.path, open = ui.diffOpen.has(key) ? ui.diffOpen.get(key) : i < 12;
    const name = f.old ? `${esc(f.old)} → ${esc(f.path)}` : esc(f.path);
    let body = '';
    if (open) {
      if (f.binary) body = '<p class="none">Binary file</p>';
      else { const r = hunks(f.patch, budget); budget -= r.n; body = r.html || '<p class="none">No text changes</p>'; }
    }
    return `<div class="dfile${open ? ' open' : ''}"><button class="dfh" data-df="${esc(key)}" aria-expanded="${open}">${ICONS.chevron}${badgeOf(f.status === 'A' ? 'A' : f.status)}<span class="dp" title="${name}">${name}</span><span class="fr">${pm(f.add, f.del)}</span></button>${body}</div>`;
  }).join('');
}

// One file's hunks, with the old and the new line numbers, up to a budget of lines.
function hunks(patch, budget) {
  let o = 0, n = 0, rows = 0, html = '';
  for (const l of (patch || '').split('\n')) {
    if (rows >= budget) { html += '<tr class="more"><td colspan="3">More lines aren’t shown.</td></tr>'; break; }
    const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(l);
    if (m) { o = +m[1]; n = +m[2]; html += `<tr class="hk"><td colspan="3">${esc(l)}</td></tr>`; rows++; continue; }
    if (l.startsWith('@@')) { html += `<tr class="hk"><td colspan="3">${esc(l)}</td></tr>`; continue; }
    if (l.startsWith('\\')) { html += `<tr class="hk"><td colspan="3">${esc(l.replace(/^\\ ?/, ''))}</td></tr>`; continue; }
    const t = l[0], text = esc(l.slice(1)) || ' ';
    if (t === '+') html += `<tr class="a"><td></td><td>${n++ || ''}</td><td>${text}</td></tr>`;
    else if (t === '-') html += `<tr class="d"><td>${o++ || ''}</td><td></td><td>${text}</td></tr>`;
    else html += `<tr><td>${o++ || ''}</td><td>${n++ || ''}</td><td>${text}</td></tr>`;
    rows++;
  }
  return { html: html ? `<div class="hunks"><table>${html}</table></div>` : '', n: rows };
}

// ── Pull requests ───────────────────────────────────────────────────────
const PR_STATE = { open: ['Open', 'op'], draft: ['Draft', 'dr'], merged: ['Merged', 'mg'], closed: ['Closed', 'cl'] };
const prPill = p => { const [t, c] = PR_STATE[p.isDraft && p.state === 'open' ? 'draft' : p.state] || PR_STATE.open; return `<span class="prs ${c}">${ICONS.pr}${t}</span>`; };
const REVIEW = { APPROVED: 'Approved', CHANGES_REQUESTED: 'Changes requested', REVIEW_REQUIRED: 'Review required' };

export function prHTML(d, md, gh, ui) {
  if (!d) return loading();
  if (d.setup || gh && (!gh.installed || !gh.signedIn) && gh.checked && d.error && !d.create) return ghSetupHTML(gh, d.setup);
  if (d.create) return prCreateHTML(d.create, ui);
  if (d.error) return empty('pr', 'No pull request', d.error);
  const checks = d.pass + d.fail + d.pending + d.skip;
  const tally = checks ? `<div class="checks"><b>Checks</b>${d.fail ? `<span class="bad">✗ ${d.fail} failing</span>` : ''}${d.pending ? `<span class="pend">◌ ${d.pending} pending</span>` : ''}${d.pass ? `<span class="ok">✓ ${d.pass} passed</span>` : ''}${d.skip ? `<span>${d.skip} skipped</span>` : ''}</div>`
    + `<div class="clist">${d.checks.map(c => `<button class="crow ${c.state}"${c.url ? ` data-ext="${esc(c.url)}"` : ' disabled'}><i></i><span>${esc(c.name)}</span></button>`).join('')}</div>` : '';
  return `<div class="prc"><div class="prt">${prPill(d)}<span class="prn">#${d.number}</span></div><h3>${esc(d.title)}</h3>`
    + `<div class="prm"><span class="chip br">${esc(d.head)} → ${esc(d.base)}</span>${d.author ? `<span>by ${esc(d.author)}</span>` : ''}<span>${pm(d.additions, d.deletions)}</span><span>${num(d.changedFiles)} file${d.changedFiles === 1 ? '' : 's'}</span>${d.comments ? `<span>${d.comments} comment${d.comments === 1 ? '' : 's'}</span>` : ''}${REVIEW[d.review] ? `<span class="rv ${d.review}">${REVIEW[d.review]}</span>` : ''}</div>`
    + `<button class="pbtn" data-ext="${esc(d.url)}">${ICONS.ext}Open on GitHub</button></div>${tally}`
    + (d.body?.trim() ? `<h4 class="sh">Description</h4><div class="md prb">${md(d.body)}</div>` : '');
}

/// The GitHub CLI, one click: install it, then sign in with GitHub's device code.
export function ghSetupHTML(gh, need) {
  const g = gh || {}, busy = g.busy, step = g.step;
  const installed = g.checked ? g.installed : need !== 'install';
  const title = !installed ? 'Set up the GitHub CLI' : 'Sign in to GitHub';
  const text = !installed ? 'Hover reads and opens pull requests through GitHub’s own command-line tool (gh). One click installs it and signs you in.'
    : 'gh is installed. Sign in once with your browser, and Hover can show this branch’s pull request and open new ones.';
  const code = g.code ? `<div class="ghcode"><span>Your one-time code</span><b>${esc(g.code)}</b><div><button class="pbtn" data-ghcopy="${esc(g.code)}">${ICONS.copy}Copy code</button><button class="pbtn pri" data-ext="${esc(g.url || 'https://github.com/login/device')}">${ICONS.ext}Open github.com/login/device</button></div></div>` : '';
  const progress = busy ? `<p class="ghline"><i class="spin"></i>${esc(g.line || (step === 'installing' ? 'Installing…' : 'Signing in…'))}</p>` : '';
  const err = g.error && !busy ? `<p class="gherr">${esc(g.error)}</p>` : '';
  const btn = busy ? '<button class="pbtn" data-gh="cancel">Cancel</button>'
    : `<button class="pbtn pri" data-gh="setup">${ICONS.github}${!installed ? 'Install and sign in' : 'Sign in with GitHub'}</button>`;
  return `<div class="ghs"><div class="ghh">${ICONS.github}<div><b>${esc(title)}</b><span>${esc(text)}</span></div></div>${code}${progress}${err}<div class="ghb">${btn}${g.user ? `<span>Signed in as ${esc(g.user)}</span>` : ''}</div></div>`;
}

/// No pull request for this branch yet: open one, from the session's title and answer.
export function prCreateHTML(c, ui) {
  const f = ui.prForm ||= { title: c.title || '', body: c.body || '', branch: c.onDefault ? c.suggest || '' : '', draft: false, commit: c.changed > 0, base: c.base || 'main' };
  const busy = ui.prBusy, res = ui.prResult;
  const what = [c.onDefault ? `A new branch from <b>${esc(c.branch || c.base)}</b>` : `Branch <b>${esc(c.branch)}</b>${c.ahead ? `, ${num(c.ahead)} commit${c.ahead === 1 ? '' : 's'} ahead of ${esc(c.base)}` : ''}`,
    c.changed ? `${num(c.changed)} file${c.changed === 1 ? '' : 's'} not committed yet` : ''].filter(Boolean).join(' · ');
  const done = res?.ok ? `<div class="prok">${ICONS.check}<span>Pull request opened.</span>${res.url ? `<button class="pbtn" data-ext="${esc(res.url)}">${ICONS.ext}Open on GitHub</button>` : ''}</div>` : '';
  const fail = res?.error ? `<p class="gherr">${esc(res.error)}${res.steps?.length ? `<br><span>Done before it: ${esc(res.steps.join(', '))}.</span>` : ''}</p>` : '';
  return `<form class="prf" data-prform>${done}<div class="prt"><span class="prs dr">${ICONS.pr}No pull request yet</span></div><p class="prw">${what}</p>`
    + `<label><span>Title</span><input name="title" required maxlength="256" value="${esc(f.title)}"></label>`
    + `<label><span>Description</span><textarea name="body" rows="6">${esc(f.body)}</textarea></label>`
    + `<div class="prrow">${c.onDefault ? `<label><span>New branch</span><input name="branch" spellcheck="false" value="${esc(f.branch)}" placeholder="hover/my-change"></label>` : ''}<label><span>Into</span><input name="base" spellcheck="false" value="${esc(f.base)}"></label></div>`
    + (c.changed ? `<label class="prck"><input type="checkbox" name="commit"${f.commit ? ' checked' : ''}><span>Commit the ${num(c.changed)} changed file${c.changed === 1 ? '' : 's'} first</span></label>` : '')
    + `<label class="prck"><input type="checkbox" name="draft"${f.draft ? ' checked' : ''}><span>Open as a draft</span></label>`
    + (c.busy ? '<p class="note">The agent is still working in this folder; open it when the run ends.</p>' : '')
    + `${fail}<div class="ghb"><button class="pbtn pri" type="submit"${busy || c.busy ? ' disabled' : ''}>${busy ? '<i class="spin"></i>Opening…' : `${ICONS.pr}Create pull request`}</button><span>Pushes the branch, then opens it with gh.</span></div></form>`;
}

export function linkedHTML(d) {
  if (!d) return loading();
  if (failed(d)) return failed(d);
  if (!d.prs?.length) return empty('linked', 'No linked pull requests', 'Pull requests this session mentions show here.');
  return (d.gh ? '' : '<p class="note">Set up the GitHub CLI in the Pull request tab to see their state.</p>') + d.prs.map(p => `<button class="lrow" data-ext="${esc(p.url)}" title="${esc(p.url)}">`
    + `${p.state ? prPill(p) : `<span class="prs">${ICONS.linked}</span>`}<span class="lt"><b>${esc(p.title || `${p.repo}#${p.number}`)}</b><i>${esc(p.repo)}#${p.number}${p.head ? ` · ${esc(p.head)}` : ''}${p.error ? ` · ${esc(p.error)}` : ''}</i></span><span class="fr">${pm(p.additions, p.deletions)}</span>${ICONS.ext}</button>`).join('');
}

// ── Subagents ───────────────────────────────────────────────────────────
export function agentsHTML(d, ui) {
  if (!d) return loading();
  if (failed(d)) return failed(d);
  if (!d.agents?.length) return empty('agents', 'No subagents', 'Work this session hands to subagents shows here.');
  const word = { in_progress: 'Running', completed: 'Done', failed: 'Failed' };
  return `<div class="dsum"><b>${d.agents.length} subagent${d.agents.length === 1 ? '' : 's'}</b>${d.running ? `<span class="live">${d.running} running</span>` : ''}</div>` + d.agents.slice().reverse().map(a => {
    const open = ui.agentOpen.has(a.id), st = a.status === 'in_progress' ? 'run' : a.status === 'failed' ? 'bad' : 'ok';
    return `<div class="sag ${st}${open ? ' open' : ''}"><button class="sah" data-sa="${esc(a.id)}" aria-expanded="${open}"><i class="dot"></i><span class="lt"><b>${esc(a.name)}</b><i>${esc(a.task)}</i></span><span class="fr">${word[a.status] || a.status}${a.ms ? ` · ${dur(a.ms)}` : ''}</span>${ICONS.chevron}</button>`
      + (open ? `${a.prompt ? `<div class="sab"><h5>Task</h5><pre>${esc(a.prompt)}</pre></div>` : ''}<div class="sab"><h5>Result</h5>${a.out ? `<pre>${esc(a.out)}</pre>` : `<p class="none">${a.status === 'in_progress' ? 'Still working…' : 'Nothing came back.'}</p>`}</div>` : '') + '</div>';
  }).join('');
}

// ── Browser ─────────────────────────────────────────────────────────────
const KIND = { server: 'Local server', fetch: 'Fetched', opened: 'Opened', screen: 'On screen', web: 'In Hover’s browser' };
const label = u => { try { const x = new URL(u); return x.host + (x.pathname === '/' ? '' : x.pathname); } catch { return u; } };

/// What the address box takes: a full http(s) URL, or "localhost:3000" and the like.
export function normalizeUrl(text) {
  let t = String(text || '').trim(); if (!t) return null;
  if (!/^[a-z][a-z0-9+.-]*:/i.test(t)) t = (/^(localhost|127\.|0\.0\.0\.0|\[::1\])/i.test(t) ? 'http://' : 'https://') + t;
  try { const u = new URL(t); return u.protocol === 'http:' || u.protocol === 'https:' ? u.href : null; } catch { return null; }
}

/// The bar over the page: its address, reload and open-in-browser, and the pages the
/// agent opened. The page itself is an iframe main.js keeps apart, so a redraw of
/// the bar never reloads it.
export function browserBarHTML(d, url, bot) {
  const pages = d?.pages || [];
  return `<form class="baddr" data-baddr><span class="bi">${url && /^http:\/\/(localhost|127\.)/.test(url) ? ICONS.server : ICONS.browser}</span><input id="bAddr" type="text" spellcheck="false" autocomplete="off" placeholder="localhost:3000 or a web address" aria-label="Address" value="${esc(url || '')}">`
    + `<button type="button" class="x" data-breload aria-label="Reload" title="Reload"${url ? '' : ' disabled'}>${ICONS.reload}</button><button type="button" class="x" data-ext="${esc(url || '')}" aria-label="Open in your browser" title="Open in your browser"${url ? '' : ' disabled'}>${ICONS.ext}</button></form>`
    + (pages.length ? `<div class="bpages" role="list" aria-label="Pages ${esc(bot)} opened">${pages.map(p => `<button role="listitem" class="bp${p.url === url ? ' on' : ''}" data-url="${esc(p.url)}" title="${esc(KIND[p.kind] || '')}: ${esc(p.url)}">${p.kind === 'server' ? ICONS.server : ICONS.browser}<span>${esc(p.title && p.kind === 'fetch' ? p.title : label(p.url))}</span></button>`).join('')}</div>` : '');
}

/// The bar over Hover's own browser (the Mac's): back, forward, the address with the
/// page's title, reload or stop, and open in the user's browser.
export function nativeBarHTML(d, tab, url, bot, browsing) {
  const pages = d?.pages || [], u = tab?.url || url || '';
  const local = /^http:\/\/(localhost|127\.)/.test(u);
  return `<form class="baddr" data-baddr><button type="button" class="x" data-bnav="back" aria-label="Back" title="Back"${tab?.canBack ? '' : ' disabled'}>${ICONS.back}</button><button type="button" class="x" data-bnav="forward" aria-label="Forward" title="Forward"${tab?.canForward ? '' : ' disabled'}>${ICONS.fwd}</button>`
    + `<span class="bi">${tab?.loading ? '<i class="spin"></i>' : local ? ICONS.server : ICONS.browser}</span><input id="bAddr" type="text" spellcheck="false" autocomplete="off" placeholder="localhost:3000 or a web address" aria-label="Address" value="${esc(u)}">`
    + `<button type="button" class="x" data-bnav="${tab?.loading ? 'stop' : 'reload'}" aria-label="${tab?.loading ? 'Stop' : 'Reload'}" title="${tab?.loading ? 'Stop' : 'Reload'}"${u ? '' : ' disabled'}>${tab?.loading ? '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"/></svg>' : ICONS.reload}</button><button type="button" class="x" data-ext="${esc(u)}" aria-label="Open in your browser" title="Open in your browser"${u ? '' : ' disabled'}>${ICONS.ext}</button></form>`
    + (browsing ? `<div class="bnow"><i></i>${esc(bot)} is using this browser${tab?.title ? ` · ${esc(tab.title)}` : ''}</div>` : tab?.error ? `<div class="bnow bad">${esc(tab.error)}</div>` : '')
    + (pages.length ? `<div class="bpages" role="list" aria-label="Pages ${esc(bot)} opened">${pages.map(p => `<button role="listitem" class="bp${p.url === u ? ' on' : ''}" data-url="${esc(p.url)}" title="${esc(KIND[p.kind] || '')}: ${esc(p.url)}">${p.kind === 'server' ? ICONS.server : ICONS.browser}<span>${esc(p.title && p.kind === 'fetch' ? p.title : label(p.url))}</span></button>`).join('')}</div>` : '');
}

export function browserEmptyHTML(bot) {
  return empty('browser', 'Nothing open', `Pages ${bot} opens, and the local servers it starts, show here. Type an address above to look at one.`);
}

export const KINDS = KIND;
export const pageLabel = label;

// ── Screen ──────────────────────────────────────────────────────────────
export function screenHTML(sc, bot, testing, mac, apps) {
  const live = sc.live, denied = mac && sc.access === false;
  const theirs = apps ? `the apps ${bot} opened` : `no apps yet: ${bot} hasn’t opened any`;
  // Without Screen Recording it can't go live, so it says so rather than wait. Your own
  // windows never show: only the desktop and the apps the agent opened.
  const note = live ? (testing ? `${bot} is testing with computer use, live: your desktop with ${theirs}. Your own windows aren’t shown.` : `Live: your desktop with ${theirs}. Your own windows aren’t shown.`)
    : testing ? (denied ? `${bot} is using the computer now. Allow Screen Recording above to watch it live.` : 'Going live…')
    : `Your desktop with ${theirs}, never your own windows. It goes live while ${bot} uses computer use.`;
  const ask = denied ? `<div class="sacc"><b>Hover needs Screen Recording to show the screen</b><span>Allow Hover in System Settings → Privacy &amp; Security → Screen Recording, then quit and reopen Hover.</span><button class="pbtn" data-saccess>Allow…</button></div>` : '';
  return `<div class="scr"><div class="sbar"><span class="sl${live ? ' on' : ''}"><i></i>${live ? 'Live' : 'Desktop'}</span><span class="sp"></span>`
    + `<button class="pbtn sm" data-watch aria-pressed="${!!sc.watch}">${sc.watch ? 'Stop watching' : 'Watch live'}</button></div>${ask}`
    + `<div class="sframe${sc.image ? '' : ' none'}"><img id="scrImg" alt="${live ? 'The screen, live' : 'The desktop'}"${sc.image ? ` src="${sc.image}"` : ''}>${sc.image ? '' : '<span><i class="spin"></i></span>'}</div><p class="snote">${esc(note)}</p></div>`;
}
