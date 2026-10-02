//! Hover's agent browser on a Mac (macos/Sources/AgentBrowser.swift): one WKWebView per
//! session, which its agent drives through Hover's MCP server (`hover_agents::browser`)
//! and the user watches in the desk's Browser panel. It stays out of the user's way: while
//! nobody watches it has no window at all (it lays out at 1280 × 800 and draws only for
//! snapshots); while the Browser panel shows it, it sits over the panel's page area in the
//! office's window. Its own non-persistent data store: none of the user's cookies or
//! sign-ins, and nothing kept after Hover quits. http and https only.
//!
//! This is the host side: `install()` hands `hover_agents::browser` a `Host` whose calls
//! run here. A call comes from a thread of its own; everything WebKit is touched on the main
//! thread (`on_main`), and a slow page is waited for on the calling thread, so the UI never
//! waits on a page.
//!
//! Elsewhere there is no browser to hand out: `supported()` is false and `note()` says
//! "Agent browser needs macOS.", which is what Settings shows beside the switch.
//!
//! The page-side JavaScript (`mac::browser_js`), the address rules (`normalize`), the way
//! a script is wrapped (`function_body`) and the texts of the answers are plain code,
//! compiled and tested on every OS.

use crate::mac::browser_js as js;
use hover_core::json::Json;

/// What Settings shows beside the switch where there is no browser to hand out.
pub use hover_agents::browser::UNSUPPORTED as NOTE;

/// Only a Mac has a browser to drive.
pub fn supported() -> bool { hover_agents::browser::supported() }

/// Why the switch is off here, or none.
pub fn note() -> Option<&'static str> { hover_agents::browser::note() }

// MARK: Plain code

/// What an address box takes: http(s) URLs, and "localhost:3000" and the like
/// (AgentTab.normalize). None when it isn't an address.
pub fn normalize(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() { return None; }
    let has_scheme = t.split_once("://").is_some_and(|(s, _)| s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)));
    let full = if has_scheme {
        t.to_owned()
    } else {
        let lower = t.to_ascii_lowercase();
        let local = ["localhost", "127.", "0.0.0.0", "[::1]"].iter().any(|p| lower.starts_with(p));
        format!("{}://{t}", if local { "http" } else { "https" })
    };
    let (scheme, rest) = full.split_once("://")?;
    if !["http", "https"].contains(&scheme.to_ascii_lowercase().as_str()) { return None; }
    // A host, and a port if there is one: what is before the path, the query and the
    // fragment, without any user info.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    let (host, port) = if let Some(v6) = hostport.strip_prefix('[') {
        let (h, after) = v6.split_once(']')?;
        if !after.is_empty() && !after.starts_with(':') { return None; }
        (h, after.strip_prefix(':'))
    } else {
        match hostport.rsplit_once(':') { Some((h, p)) => (h, Some(p)), None => (hostport, None) }
    };
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()));
    if host.is_empty() || !port_ok || full.chars().any(|c| c.is_whitespace() || c.is_control()) { return None; }
    Some(full)
}

