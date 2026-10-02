//! The JavaScript the agent browser runs in the page (AgentBrowser.swift's AgentTab:
//! consoleHook, find and scripts), unchanged: the same page-side code on every OS, so a tool
//! call does the same thing wherever it is answered. Each op's body is a function body: it
//! takes `a` (the tool's arguments, put in front by `browser_host::function_body`) and
//! returns the text to say.

/// Keeps the page's console and errors for browser_console (in the page's world, so its own
/// console calls are seen). Runs at document start.
pub const CONSOLE_HOOK: &str = r####"(() => { if (window.__hoverConsole) return; const buf = window.__hoverConsole = [];
  const put = (level, args) => { try { buf.push({ level, text: args.map(a => { try { return typeof a === 'string' ? a : a instanceof Error ? (a.stack || a.message) : JSON.stringify(a); } catch { return String(a); } }).join(' ').slice(0, 2000), t: Date.now() }); if (buf.length > 300) buf.splice(0, buf.length - 300); } catch {} };
  for (const level of ['log', 'info', 'warn', 'error', 'debug']) { const orig = console[level]; console[level] = function (...a) { put(level, a); return orig.apply(this, a); }; }
  addEventListener('error', e => put('error', [e.message + (e.filename ? ` (${e.filename}:${e.lineno})` : '')]));
  addEventListener('unhandledrejection', e => put('error', ['Unhandled rejection: ' + (e.reason && (e.reason.stack || e.reason.message) || e.reason)]));
})();"####;

/// Shared by the tools: finding an element by its ref, a selector or its text.
pub const FIND: &str = r####"const visible = el => { const r = el.getBoundingClientRect(), s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
const field = el => /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
const nameOf = el => (el.getAttribute('aria-label') || (field(el) ? (el.labels?.[0]?.innerText || el.getAttribute('placeholder') || el.name) : el.innerText) || el.value || el.title || el.getAttribute('alt') || el.name || '').replace(/\s+/g, ' ').trim();
const ACT = 'a[href],button,input,select,textarea,summary,label,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[role=switch],[role=textbox],[role=combobox],[contenteditable=""],[contenteditable=true],[onclick],[tabindex]:not([tabindex="-1"])';
const find = (a, fields) => {
  let el = null;
  if (a.ref != null) { el = document.querySelector(`[data-hover-ref="${Number(a.ref)}"]`); if (!el) throw new Error(`No element [${a.ref}] on the page now. Take a new browser_snapshot.`); return el; }
  if (a.selector) { try { el = document.querySelector(a.selector); } catch { throw new Error('That selector isn’t valid.'); } if (!el) throw new Error(`Nothing matches ${a.selector}.`); return el; }
  const t = String(a[fields] || '').trim().toLowerCase();
  if (!t) throw new Error('Name the element: a ref from browser_snapshot, a selector, or its text.');
  const all = [...document.querySelectorAll(ACT)].filter(visible);
  el = all.find(e => nameOf(e).toLowerCase() === t) || all.find(e => nameOf(e).toLowerCase().includes(t));
  if (!el) throw new Error(`Nothing on the page is called “${a[fields]}”.`);
  return el;
};
const describe = el => { const r = el.getAttribute('data-hover-ref'); const n = nameOf(el).slice(0, 60); return `${r ? `[${r}] ` : ''}${(el.getAttribute('role') || el.tagName).toLowerCase()}${n ? ` “${n}”` : ''}`; };"####;

