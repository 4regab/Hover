import AppKit
import WebKit

/// Hover's built-in browser on a Mac: one WKWebView per session, which its agent drives
/// through Hover's MCP server (BrowserTool in the backend) and the user sees in the
/// desk's Browser panel. It stays out of the user's way: while nobody watches it has no
/// window at all (it lays out at 1280×800 and draws only for snapshots); while the
/// Browser panel shows it, it sits over the panel's page area inside the office, as
/// T3 Code's preview sits in its panel. Its own non-persistent data store: none of the
/// user's cookies or sign-ins, and nothing kept after Hover quits. http and https only.
/// Called on the main thread only (the app delegate's, like everything else here);
/// each entry point says so to the compiler with MainActor.assumeIsolated.
final class AgentBrowsers {
    /// Answers to the backend ({type:'browserResult', ...}).
    var reply: (([String: Any]) -> Void)?
    /// State for the offices ({type:'agentBrowser', ...}).
    var deliver: (([String: Any]) -> Void)?
    private var tabs: [Int: AgentTab] = [:]

    @MainActor private func tab(_ id: Int) -> AgentTab {
        if let t = tabs[id] { return t }
        let t = AgentTab(id: id)
        t.changed = { [weak self, weak t] in if let t { self?.deliver?(t.state) } }
        tabs[id] = t
        return t
    }

    /// A tool call from an agent ({type:'browser', call, id, op, args}).
    func handle(_ m: [String: Any]) {
        guard let call = (m["call"] as? NSNumber)?.int64Value, let id = (m["id"] as? NSNumber)?.intValue, let op = m["op"] as? String else { return }
        let args = m["args"] as? [String: Any] ?? [:]
        MainActor.assumeIsolated {
            let t = tab(id)
            Task { @MainActor in
                var out: [String: Any] = ["type": "browserResult", "call": call]
                do {
                    let r = try await t.run(op, args)
                    out["ok"] = true; out["text"] = r.text
                    if let image = r.image { out["image"] = image; out["mime"] = "image/jpeg" }
                } catch {
                    out["ok"] = false; out["text"] = (error as? BrowserError)?.message ?? error.localizedDescription
                }
                self.reply?(out)
            }
        }
    }

    /// The user typed an address in the panel, or picked a page.
    func go(_ id: Int, _ text: String) {
        MainActor.assumeIsolated {
            guard let url = AgentTab.normalize(text) else { return }
            tab(id).load(url)
        }
    }

    func nav(_ id: Int, _ what: String) {
        MainActor.assumeIsolated {
            guard let t = tabs[id] else { return }
            switch what {
            case "back": t.web.goBack()
            case "forward": t.web.goForward()
            case "stop": t.web.stopLoading()
            default: t.web.reload()
            }
        }
    }

    /// The panel's page area in the office (CSS pixels from its top left), or nil to
    /// take the browser out of it.
    func view(_ id: Int, rect: CGRect?, in office: WKWebView) {
        MainActor.assumeIsolated {
            // One browser shows at a time in an office; the others go back to no window.
            for (k, t) in tabs where k != id && t.web.superview === office { t.park() }
            guard let rect, rect.width > 20, rect.height > 20 else { tabs[id]?.park(); return }
            let t = tab(id)
            if t.web.superview !== office { t.web.removeFromSuperview(); office.addSubview(t.web) }
            let y = office.isFlipped ? rect.minY : office.bounds.height - rect.maxY
            t.web.frame = CGRect(x: rect.minX, y: y, width: rect.width, height: rect.height).integral
            deliver?(t.state)
        }
    }

    /// The office folded or closed: whatever it showed goes back to no window.
    func detach(from office: WKWebView) {
        MainActor.assumeIsolated { for t in tabs.values where t.web.superview === office { t.park() } }
    }

    /// Sessions that are gone take their browsers with them.
    func keep(_ ids: Set<Int>) {
        MainActor.assumeIsolated { for (k, t) in tabs where !ids.contains(k) { t.close(); tabs[k] = nil } }
    }

    /// Each tab as the page shows it, for an office that just loaded.
    func states() -> [[String: Any]] { MainActor.assumeIsolated { tabs.values.map(\.state) } }
}

struct BrowserError: Error { let message: String }