/// The arguments as a JavaScript literal: JSON, with the two line separators escaped (a
/// string may hold them in JSON but older engines end a line at them).
pub fn args_literal(args: &Json) -> String { args.compact().replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029") }

/// The page-side script of an op (its body, with the shared helpers in front where it uses
/// them), or None for an op that isn't one.
pub fn script_of(op: &str) -> Option<String> {
    let find = |body: &str| format!("{}\n{}", js::FIND, body);
    Some(match op {
        "snapshot" => find(js::SNAPSHOT),
        "click" => find(js::CLICK),
        "type" => find(js::TYPE),
        "press" => find(js::PRESS),
        "scroll" => find(js::SCROLL),
        "wait" => find(js::WAIT),
        "console" => js::CONSOLE.to_owned(),
        _ => return None,
    })
}

/// What the page is asked to run for an op: `a` is the tool's arguments; `evaluate` runs
/// the agent's own script as a function body (WebKit runs it, so the page's CSP doesn't
/// stop it), one expression without a `return` is returned, and its result comes back as
/// JSON text (`"quoted"` for a string, `undefined` for nothing), cut at 20,000 characters
/// on the Rust side.
pub fn function_body(op: &str, args: &Json) -> Option<String> {
    let a = format!("const a = {};\n", args_literal(args));
    if op == "evaluate" {
        let script = args.get("script").and_then(Json::as_str).unwrap_or("").trim().to_owned();
        let code = if script.contains("return") || script.contains(';') || script.contains('\n') { script } else { format!("return ({script})") };
        return Some(format!("{a}const __v = await (async () => {{\n{code}\n}})();\nif (__v === undefined) return 'undefined';\nif (typeof __v === 'string') return JSON.stringify(__v);\nreturn JSON.stringify(__v, null, 2) ?? String(__v);"));
    }
    Some(format!("{a}{}", script_of(op)?))
}

/// A script's result cut at 20,000 characters.
pub fn clip_result(text: &str) -> String {
    if text.chars().count() > 20_000 { format!("{}…", text.chars().take(20_000).collect::<String>()) } else { text.to_owned() }
}

/// "Title — url", as every answer names the page.
pub fn here(title: &str, url: Option<&str>) -> String {
    format!("{} — {}", if title.is_empty() { "(no title)" } else { title }, url.unwrap_or("about:blank"))
}

/// The answer to browser_open (AgentTab.run "open").
pub fn opened(here: &str, status: Option<i64>, loading: bool) -> String {
    format!("Opened {here}{}. {}Use browser_snapshot to read it.", status.map_or(String::new(), |s| format!(" (HTTP {s})")), if loading { "It is still loading. " } else { "" })
}

/// An op that needs a page open says so when there is none.
pub const NOTHING_OPEN: &str = "Nothing is open. Use browser_open first.";
pub const BAD_ADDRESS: &str = "Give an http(s) address, like localhost:3000 or https://example.com.";

/// The ops a tool call may name, in the MCP server's words without "browser_".
pub const OPS: [&str; 12] = ["open", "snapshot", "click", "type", "press", "scroll", "screenshot", "evaluate", "wait", "console", "back", "reload"];

/// A tab as the desk's Browser panel shows it (AgentTab.state).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabState {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub error: Option<String>,
    /// Whether it is showing in a window now.
    pub shown: bool,
}

// MARK: The Mac's

/// Hands `hover_agents::browser` the Mac's browser. Call once at start, on the main thread.
/// Nothing is installed elsewhere.
pub fn install() {
    #[cfg(target_os = "macos")]
    mac::install();
}

/// The Host is gone (Hover quits).
pub fn uninstall() { hover_agents::browser::clear_host(); }

#[cfg(target_os = "macos")]
pub use mac::{forget, go, nav, on_change, park, set_resolver, show, state, states};