pub const SNAPSHOT: &str = r####"const max = Math.min(Math.max(Number(a.max_chars) || 12000, 2000), 40000);
document.querySelectorAll('[data-hover-ref]').forEach(e => e.removeAttribute('data-hover-ref'));
let n = 0, out = '', more = false;
const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'SVG', 'IFRAME', 'CANVAS', 'VIDEO', 'AUDIO']);
const BLOCK = /^(P|DIV|SECTION|ARTICLE|MAIN|HEADER|FOOTER|NAV|ASIDE|LI|UL|OL|TR|TABLE|FORM|FIELDSET|DL|DT|DD|BLOCKQUOTE|PRE|FIGURE|H[1-6]|BR|HR)$/;
const put = s => { if (out.length >= max) { more = true; return; } out += s; };
const isAct = el => el.matches(ACT) && !(el.tagName === 'LABEL' && el.control);
const walk = node => {
  if (more) return;
  if (node.nodeType === 3) { const t = node.textContent.replace(/\s+/g, ' '); if (t.trim()) put(t); return; }
  if (node.nodeType !== 1) return;
  const el = node;
  if (SKIP.has(el.tagName.toUpperCase()) || el.getAttribute('aria-hidden') === 'true' || !visible(el) && !el.matches('input[type=hidden]') && el.tagName !== 'BODY') { if (el.tagName === 'IFRAME') put(' [frame] '); return; }
  if (el.matches('input[type=hidden]')) return;
  if (/^H[1-6]$/.test(el.tagName)) { put('\n' + '#'.repeat(+el.tagName[1]) + ' '); }
  else if (BLOCK.test(el.tagName)) put('\n');
  if (isAct(el)) {
    const ref = ++n; el.setAttribute('data-hover-ref', ref);
    const tag = el.tagName.toLowerCase(), role = el.getAttribute('role') || (tag === 'a' ? 'link' : tag === 'input' ? (el.type || 'text') : tag);
    let s = ` [${ref}] ${role}`; const nm = nameOf(el).slice(0, 80); if (nm && !(tag === 'input' && nm === el.value)) s += ` “${nm}”`;
    if ('value' in el && el.value && tag !== 'button' && el.type !== 'submit') s += ` = “${String(el.value).slice(0, 80)}”`;
    if (el.checked) s += ' (checked)'; if (el.disabled) s += ' (disabled)';
    if (tag === 'a' && el.getAttribute('href')) { const h = el.getAttribute('href'); if (!h.startsWith('javascript:')) s += ` → ${h.slice(0, 80)}`; }
    if (tag === 'select') s += ` options: ${[...el.options].slice(0, 12).map(o => o.text.trim()).join(' | ')}`;
    put(s + ' ');
    if (tag === 'input' || tag === 'textarea' || tag === 'select') return;
    if (nm && el.children.length === 0) return;
  }
  for (const c of el.childNodes) walk(c);
  if (el.shadowRoot) for (const c of el.shadowRoot.childNodes) walk(c);
  if (BLOCK.test(el.tagName)) put('\n');
};
walk(document.body || document.documentElement);
const text = out.replace(/[ \t]+\n/g, '\n').replace(/\n{3,}/g, '\n\n').replace(/ {2,}/g, ' ').trim();
return `Title: ${document.title}\nURL: ${location.href}\n${n} interactive elements, with [ref] numbers for browser_click and browser_type.\n\n${text}${more ? '\n\n(The page goes on; scroll, or ask for more with max_chars.)' : ''}`;"####;

pub const CLICK: &str = r####"const el = find(a, 'text');
el.scrollIntoView({ block: 'center', inline: 'center' });
const r = el.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
const o = { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, button: 0, view: window };
el.dispatchEvent(new PointerEvent('pointerdown', o)); el.dispatchEvent(new MouseEvent('mousedown', o));
if (el.focus) el.focus({ preventScroll: true });
el.dispatchEvent(new PointerEvent('pointerup', o)); el.dispatchEvent(new MouseEvent('mouseup', o));
el.click();
return `Clicked ${describe(el)}.`;"####;

pub const TYPE: &str = r####"if (a.text == null) throw new Error('Say what to type.');
const el = (a.ref != null || a.selector || a.label) ? find(a, 'label') : document.activeElement;
if (!el || el === document.body) throw new Error('Name the field to type in: a ref, a selector or its label.');
el.scrollIntoView({ block: 'center' }); if (el.focus) el.focus({ preventScroll: true });
const text = String(a.text);
if (el.isContentEditable) { if (!a.append) document.execCommand('selectAll'); document.execCommand('insertText', false, text); }
else if ('value' in el) {
  const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  const set = Object.getOwnPropertyDescriptor(proto, 'value').set;
  if (el.tagName === 'SELECT') { const o = [...el.options].find(o => o.text.trim().toLowerCase() === text.toLowerCase() || o.value === text); if (!o) throw new Error(`No option “${text}”.`); set.call(el, o.value); }
  else set.call(el, (a.append ? el.value : '') + text);
  el.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, data: text, inputType: 'insertText' }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
} else throw new Error(`${describe(el)} isn’t a text field.`);
let said = `Typed into ${describe(el)}.`;
if (a.submit) {
  const k = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true };
  const go = el.dispatchEvent(new KeyboardEvent('keydown', k)); el.dispatchEvent(new KeyboardEvent('keypress', k)); el.dispatchEvent(new KeyboardEvent('keyup', k));
  if (go && el.form) { el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit(); said += ' Submitted its form.'; } else said += ' Pressed Enter.';
}
return said;"####;