@MainActor
final class AgentTab: NSObject, WKNavigationDelegate, WKUIDelegate {
    let id: Int
    let web: WKWebView
    var changed: (() -> Void)?
    private var waiters: [CheckedContinuation<Void, Never>] = []
    private var status: Int?
    private var failure: String?
    private var watches: [NSKeyValueObservation] = []
    static let size = CGSize(width: 1280, height: 800)

    init(id: Int) {
        self.id = id
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.applicationNameForUserAgent = "Hover"
        config.userContentController.addUserScript(WKUserScript(source: AgentTab.consoleHook, injectionTime: .atDocumentStart, forMainFrameOnly: true))
        web = WKWebView(frame: CGRect(origin: .zero, size: AgentTab.size), configuration: config)
        super.init()
        web.navigationDelegate = self
        web.uiDelegate = self
        web.allowsBackForwardNavigationGestures = true
        watches = [
            web.observe(\.url) { [weak self] _, _ in Task { @MainActor in self?.changed?() } },
            web.observe(\.title) { [weak self] _, _ in Task { @MainActor in self?.changed?() } },
            web.observe(\.isLoading) { [weak self] _, _ in Task { @MainActor in self?.changed?() } },
        ]
    }

    var state: [String: Any] {
        ["type": "agentBrowser", "id": id, "url": web.url?.absoluteString ?? "", "title": web.title ?? "", "loading": web.isLoading,
         "canBack": web.canGoBack, "canForward": web.canGoForward, "error": (failure as Any?) ?? NSNull(), "shown": web.superview != nil]
    }

    func park() {
        web.removeFromSuperview()
        web.frame = CGRect(origin: .zero, size: AgentTab.size)
        changed?()
    }

    func close() { web.stopLoading(); web.removeFromSuperview(); watches.removeAll(); resume() }