/// Where there is no browser these do nothing (the panel isn't offered).
#[cfg(not(target_os = "macos"))]
pub fn on_change(_: impl Fn(&str) + Send + Sync + 'static) {}
#[cfg(not(target_os = "macos"))]
pub fn set_resolver(_: impl Fn() -> Option<String> + Send + Sync + 'static) {}
#[cfg(not(target_os = "macos"))]
pub fn state(_session: &str) -> Option<TabState> { None }
#[cfg(not(target_os = "macos"))]
pub fn states() -> Vec<(String, TabState)> { vec![] }
#[cfg(not(target_os = "macos"))]
pub fn go(_session: &str, _text: &str) {}
#[cfg(not(target_os = "macos"))]
pub fn nav(_session: &str, _what: &str) {}
#[cfg(not(target_os = "macos"))]
pub fn forget(_keep: &[String]) {}
#[cfg(not(target_os = "macos"))]
pub fn park(_session: &str) {}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use hover_agents::browser::{Host, Reply};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, ProtocolObject};
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSWindow};
    use objc2_foundation::{ns_string, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest};
    use objc2_web_kit::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// 1280 × 800: the size a page lays out at with no window to size it.
    const SIZE: (f64, f64) = (1280.0, 800.0);

    fn ns(s: &str) -> Retained<NSString> { NSString::from_str(s) }

    /// What the delegate learns, which a call reads after the page settles.
    #[derive(Default)]
    struct Shared { status: Option<i64>, failure: Option<String> }

    struct Ivars { key: String, shared: Arc<Mutex<Shared>> }

    static CHANGED: OnceLock<Box<dyn Fn(&str) + Send + Sync>> = OnceLock::new();
    static RESOLVER: Mutex<Option<Box<dyn Fn() -> Option<String> + Send + Sync>>> = Mutex::new(None);

    fn changed(key: &str) { if let Some(f) = CHANGED.get() { f(key); } }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and this class has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "HoverTabDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl WKNavigationDelegate for Delegate {
            #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
            fn decide_action(&self, _web: &WKWebView, action: &WKNavigationAction, handler: &block2::DynBlock<dyn Fn(WKNavigationActionPolicy)>) {
                let scheme = unsafe { action.request().URL().and_then(|u| u.scheme()) }.map(|s| s.to_string().to_lowercase()).unwrap_or_default();
                let ok = ["http", "https", "about", "data", "blob"].contains(&scheme.as_str());
                handler.call((if ok { WKNavigationActionPolicy::Allow } else { WKNavigationActionPolicy::Cancel },));
            }

            #[unsafe(method(webView:decidePolicyForNavigationResponse:decisionHandler:))]
            fn decide_response(&self, _web: &WKWebView, response: &WKNavigationResponse, handler: &block2::DynBlock<dyn Fn(WKNavigationResponsePolicy)>) {
                if unsafe { response.isForMainFrame() } {
                    let code = unsafe { response.response() }.downcast_ref::<objc2_foundation::NSHTTPURLResponse>().map(|h| h.statusCode() as i64);
                    self.ivars().shared.lock().unwrap().status = code;
                }
                handler.call((WKNavigationResponsePolicy::Allow,));
            }

            #[unsafe(method(webView:didStartProvisionalNavigation:))]
            fn started(&self, _web: &WKWebView, _n: Option<&WKNavigation>) { changed(&self.ivars().key); }

            #[unsafe(method(webView:didCommitNavigation:))]
            fn committed(&self, _web: &WKWebView, _n: Option<&WKNavigation>) { changed(&self.ivars().key); }

            #[unsafe(method(webView:didFinishNavigation:))]
            fn finished(&self, _web: &WKWebView, _n: Option<&WKNavigation>) {
                self.ivars().shared.lock().unwrap().failure = None;
                changed(&self.ivars().key);
            }

            #[unsafe(method(webView:didFailNavigation:withError:))]
            fn failed(&self, _web: &WKWebView, _n: Option<&WKNavigation>, error: &NSError) { self.fail(error); }

            #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
            fn failed_early(&self, _web: &WKWebView, _n: Option<&WKNavigation>, error: &NSError) { self.fail(error); }

            #[unsafe(method(webViewWebContentProcessDidTerminate:))]
            fn crashed(&self, _web: &WKWebView) {
                self.ivars().shared.lock().unwrap().failure = Some("The page crashed.".into());
                changed(&self.ivars().key);
            }
        }

        unsafe impl WKUIDelegate for Delegate {
            /// A link that opens a new window opens here instead.
            #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
            fn create(&self, web: &WKWebView, _c: &WKWebViewConfiguration, action: &WKNavigationAction, _f: &WKWindowFeatures) -> Option<Retained<WKWebView>> {
                if let Some(url) = unsafe { action.request().URL() } {
                    let scheme = url.scheme().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
                    if scheme == "http" || scheme == "https" { unsafe { web.loadRequest(&NSURLRequest::requestWithURL(&url)) }; }
                }
                None
            }

            #[unsafe(method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:))]
            fn alert(&self, _web: &WKWebView, _m: &NSString, _f: &WKFrameInfo, done: &block2::DynBlock<dyn Fn()>) { done.call(()); }

            #[unsafe(method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:))]
            fn confirm(&self, _web: &WKWebView, _m: &NSString, _f: &WKFrameInfo, done: &block2::DynBlock<dyn Fn(Bool)>) { done.call((Bool::YES,)); }
        }
    );

    impl Delegate {
        fn new(mtm: MainThreadMarker, key: &str, shared: Arc<Mutex<Shared>>) -> Retained<Delegate> {
            let this = Self::alloc(mtm).set_ivars(Ivars { key: key.to_owned(), shared });
            unsafe { msg_send![super(this), init] }
        }

        fn fail(&self, error: &NSError) {
            // A new navigation replacing this one isn't a failure (NSURLErrorCancelled).
            if error.domain().to_string() == "NSURLErrorDomain" && error.code() == -999 { return; }
            self.ivars().shared.lock().unwrap().failure = Some(error.localizedDescription().to_string());
            changed(&self.ivars().key);
        }
    }

    struct Tab { web: Retained<WKWebView>, _delegate: Retained<Delegate>, shared: Arc<Mutex<Shared>> }

    thread_local! {
        static TABS: RefCell<HashMap<String, Tab>> = RefCell::new(HashMap::new());
    }

    fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect { NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)) }

    fn make(mtm: MainThreadMarker, key: &str) -> Tab {
        unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
            config.preferences().setJavaScriptCanOpenWindowsAutomatically(false);
            config.setApplicationNameForUserAgent(Some(&ns("Hover")));
            let hook = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(WKUserScript::alloc(mtm), &ns(js::CONSOLE_HOOK), WKUserScriptInjectionTime::AtDocumentStart, true);
            config.userContentController().addUserScript(&hook);
            let web = WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame(0.0, 0.0, SIZE.0, SIZE.1), &config);
            let shared = Arc::new(Mutex::new(Shared::default()));
            let delegate = Delegate::new(mtm, key, shared.clone());
            web.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            web.setUIDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            web.setAllowsBackForwardNavigationGestures(true);
            Tab { web, _delegate: delegate, shared }
        }
    }

    /// The tab for a session, made the first time it is asked for. Main thread.
    fn with_tab<R>(key: &str, f: impl FnOnce(&Tab) -> R) -> Option<R> {
        let mtm = MainThreadMarker::new()?;
        TABS.with(|t| {
            let mut t = t.borrow_mut();
            let tab = t.entry(key.to_owned()).or_insert_with(|| make(mtm, key));
            Some(f(tab))
        })
    }

    fn state_of(tab: &Tab) -> TabState {
        unsafe {
            TabState {
                url: tab.web.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string()).unwrap_or_default(),
                title: tab.web.title().map(|s| s.to_string()).unwrap_or_default(),
                loading: tab.web.isLoading(), can_back: tab.web.canGoBack(), can_forward: tab.web.canGoForward(),
                error: tab.shared.lock().unwrap().failure.clone(), shown: tab.web.superview().is_some(),
            }
        }
    }

    /// Runs `f` on the main thread and waits for what it returns. None if the main thread
    /// doesn't come to it in time (a modal dialog is up).
    fn on_main<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
        let (tx, rx) = mpsc::channel();
        slint::invoke_from_event_loop(move || { let _ = tx.send(f()); }).ok()?;
        rx.recv_timeout(Duration::from_secs(10)).ok()
    }

    fn info(key: &str) -> Option<(TabState, Option<i64>)> {
        let key = key.to_owned();
        on_main(move || with_tab(&key, |t| (state_of(t), t.shared.lock().unwrap().status))).flatten()
    }

    /// Until the page has loaded (or failed), at most `secs`.
    fn settle(key: &str, secs: f64) {
        let end = Instant::now() + Duration::from_secs_f64(secs);
        while Instant::now() < end {
            match info(key) { Some((s, _)) if s.loading => std::thread::sleep(Duration::from_millis(100)), _ => return }
        }
    }

    fn pause(ms: u64) { std::thread::sleep(Duration::from_millis(ms)); }

    fn js_error(e: &NSError) -> String {
        let m = e.userInfo().objectForKey(ns_string!("WKJavaScriptExceptionMessage")).and_then(|o| o.downcast::<NSString>().ok());
        m.map_or_else(|| e.localizedDescription().to_string(), |s| s.to_string().replacen("Error: ", "", 1))
    }

    /// Runs a function body in the page and gives back the string it returned.
    fn eval(key: &str, code: String) -> Result<Option<String>, String> {
        let (tx, rx) = mpsc::channel::<Result<Option<String>, String>>();
        let key = key.to_owned();
        on_main(move || with_tab(&key, |t| unsafe {
            let mtm = MainThreadMarker::new().unwrap();
            let block = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                if !error.is_null() { let _ = tx.send(Err(js_error(&*error))); return; }
                let text = if value.is_null() { None } else { (&*value).downcast_ref::<NSString>().map(|s| s.to_string()) };
                let _ = tx.send(Ok(text));
            });
            t.web.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&ns(&code), None, None, &WKContentWorld::pageWorld(mtm), Some(&block));
        })).ok_or("Hover’s browser isn’t answering.")?.ok_or("Hover’s browser isn’t available.")?;
        rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "The page didn’t answer in time.".to_string())?
    }

    /// A screenshot of the page, as JPEG bytes at one pixel per point, and its size.
    fn snapshot(key: &str) -> Result<(Vec<u8>, u32, u32), String> {
        let (tx, rx) = mpsc::channel::<Result<(Vec<u8>, f64, f64), String>>();
        let key = key.to_owned();
        on_main(move || with_tab(&key, |t| unsafe {
            let mtm = MainThreadMarker::new().unwrap();
            let config = WKSnapshotConfiguration::new(mtm);
            let width = t.web.bounds().size.width.min(1280.0);
            config.setSnapshotWidth(Some(&NSNumber::new_f64(width)));
            let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                if image.is_null() {
                    let why = if error.is_null() { "no image".to_string() } else { (&*error).localizedDescription().to_string() };
                    let _ = tx.send(Err(why));
                    return;
                }
                let image = &*image;
                let size = image.size();
                let png = image.TIFFRepresentation().and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                    .and_then(|rep| rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()));
                let _ = tx.send(png.map(|d| (d.to_vec(), size.width, size.height)).ok_or_else(|| "The screenshot couldn’t be made.".to_string()));
            });
            t.web.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block);
        })).ok_or("Hover’s browser isn’t answering.")?.ok_or("Hover’s browser isn’t available.")?;
        let (png, w, h) = rx.recv_timeout(Duration::from_secs(30)).map_err(|_| "The screenshot timed out.".to_string())??;
        let img = image::load_from_memory(&png).map_err(|e| format!("The screenshot couldn’t be read: {e}"))?.to_rgba8();
        let (w, h) = ((w.round() as u32).max(1), (h.round() as u32).max(1));
        // One pixel to the point: a Retina snapshot is four times the pixels for the model
        // to read, and no more to see.
        let img = if img.dimensions() == (w, h) { img } else { image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle) };
        Ok((crate::screen::jpeg(&img, 72), w, h))
    }

    struct Mac;

    impl Mac {
        fn session(&self, tag: &str) -> String {
            if tag != "opencode" { return tag.to_owned(); }
            // OpenCode has one server for all its sessions: the one at work.
            RESOLVER.lock().unwrap().as_ref().and_then(|f| f()).unwrap_or_else(|| tag.to_owned())
        }

        fn run(&self, tag: &str, op: &str, args: &Json) -> Result<(String, Option<String>), String> {
            let key = self.session(tag);
            let key = key.as_str();
            let here_now = |k: &str| info(k).map_or_else(|| here("", None), |(s, _)| here(&s.title, (!s.url.is_empty()).then_some(s.url.as_str())));
            let nothing_open = |k: &str| info(k).is_none_or(|(s, _)| s.url.is_empty());
            match op {
                "open" => {
                    let url = args.get("url").and_then(Json::as_str).and_then(normalize).ok_or(BAD_ADDRESS)?;
                    let k = key.to_owned();
                    on_main(move || with_tab(&k, |t| unsafe {
                        t.shared.lock().unwrap().failure = None;
                        t.shared.lock().unwrap().status = None;
                        if let Some(u) = NSURL::URLWithString(&NSString::from_str(&url)) { t.web.loadRequest(&NSURLRequest::requestWithURL(&u)); }
                    })).ok_or("Hover’s browser isn’t answering.")?;
                    pause(50);
                    settle(key, 30.0);
                    let (s, status) = info(key).ok_or("Hover’s browser isn’t available.")?;
                    if let Some(e) = &s.error { return Err(format!("Couldn’t open {}: {e}", normalize(args.get("url").and_then(Json::as_str).unwrap_or("")).unwrap_or_default())); }
                    // The title comes a moment after the load ends.
                    let mut s = s;
                    for _ in 0..10 { if !s.title.is_empty() { break; } pause(100); s = info(key).map(|i| i.0).unwrap_or(s); }
                    Ok((opened(&here(&s.title, Some(&s.url)), status, s.loading), None))
                }
                "back" => {
                    if !info(key).is_some_and(|(s, _)| s.can_back) { return Err("There is no page to go back to.".into()); }
                    let k = key.to_owned();
                    on_main(move || with_tab(&k, |t| unsafe { t.web.goBack(); }));
                    pause(100);
                    settle(key, 15.0);
                    Ok((format!("Back at {}.", here_now(key)), None))
                }
                "reload" => {
                    let k = key.to_owned();
                    on_main(move || with_tab(&k, |t| unsafe { t.web.reload(); }));
                    pause(100);
                    settle(key, 30.0);
                    if let Some(e) = info(key).and_then(|(s, _)| s.error) { return Err(format!("The page didn’t load: {e}")); }
                    Ok((format!("Reloaded {}.", here_now(key)), None))
                }
                "screenshot" => {
                    if nothing_open(key) { return Err(NOTHING_OPEN.into()); }
                    settle(key, 10.0);
                    let (jpeg, w, h) = snapshot(key)?;
                    Ok((format!("Screenshot of {}, {w}×{h}.", here_now(key)), Some(hover_agents::http::base64(&jpeg))))
                }
                _ => {
                    if nothing_open(key) { return Err(NOTHING_OPEN.into()); }
                    let code = function_body(op, args).ok_or_else(|| format!("Unknown browser tool {op}."))?;
                    if op != "evaluate" { settle(key, 10.0); }
                    let before = info(key).map(|(s, _)| s.url);
                    let value = eval(key, code)?;
                    let mut text = if op == "evaluate" { clip_result(&value.unwrap_or_else(|| "undefined".into())) } else { value.unwrap_or_else(|| "Done.".into()) };
                    // A click or a submit may start a page load: wait for it, and say where it went.
                    if ["click", "type", "press"].contains(&op) {
                        pause(350);
                        settle(key, 15.0);
                        if info(key).map(|(s, _)| s.url) != before { text.push_str(&format!(" Now at {}.", here_now(key))); }
                    }
                    Ok((text, None))
                }
            }
        }
    }

    impl Host for Mac {
        fn call(&self, session: &str, op: &str, args: &Json) -> Reply {
            match self.run(session, op, args) {
                Ok((text, image)) => Reply { ok: true, text, mime: image.as_ref().map(|_| "image/jpeg".to_owned()), image },
                Err(e) => Reply::text(false, &e),
            }
        }
    }

    pub fn install() { hover_agents::browser::set_host(Box::new(Mac)); }

    /// Who the OpenCode server's calls are for: the session of OpenCode at work.
    pub fn set_resolver(f: impl Fn() -> Option<String> + Send + Sync + 'static) { *RESOLVER.lock().unwrap() = Some(Box::new(f)); }

    /// Told (on the main thread) whenever a tab's page, title or loading changes.
    pub fn on_change(f: impl Fn(&str) + Send + Sync + 'static) { let _ = CHANGED.set(Box::new(f)); }

    /// A tab as the panel shows it, if the session has one.
    pub fn state(session: &str) -> Option<TabState> { TABS.with(|t| t.borrow().get(session).map(state_of)) }

    /// Every tab, for an office that just loaded.
    pub fn states() -> Vec<(String, TabState)> {
        TABS.with(|t| t.borrow().iter().map(|(k, t)| (k.clone(), state_of(t))).collect())
    }

    /// The user typed an address in the panel.
    pub fn go(session: &str, text: &str) {
        let Some(url) = normalize(text) else { return };
        with_tab(session, |t| unsafe {
            t.shared.lock().unwrap().failure = None;
            if let Some(u) = NSURL::URLWithString(&NSString::from_str(&url)) { t.web.loadRequest(&NSURLRequest::requestWithURL(&u)); }
        });
    }

    /// Back, forward, stop or reload (the panel's buttons).
    pub fn nav(session: &str, what: &str) {
        TABS.with(|t| if let Some(tab) = t.borrow().get(session) {
            unsafe {
                match what { "back" => { tab.web.goBack(); } "forward" => { tab.web.goForward(); } "stop" => tab.web.stopLoading(), _ => { tab.web.reload(); } }
            }
        });
    }

    /// Shows the session's browser over `rect` (x, y, width, height in points from the
    /// top left of `window`'s content), and takes any other out of that window. A rect too
    /// small to use parks it instead. The window's content view keeps it until `park`.
    pub fn show(session: &str, rect: (f64, f64, f64, f64), window: &NSWindow) {
        let Some(view) = window.contentView() else { return };
        TABS.with(|t| for (k, tab) in t.borrow().iter() {
            if k != session && unsafe { tab.web.superview() }.as_deref().is_some_and(|s| std::ptr::eq(s, &*view)) { park_tab(tab); }
        });
        if rect.2 <= 20.0 || rect.3 <= 20.0 { park(session); return; }
        with_tab(session, |t| unsafe {
            if !t.web.superview().as_deref().is_some_and(|s| std::ptr::eq(s, &*view)) { t.web.removeFromSuperview(); view.addSubview(&t.web); }
            let y = if view.isFlipped() { rect.1 } else { view.bounds().size.height - rect.1 - rect.3 };
            t.web.setFrame(frame(rect.0.floor(), y.floor(), rect.2.ceil(), rect.3.ceil()));
        });
        changed(session);
    }

    fn park_tab(t: &Tab) {
        t.web.removeFromSuperview();
        t.web.setFrame(frame(0.0, 0.0, SIZE.0, SIZE.1));
    }

    /// The office folded or closed: the browser goes back to no window.
    pub fn park(session: &str) {
        TABS.with(|t| if let Some(tab) = t.borrow().get(session) { park_tab(tab); });
        changed(session);
    }

    /// Sessions that are gone take their browsers with them.
    pub fn forget(keep: &[String]) {
        TABS.with(|t| t.borrow_mut().retain(|k, tab| {
            let keepit = keep.contains(k);
            if !keepit { unsafe { tab.web.stopLoading(); } tab.web.removeFromSuperview(); }
            keepit
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_made_from_what_was_typed() {
        assert_eq!(normalize("localhost:3000").as_deref(), Some("http://localhost:3000"));
        assert_eq!(normalize("  127.0.0.1:8080/app ").as_deref(), Some("http://127.0.0.1:8080/app"));
        assert_eq!(normalize("[::1]:5173").as_deref(), Some("http://[::1]:5173"));
        assert_eq!(normalize("example.com/a?b=c").as_deref(), Some("https://example.com/a?b=c"));
        assert_eq!(normalize("HTTP://Example.com").as_deref(), Some("HTTP://Example.com"));
        assert_eq!(normalize("https://user@host.dev:8443/x").as_deref(), Some("https://user@host.dev:8443/x"));
    }

    #[test]
    fn only_http_and_https_pages_open() {
        for bad in ["", "   ", "file:///etc/passwd", "javascript:alert(1)", "ftp://example.com", "data:text/html,hi", "http://", "https:///path", "http://exa mple.com", "about:blank"] {
            assert_eq!(normalize(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_script_runs_as_a_function_body_with_the_arguments_in_front() {
        let args = Json::obj(vec![("ref", Json::int(3)), ("text", Json::str("say \"hi\"\u{2028}"))]);
        let body = function_body("click", &args).unwrap();
        // Compact JSON (quotes written as \u0022 by hover-core), the line separator escaped.
        assert!(body.starts_with("const a = {\"ref\":3,\"text\":\"say ") && body[..90].contains("\\u2028\"};\n"), "{}", &body[..90]);
        assert!(!body.contains('\u{2028}'));
        assert!(body.contains("const find = (a, fields) =>") && body.contains("el.click();"));
        assert!(function_body("snapshot", &Json::obj(vec![])).unwrap().contains("data-hover-ref"));
        assert!(function_body("console", &Json::obj(vec![])).unwrap().contains("__hoverConsole"));
        assert!(function_body("nope", &Json::obj(vec![])).is_none());
    }

    #[test]
    fn evaluate_takes_an_expression_or_a_body_and_answers_in_json() {
        let ev = |s: &str| function_body("evaluate", &Json::obj(vec![("script", Json::str(s))])).unwrap();
        assert!(ev("document.title").contains("return (document.title)"));
        assert!(ev("  1 + 1  ").contains("return (1 + 1)"));
        assert!(ev("const x = 1; return x").contains("const x = 1; return x\n})()"));
        assert!(ev("return 1").contains("return 1\n})()"), "a body with return is run as it is");
        assert!(ev("x").contains("if (__v === undefined) return 'undefined';") && ev("x").contains("JSON.stringify(__v, null, 2)"));
    }

    #[test]
    fn every_tool_the_server_lists_has_an_answer_here() {
        for op in OPS {
            match op {
                "open" | "screenshot" | "evaluate" | "back" | "reload" => {}
                _ => assert!(script_of(op).is_some(), "{op}"),
            }
        }
        // And the tools the MCP server lists are exactly these.
        let tools = hover_agents::browser::tools();
        let names: Vec<String> = tools.items().unwrap().iter().filter_map(|t| t.get("name").and_then(Json::as_str).map(|n| n.trim_start_matches("browser_").to_owned())).collect();
        let mut listed = names.clone();
        listed.sort();
        let mut ours: Vec<String> = OPS.iter().map(|s| s.to_string()).collect();
        ours.sort();
        assert_eq!(listed, ours);
    }

    #[test]
    fn the_answers_read_as_the_mac_app_said_them() {
        let h = here("Hover — Agent Office", Some("http://localhost:3000/"));
        assert_eq!(h, "Hover — Agent Office — http://localhost:3000/");
        assert_eq!(opened(&h, Some(200), false), "Opened Hover — Agent Office — http://localhost:3000/ (HTTP 200). Use browser_snapshot to read it.");
        assert_eq!(opened("x", None, true), "Opened x. It is still loading. Use browser_snapshot to read it.");
        assert_eq!(here("", None), "(no title) — about:blank");
        assert_eq!(clip_result(&"a".repeat(30_000)).chars().count(), 20_001);
        assert_eq!(clip_result("short"), "short");
    }

    #[test]
    fn where_there_is_no_browser_the_switch_says_so() {
        assert_eq!(supported(), cfg!(target_os = "macos"));
        assert_eq!(note(), if cfg!(target_os = "macos") { None } else { Some("Agent browser needs macOS.") });
        assert_eq!(NOTE, "Agent browser needs macOS.");
    }
}