pub const PRESS: &str = r####"const key = String(a.key || ''); if (!key) throw new Error('Say which key.');
const el = document.activeElement || document.body;
const codes = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, ArrowDown: 40, ArrowUp: 38, ArrowLeft: 37, ArrowRight: 39, Space: 32, ' ': 32 };
const k = { key: key === 'Space' ? ' ' : key, code: key.length === 1 ? 'Key' + key.toUpperCase() : key, keyCode: codes[key] || key.toUpperCase().charCodeAt(0), bubbles: true, cancelable: true, composed: true };
const go = el.dispatchEvent(new KeyboardEvent('keydown', k));
if (key.length === 1) el.dispatchEvent(new KeyboardEvent('keypress', k));
el.dispatchEvent(new KeyboardEvent('keyup', k));
if (go && key === 'Enter' && el.form) { el.form.requestSubmit ? el.form.requestSubmit() : el.form.submit(); return `Pressed Enter in ${describe(el)}; its form was submitted.`; }
if (go && key === 'Tab') { const all = [...document.querySelectorAll(ACT)].filter(visible); const i = all.indexOf(el); const next = all[(i + 1) % all.length]; next?.focus(); return `Pressed Tab; ${next ? describe(next) : 'nothing'} has focus.`; }
if (go && key.length === 1 && 'value' in el) { const set = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value')?.set; set?.call(el, el.value + key); el.dispatchEvent(new InputEvent('input', { bubbles: true, data: key })); }
if (go && key === 'Backspace' && 'value' in el) { const set = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value')?.set; set?.call(el, el.value.slice(0, -1)); el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' })); }
return `Pressed ${key} in ${el === document.body ? 'the page' : describe(el)}.`;"####;

pub const SCROLL: &str = r####"if (a.ref != null) { const el = find(a, 'text'); el.scrollIntoView({ block: 'center' }); return `Scrolled to ${describe(el)}.`; }
if (a.to === 'top') scrollTo(0, 0); else if (a.to === 'bottom') scrollTo(0, document.documentElement.scrollHeight); else scrollBy(0, Number(a.dy) || 600);
await new Promise(r => setTimeout(r, 120));
const h = document.documentElement.scrollHeight, y = Math.round(scrollY);
return `Scrolled to ${y} of ${Math.max(0, h - innerHeight)} px.`;"####;

pub const WAIT: &str = r####"const until = Date.now() + Math.min(Math.max(Number(a.timeout_ms) || 5000, 100), 20000);
const seen = () => a.selector ? document.querySelector(a.selector) : a.text ? (document.body?.innerText || '').toLowerCase().includes(String(a.text).toLowerCase()) : document.readyState === 'complete';
while (Date.now() < until) { if (seen()) return a.selector ? `${a.selector} is on the page.` : a.text ? `“${a.text}” is on the page.` : 'The page has loaded.'; await new Promise(r => setTimeout(r, 120)); }
throw new Error(a.selector ? `${a.selector} didn’t appear in time.` : a.text ? `“${a.text}” didn’t appear in time.` : 'The page didn’t finish loading in time.');"####;

pub const CONSOLE: &str = r####"const buf = window.__hoverConsole || [];
const lines = buf.slice(-100).map(e => `${new Date(e.t).toISOString().slice(11, 19)} ${e.level.toUpperCase()} ${e.text}`);
if (a.clear) buf.length = 0;
return lines.length ? lines.join('\n') : 'The console is empty.';"####;