    /// What an address box takes: http(s) URLs, and "localhost:3000" and the like.
    static func normalize(_ text: String) -> URL? {
        var t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return nil }
        if t.range(of: "^[a-zA-Z][a-zA-Z0-9+.-]*://", options: .regularExpression) == nil {
            let local = t.range(of: "^(localhost|127\\.|0\\.0\\.0\\.0|\\[::1\\])", options: [.regularExpression, .caseInsensitive]) != nil
            t = (local ? "http://" : "https://") + t
        }
        guard let url = URL(string: t), let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https", url.host != nil else { return nil }
        return url
    }

    func load(_ url: URL) {
        failure = nil; status = nil
        web.load(URLRequest(url: url))
    }

    // MARK: Waiting for a page

    private func resume() { let w = waiters; waiters.removeAll(); for c in w { c.resume() } }

    /// Until the page has loaded (or failed), at most `seconds`.
    private func settle(_ seconds: Double) async {
        if !web.isLoading { return }
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            waiters.append(c)
            Task { @MainActor [weak self] in
                try? await Task.sleep(nanoseconds: UInt64(seconds * 1e9))
                self?.resume()
            }
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { failure = nil; resume(); changed?() }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { fail(error) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { fail(error) }
    private func fail(_ error: Error) {
        let e = error as NSError
        // A new navigation replacing this one isn't a failure.
        if e.domain == NSURLErrorDomain && e.code == NSURLErrorCancelled { return }
        failure = e.localizedDescription; resume(); changed?()
    }
    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) { failure = "The page crashed."; resume(); changed?() }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let scheme = action.request.url?.scheme?.lowercased() ?? ""
        decisionHandler(["http", "https", "about", "data", "blob"].contains(scheme) ? .allow : .cancel)
    }

    func webView(_ webView: WKWebView, decidePolicyFor response: WKNavigationResponse, decisionHandler: @escaping (WKNavigationResponsePolicy) -> Void) {
        if response.isForMainFrame { status = (response.response as? HTTPURLResponse)?.statusCode }
        decisionHandler(.allow)
    }

    /// A link that opens a new window opens here instead.
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if let url = action.request.url, ["http", "https"].contains(url.scheme?.lowercased() ?? "") { webView.load(URLRequest(url: url)) }
        return nil
    }

    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        failure = nil; completionHandler()
    }
    func webView(_ webView: WKWebView, runJavaScriptConfirmPanelWithMessage message: String, initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping (Bool) -> Void) { completionHandler(true) }

    // MARK: The agent's tools

    struct Answer { var text: String; var image: String? = nil }

    private var here: String {
        let title = (web.title ?? "").isEmpty ? "(no title)" : web.title!
        return "\(title) — \(web.url?.absoluteString ?? "about:blank")"
    }

    func run(_ op: String, _ a: [String: Any]) async throws -> Answer {
        switch op {
        case "open":
            guard let text = a["url"] as? String, let url = AgentTab.normalize(text) else { throw BrowserError(message: "Give an http(s) address, like localhost:3000 or https://example.com.") }
            load(url)
            try? await Task.sleep(nanoseconds: 50_000_000)
            await settle(30)
            if let failure { throw BrowserError(message: "Couldn’t open \(url.absoluteString): \(failure)") }
            // The title comes a moment after the load ends.
            for _ in 0..<10 where (web.title ?? "").isEmpty { try? await Task.sleep(nanoseconds: 100_000_000) }
            let code = status.map { " (HTTP \($0))" } ?? ""
            return Answer(text: "Opened \(here)\(code). \(web.isLoading ? "It is still loading. " : "")Use browser_snapshot to read it.")
        case "back":
            guard web.canGoBack else { throw BrowserError(message: "There is no page to go back to.") }
            web.goBack(); try? await Task.sleep(nanoseconds: 100_000_000); await settle(15)
            return Answer(text: "Back at \(here).")
        case "reload":
            web.reload(); try? await Task.sleep(nanoseconds: 100_000_000); await settle(30)
            if let failure { throw BrowserError(message: "The page didn’t load: \(failure)") }
            return Answer(text: "Reloaded \(here).")
        case "screenshot":
            guard web.url != nil else { throw BrowserError(message: "Nothing is open. Use browser_open first.") }
            await settle(10)
            let config = WKSnapshotConfiguration()
            config.snapshotWidth = NSNumber(value: Double(min(1280, web.bounds.width)))
            let image = try await web.takeSnapshot(configuration: config)
            // At one pixel per point: a Retina snapshot is four times the pixels for the
            // model to read, and no more to see.
            let w = Int(image.size.width), h = Int(image.size.height)
            guard w > 0, h > 0, let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
            else { throw BrowserError(message: "The screenshot couldn’t be made.") }
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
            image.draw(in: CGRect(x: 0, y: 0, width: w, height: h))
            NSGraphicsContext.restoreGraphicsState()
            guard let jpeg = rep.representation(using: .jpeg, properties: [.compressionFactor: 0.72]) else { throw BrowserError(message: "The screenshot couldn’t be made.") }
            return Answer(text: "Screenshot of \(here), \(w)×\(h).", image: jpeg.base64EncodedString())
        default:
            guard web.url != nil else { throw BrowserError(message: "Nothing is open. Use browser_open first.") }
            guard let body = op == "evaluate" ? "" : AgentTab.scripts[op] else { throw BrowserError(message: "Unknown browser tool \(op).") }
            if op != "evaluate" { await settle(10) }
            let before = web.url
            // A script runs as given, as a function body (WebKit runs it, so the page's
            // CSP doesn't stop it); one expression without return is returned.
            let script = (a["script"] as? String ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            let code = op != "evaluate" ? body : script.contains("return") || script.contains(";") || script.contains("\n") ? script : "return (\(script))"
            let value: Any?
            do { value = try await web.callAsyncJavaScript(code, arguments: op == "evaluate" ? [:] : ["a": a], in: nil, contentWorld: .page) }
            catch { throw BrowserError(message: AgentTab.jsError(error)) }
            var text = op == "evaluate" ? AgentTab.json(value) : (value as? String) ?? "Done."
            // A click or a submit may start a page load: wait for it, and say where it went.
            if ["click", "type", "press"].contains(op) {
                try? await Task.sleep(nanoseconds: 350_000_000)
                await settle(15)
                if web.url != before { text += " Now at \(here)." }
            }
            return Answer(text: text)
        }
    }

    static func jsError(_ error: Error) -> String {
        let e = error as NSError
        if let m = e.userInfo["WKJavaScriptExceptionMessage"] as? String { return m.replacingOccurrences(of: "Error: ", with: "") }
        return e.localizedDescription
    }

    // MARK: The page's side

    /// Keeps the page's console and errors for browser_console (in the page's world, so
    /// its own console calls are seen).
    static let consoleHook = """
    (() => { if (window.__hoverConsole) return; const buf = window.__hoverConsole = [];
      const put = (level, args) => { try { buf.push({ level, text: args.map(a => { try { return typeof a === 'string' ? a : a instanceof Error ? (a.stack || a.message) : JSON.stringify(a); } catch { return String(a); } }).join(' ').slice(0, 2000), t: Date.now() }); if (buf.length > 300) buf.splice(0, buf.length - 300); } catch {} };
      for (const level of ['log', 'info', 'warn', 'error', 'debug']) { const orig = console[level]; console[level] = function (...a) { put(level, a); return orig.apply(this, a); }; }
      addEventListener('error', e => put('error', [e.message + (e.filename ? ` (${e.filename}:${e.lineno})` : '')]));
      addEventListener('unhandledrejection', e => put('error', ['Unhandled rejection: ' + (e.reason && (e.reason.stack || e.reason.message) || e.reason)]));
    })();
    """

    /// Shared by the tools: finding an element by its ref, a selector or its text.
    static let find = """
    const visible = el => { const r = el.getBoundingClientRect(), s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
    const field = el => /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
    const nameOf = el => (el.getAttribute('aria-label') || (field(el) ? (el.labels?.[0]?.innerText || el.getAttribute('placeholder') || el.name) : el.innerText) || el.value || el.title || el.getAttribute('alt') || el.name || '').replace(/\\s+/g, ' ').trim();
    const ACT = 'a[href],button,input,select,textarea,summary,label,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[role=switch],[role=textbox],[role=combobox],[contenteditable=""],[contenteditable=true],[onclick],[tabindex]:not([tabindex="-1"])';
    const find = (a, fields) => {
      let el = null;
      if (a.ref != null) { el = document.querySelector(`[data-hover-ref="${Number(a.ref)}"]`); if (!el) throw new Error(`No element [${a.ref}] on the page now. Take a new browser_snapshot.`); return el; }
      if (a.selector) { try { el = document.querySelector(a.selector); } catch { throw new Error('That selector isn’t valid.'); } if (!el) throw new Error(`Nothing matches ${a.selector}.`); return el; }
      const t = String(a[fields] || '').trim().toLowerCase();
      if (!t) throw new Error('Name the element: a ref from browser_snapshot, a selector, or its text.');
      const all = [...document.querySelectorAll(ACT)].filter(visible);
      // A field asked for by its label is the field, not the label around it.
      const pool = fields === 'label' ? all.filter(e => field(e) || e.isContentEditable || e.tagName === 'LABEL') : all;
      el = pool.find(e => nameOf(e).toLowerCase() === t) || pool.find(e => nameOf(e).toLowerCase().includes(t));
      if (!el) throw new Error(`Nothing on the page is called “${a[fields]}”.`);
      if (el.tagName === 'LABEL') el = el.control || el.querySelector('input,textarea,select') || el;
      return el;
    };
    const describe = el => { const r = el.getAttribute('data-hover-ref'); const n = nameOf(el).slice(0, 60); return `${r ? `[${r}] ` : ''}${(el.getAttribute('role') || el.tagName).toLowerCase()}${n ? ` “${n}”` : ''}`; };
    """

    /// A script's result as JSON text, cut at 20,000 characters.
    static func json(_ value: Any?) -> String {
        guard let value, !(value is NSNull) else { return "undefined" }
        var text: String
        if JSONSerialization.isValidJSONObject(value), let data = try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys]) { text = String(decoding: data, as: UTF8.self) }
        else if let s = value as? String { text = "\"\(s)\"" }
        else { text = "\(value)" }
        return text.count > 20000 ? String(text.prefix(20000)) + "…" : text
    }

    static let scripts: [String: String] = [
        "snapshot": find + """
        const max = Math.min(Math.max(Number(a.max_chars) || 12000, 2000), 40000);
        document.querySelectorAll('[data-hover-ref]').forEach(e => e.removeAttribute('data-hover-ref'));
        let n = 0, out = '', more = false;
        const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'SVG', 'IFRAME', 'CANVAS', 'VIDEO', 'AUDIO']);
        const BLOCK = /^(P|DIV|SECTION|ARTICLE|MAIN|HEADER|FOOTER|NAV|ASIDE|LI|UL|OL|TR|TABLE|FORM|FIELDSET|DL|DT|DD|BLOCKQUOTE|PRE|FIGURE|H[1-6]|BR|HR)$/;
        const put = s => { if (out.length >= max) { more = true; return; } out += s; };
        const isAct = el => el.matches(ACT) && !(el.tagName === 'LABEL' && el.control);
        const walk = node => {
          if (more) return;
          if (node.nodeType === 3) { const t = node.textContent.replace(/\\s+/g, ' '); if (t.trim()) put(t); return; }
          if (node.nodeType !== 1) return;
          const el = node;
          if (SKIP.has(el.tagName.toUpperCase()) || el.getAttribute('aria-hidden') === 'true' || !visible(el) && !el.matches('input[type=hidden]') && el.tagName !== 'BODY') { if (el.tagName === 'IFRAME') put(' [frame] '); return; }
          if (el.matches('input[type=hidden]')) return;
          if (/^H[1-6]$/.test(el.tagName)) { put('\\n' + '#'.repeat(+el.tagName[1]) + ' '); }
          else if (BLOCK.test(el.tagName)) put('\\n');
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
          if (BLOCK.test(el.tagName)) put('\\n');
        };
        walk(document.body || document.documentElement);
        const text = out.replace(/[ \\t]+\\n/g, '\\n').replace(/\\n{3,}/g, '\\n\\n').replace(/ {2,}/g, ' ').trim();
        return `Title: ${document.title}\\nURL: ${location.href}\\n${n} interactive elements, with [ref] numbers for browser_click and browser_type.\\n\\n${text}${more ? '\\n\\n(The page goes on; scroll, or ask for more with max_chars.)' : ''}`;
        """,
        "click": find + """
        const el = find(a, 'text');
        el.scrollIntoView({ block: 'center', inline: 'center' });
        const r = el.getBoundingClientRect(), x = r.left + r.width / 2, y = r.top + r.height / 2;
        const o = { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, button: 0, view: window };
        el.dispatchEvent(new PointerEvent('pointerdown', o)); el.dispatchEvent(new MouseEvent('mousedown', o));
        if (el.focus) el.focus({ preventScroll: true });
        el.dispatchEvent(new PointerEvent('pointerup', o)); el.dispatchEvent(new MouseEvent('mouseup', o));
        el.click();
        return `Clicked ${describe(el)}.`;
        """,
        "type": find + """
        if (a.text == null) throw new Error('Say what to type.');
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
        return said;
        """,
        "press": find + """
        const key = String(a.key || ''); if (!key) throw new Error('Say which key.');
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
        return `Pressed ${key} in ${el === document.body ? 'the page' : describe(el)}.`;
        """,
        "scroll": find + """
        if (a.ref != null) { const el = find(a, 'text'); el.scrollIntoView({ block: 'center' }); return `Scrolled to ${describe(el)}.`; }
        if (a.to === 'top') scrollTo(0, 0); else if (a.to === 'bottom') scrollTo(0, document.documentElement.scrollHeight); else scrollBy(0, Number(a.dy) || 600);
        await new Promise(r => setTimeout(r, 120));
        const h = document.documentElement.scrollHeight, y = Math.round(scrollY);
        return `Scrolled to ${y} of ${Math.max(0, h - innerHeight)} px.`;
        """,
        "wait": find + """
        const until = Date.now() + Math.min(Math.max(Number(a.timeout_ms) || 5000, 100), 20000);
        const seen = () => a.selector ? document.querySelector(a.selector) : a.text ? (document.body?.innerText || '').toLowerCase().includes(String(a.text).toLowerCase()) : document.readyState === 'complete';
        while (Date.now() < until) { if (seen()) return a.selector ? `${a.selector} is on the page.` : a.text ? `“${a.text}” is on the page.` : 'The page has loaded.'; await new Promise(r => setTimeout(r, 120)); }
        throw new Error(a.selector ? `${a.selector} didn’t appear in time.` : a.text ? `“${a.text}” didn’t appear in time.` : 'The page didn’t finish loading in time.');
        """,
        "console": """
        const buf = window.__hoverConsole || [];
        const lines = buf.slice(-100).map(e => `${new Date(e.t).toISOString().slice(11, 19)} ${e.level.toUpperCase()} ${e.text}`);
        if (a.clear) buf.length = 0;
        return lines.length ? lines.join('\\n') : 'The console is empty.';
        """,
    ]
}
