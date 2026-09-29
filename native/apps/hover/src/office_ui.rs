//! Owl/KiroPage.cs without the web view: the office (hover-office, on its own thread)
//! fed with Hover's state (KiroPage.Push, every change, at most every 120 ms), and the
//! page's own logic around it: tags, the HUD, the new-task circle, the panels, the chat
//! drawer (hover-chat's thread), the toast, deleting after asking.

use crate::ui::*;
use crate::App;
use hover_agents::session::KiroSession;
use hover_core::model::AgentTool;
use hover_office::bot::Stage;
use hover_office::live::{In, Live};
use hover_office::office::{Click, Time};
use slint::{Color, ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const TOOLS: [(&str, &str, u32); 3] = [("kiro", "Kiro", 0xb48cff), ("codex", "Codex", 0x3fd6a0), ("cursor", "Cursor", 0x7cc0ff)];
fn tool_color(id: &str) -> Color { let c = TOOLS.iter().find(|t| t.0 == id).map_or(0xb48cff, |t| t.2); Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8) }
fn tool_name(id: &str) -> &'static str { TOOLS.iter().find(|t| t.0 == id).map_or("Kiro", |t| t.1) }
fn s(v: impl AsRef<str>) -> SharedString { v.as_ref().into() }

/// A panel's title, its line under it, its rows, and what each row opens (a session
/// by id, a history entry by key).
type Panel = (String, String, Vec<PanelRow>, Vec<(Option<i64>, Option<String>)>);

pub struct Page {
    pub live: RefCell<Option<Live>>,
    size: Cell<(u32, u32)>,
    pub open: Cell<Option<i32>>,
    pub panel: Cell<Option<&'static str>>,
    fab: Cell<i32>,
    new_tool: Cell<usize>,
    new_folder: RefCell<Option<String>>,
    time_mode: Cell<i32>,
    toast_timer: slint::Timer,
    push_timer: slint::Timer,
    dirty: Cell<bool>,
    history_sent: Cell<usize>,
    confirm_key: RefCell<Option<(Option<i32>, Option<String>)>>,
    last_state: RefCell<hover_core::json::Json>,
    thread: RefCell<Option<Chat>>,
    rows_open: RefCell<Vec<(Option<i64>, Option<String>)>>,
    pub target: Cell<i32>,
    shown: Cell<Option<bool>>,
    drop_timer: slint::Timer,
    view: Cell<Option<[f64; 3]>>,
}

/// The drawer's thread, laid out and painted by hover-chat.
struct Chat { id: i32, thread: hover_chat::Thread, painter: hover_chat::Painter, scroll: f32, width: f32, key: String }

fn fonts() -> Vec<Vec<u8>> { vec![hover_office::canvas::PIXELIFY.to_vec()] }

impl Default for Page {
    fn default() -> Page {
        Page { live: RefCell::new(None), size: Cell::new((0, 0)), open: Cell::new(None), panel: Cell::new(None), fab: Cell::new(0), new_tool: Cell::new(0),
            new_folder: RefCell::new(None), time_mode: Cell::new(0), toast_timer: Default::default(), push_timer: Default::default(), dirty: Cell::new(true),
            history_sent: Cell::new(usize::MAX), confirm_key: RefCell::new(None), last_state: RefCell::new(hover_core::json::Json::Null), thread: RefCell::new(None),
            rows_open: RefCell::new(vec![]), target: Cell::new(0), shown: Cell::new(None), drop_timer: Default::default(), view: Cell::new(None) }
    }
}

/// Every window with an office runs this against its own Office global.
macro_rules! each {
    ($a:expr, |$g:ident| $body:expr) => {{
        { let $g = $a.notch.global::<crate::ui::Office>(); $body; }
        if let Some(d) = &*$a.dash.borrow() { let $g = d.global::<crate::ui::Office>(); $body; }
    }};
}

impl App {
    /// The office's window and size: the app window when it is up, else the open notch.
    fn office_size(&self) -> Option<(u32, u32, i32)> {
        if let Some(d) = &*self.dash.borrow() {
            if d.window().is_visible() {
                let sz = d.window().size();
                let k = d.window().scale_factor();
                return Some(((sz.width as f32 / k) as u32, (sz.height as f32 / k) as u32, 1));
            }
        }
        let n = self.n.borrow();
        (n.hover.state != hover_notch::State::Rest || self.headless).then(|| ((n.open_size.0 - 16.0) as u32, (n.open_size.1 - 16.0) as u32, 0))
    }

    /// Starts the office thread the first time an office is seen, and tells it whether
    /// it is in view (a hidden page draws nothing).
    pub fn office_follow(self: &Rc<Self>) {
        let want = self.office_size();
        let p = &self.page;
        if p.live.borrow().is_none() {
            let Some((w, h, _)) = want else { return };
            let still = !self.look.get().animations;
            let live = Live::start(w, h, still, || crate::ui_do(|a| a.office_frame()));
            // The page made again: the camera where the user left it (office.view), and the
            // chat that was open.
            let view = p.view.get().or_else(|| crate::local_get("view").and_then(|v| {
                let n: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                (n.len() == 3 && n.iter().all(|x| x.is_finite())).then(|| [n[0], n[1], n[2]])
            }));
            if let Some(v) = view { live.send(In::View(v)); }
            if let Some(id) = p.open.get() { live.send(In::Drawer(Some(id as i64))); }
            p.size.set((w, h));
            p.shown.set(None);
            *p.live.borrow_mut() = Some(live);
            p.dirty.set(true);
            p.history_sent.set(usize::MAX);
            self.office_push();
            let a = self.clone();
            // KiroPage's push timer: at most every 120 ms, when something changed.
            p.push_timer.start(slint::TimerMode::Repeated, Duration::from_millis(120), move || if a.page.dirty.get() { a.office_push(); });
        }
        let live = p.live.borrow();
        let live = live.as_ref().unwrap();
        // Only when it changes: every message to the page wakes it to full speed.
        if p.shown.get() != Some(want.is_some()) {
            p.shown.set(Some(want.is_some()));
            live.send(In::Visible(want.is_some()));
            // Hidden 30 s, the page is dropped (KiroPage's rule for its WebView2); shown
            // again, it is made again at once.
            if want.is_none() {
                let a = self.clone();
                p.drop_timer.start(slint::TimerMode::SingleShot, Duration::from_secs(30), move || a.office_drop());
            } else {
                p.drop_timer.stop();
            }
        }
        if let Some((w, h, which)) = want {
            p.target.set(which);
            if p.size.get() != (w, h) { p.size.set((w, h)); live.send(In::Resize(w, h)); }
        }
    }

    pub fn office_changed(&self) { self.page.dirty.set(true); }

    /// The office thread goes, and its GPU memory with it.
    fn office_drop(self: &Rc<Self>) {
        let p = &self.page;
        if p.shown.get() != Some(false) { return; }
        p.push_timer.stop();
        *p.live.borrow_mut() = None;
        *p.thread.borrow_mut() = None;
        // The last frame and its blurred copy would otherwise stay in the globals.
        let clear = |g: crate::ui::Office| { g.set_scene(Image::default()); g.set_tags(ModelRc::default()); };
        clear(self.notch.global::<crate::ui::Office>());
        self.notch.global::<crate::ui::Backdrop>().set_blurred(Image::default());
        if let Some(d) = &*self.dash.borrow() { clear(d.global::<crate::ui::Office>()); d.global::<crate::ui::Backdrop>().set_blurred(Image::default()); }
        hover_core::log::line("office dropped after 30 s hidden");
        // glibc keeps what the office thread freed (its arena, the software GPU's
        // buffers) mapped; the drop is for the memory, so give it back once the thread
        // has gone.
        #[cfg(target_os = "linux")]
        slint::Timer::single_shot(Duration::from_secs(2), || unsafe { libc::malloc_trim(0); });
    }

    /// KiroPage.Push: every session and what the page needs to show them.
    pub fn office_push(self: &Rc<Self>) {
        let p = &self.page;
        p.dirty.set(false);
        let hv = &self.hover;
        let sessions = hv.sessions.all();
        let folder = hv.settings.kiro_folder().filter(|f| hover_agents::usable_folder(Some(f)));
        let entries = hv.history.as_ref().map(|h| h.entries()).unwrap_or_default();
        let history = (entries.len() != p.history_sent.get()).then(|| { p.history_sent.set(entries.len()); entries.clone() });
        let ready = |t: AgentTool| hover_agents::agents::known(t);
        let files = |_: &KiroSession| None;
        let o = hover_agents::state::Office { window: p.target.get() == 1, open: p.open.get(), settings: &hv.settings, folder: folder.clone(), history, ready: &ready, files: &files };
        let msg = hover_agents::state::push(&o, &sessions);
        if let Some(l) = &*p.live.borrow() { l.send(In::State(msg.clone())); }
        *p.last_state.borrow_mut() = msg;
        if p.new_folder.borrow().is_none() { *p.new_folder.borrow_mut() = folder; }
        for t in AgentTool::ALL { if hover_agents::agents::known(t).is_none() { std::thread::spawn(move || { hover_agents::agents::check(t, false); crate::ui_do(|a| a.office_changed()); }); } }
        self.office_widgets();
    }

    /// A new frame from the office thread: the picture, the tags, the tooltip; the clicks.
    pub fn office_frame(self: &Rc<Self>) {
        let out = match &*self.page.live.borrow() { Some(l) => l.take(), None => return };
        if let Some(e) = &out.error { hover_core::log::line(&format!("office: {e}")); return; }
        if out.rgba.is_empty() && out.clicks.is_empty() { return; }
        if !out.rgba.is_empty() { crate::bench::office_frame(); }
        if !out.rgba.is_empty() && self.page.view.get() != Some(out.view) {
            self.page.view.set(Some(out.view));
            crate::local_set("view", &format!("{},{},{}", out.view[0], out.view[1], out.view[2]));
        }
        if !out.rgba.is_empty() {
            let rgb = &out.rgb;
            let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(out.w, out.h);
            for (d, s) in buf.make_mut_slice().iter_mut().zip(rgb.chunks(3)) { *d = Rgba8Pixel { r: s[0], g: s[1], b: s[2], a: 255 }; }
            let img = Image::from_rgba8(buf);
            let blurred = Image::from_rgba8(blur(rgb, out.w as usize, out.h as usize));
            let tags: Vec<TagData> = out.tags.iter().map(|t| TagData {
                id: t.id as i32, x: t.x as f32, y: t.y as f32, name: s(t.name), color: Color::from_rgb_u8(t.color[0], t.color[1], t.color[2]),
                tool: s(tool_name(&t.tool)), tool_color: tool_color(&t.tool), text: s(&t.text), stage: t.stage as i32, hot: t.hot,
            }).collect();
            let hint = if out.hint == "clock" { full_date() } else { out.hint.clone() };
            let (tx, ty) = out.pointer.unwrap_or((0.0, 0.0));
            let which = self.page.target.get();
            let set = |g: crate::ui::Office| {
                g.set_scene(img.clone());
                g.set_tags(ModelRc::new(VecModel::from(tags.clone())));
                g.set_hint(s(&hint));
                g.set_tip_x(tx as f32);
                g.set_tip_y(ty as f32);
            };
            if which == 1 {
                if let Some(d) = &*self.dash.borrow() { set(d.global::<crate::ui::Office>()); d.global::<crate::ui::Backdrop>().set_blurred(blurred); }
            } else {
                set(self.notch.global::<crate::ui::Office>());
                self.notch.global::<crate::ui::Backdrop>().set_blurred(blurred);
            }
        }
        for c in out.clicks {
            match c {
                Click::Open(id) => self.open_session(id as i32),
                Click::Panel(p) => self.open_panel(Some(p)),
                Click::Toast(_) => self.toast(&full_date()),
                Click::NewTask => { self.close_drawer(); self.open_panel(None); self.page.fab.set(1); self.office_widgets(); }
                Click::Time(t) => { self.page.time_mode.set(if t == Time::Night { 1 } else { 2 }); self.office_widgets(); }
                Click::Fold => self.collapse(),
                Click::Nothing => {
                    if self.page.fab.get() != 0 { self.page.fab.set(0); self.office_widgets(); }
                    else if self.page.open.get().is_some() { self.close_drawer(); }
                    else if self.page.panel.get().is_some() { self.open_panel(None); }
                }
            }
        }
    }

    pub fn toast(self: &Rc<Self>, text: &str) {
        each!(self, |g| { g.set_toast(s(text)); g.set_toast_shown(true); });
        let a = self.clone();
        self.page.toast_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(2800), move || each!(a, |g| g.set_toast_shown(false)));
    }

    fn send(&self, m: In) { if let Some(l) = &*self.page.live.borrow() { l.send(m); } }

    pub fn open_session(self: &Rc<Self>, id: i32) {
        self.page.fab.set(0);
        self.page.panel.set(None);
        self.send(In::Panel(None));
        self.page.open.set(Some(id));
        self.send(In::Drawer(Some(id as i64)));
        *self.page.thread.borrow_mut() = None;
        self.office_widgets();
    }

    pub fn close_drawer(self: &Rc<Self>) {
        self.page.open.set(None);
        self.send(In::Drawer(None));
        *self.page.thread.borrow_mut() = None;
        self.office_widgets();
    }

    pub fn open_panel(self: &Rc<Self>, p: Option<&'static str>) {
        if p.is_some() { self.page.open.set(None); self.send(In::Drawer(None)); self.page.fab.set(0); }
        self.page.panel.set(p);
        self.send(In::Panel(p));
        self.office_widgets();
    }

    /// Everything around the scene, from the state and the page's own state.
    pub fn office_widgets(self: &Rc<Self>) {
        let p = &self.page;
        let st = p.last_state.borrow().clone();
        let sessions = self.hover.sessions.all();
        let tools: Vec<ToolData> = AgentTool::ALL.iter().map(|&t| {
            let r = hover_agents::agents::known(t);
            ToolData { id: s(t.id()), name: s(t.name()), ready: r.as_ref().is_none_or(|r| r.ok()), hint: s(r.map(|r| r.hint).unwrap_or_default()) }
        }).collect();
        let nt = p.new_tool.get();
        let tool = AgentTool::ALL[nt];
        let ready = hover_agents::agents::known(tool).is_none_or(|r| r.ok());
        let folder = p.new_folder.borrow().clone();
        let full = sessions.len() >= 6 && sessions.iter().all(|s| s.busy());
        let can = self.hover.sessions.can_start();
        let note = if !ready { hover_agents::agents::known(tool).map(|r| r.hint).unwrap_or_default() } else if !can { "3 tasks are running. Start another when one is done.".into() } else if full { "All six desks are busy. Stop or remove a session first.".into() } else { String::new() };
        // The panel's rows.
        let (title, sub, rows, opens) = self.panel_rows(&st, &sessions);
        *p.rows_open.borrow_mut() = opens;
        let open = p.open.get().and_then(|id| sessions.iter().find(|s| s.id == id).cloned());
        let summary = if sessions.is_empty() { "The office is quiet.".to_owned() } else { format!("{} sessions in the office", sessions.len()) };
        let fab = p.fab.get();
        let time_mode = p.time_mode.get();
        let beats = self.beats.want();
        each!(self, |g| {
            g.set_tools(ModelRc::new(VecModel::from(tools.clone())));
            g.set_new_tool(nt as i32);
            g.set_fab(fab);
            g.set_new_folder(s(folder.as_deref().map(hover_office::office::short).unwrap_or_else(|| "Choose a folder".into())));
            g.set_new_note(s(&note));
            g.set_new_go(ready && can && !full && folder.is_some() && !g.get_new_draft().trim().is_empty());
            g.set_time_mode(time_mode);
            g.set_beats(beats);
            g.set_summary(s(&summary));
            g.set_panel(match p.panel.get() { Some("board") => 1, Some("tv") => 2, Some("history") => 3, _ => 0 });
            g.set_panel_title(s(&title));
            g.set_panel_sub(s(&sub));
            g.set_rows(ModelRc::new(VecModel::from(rows.clone())));
            g.set_drawer(open.is_some());
            if let Some(o) = &open {
                let (name, c) = hover_office::bot::BOTS[o.bot % 6];
                let _ = name;
                g.set_d_name(s(hover_office::bot::BOTS[o.bot % 6].0));
                g.set_d_color(Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8));
                g.set_d_tool(s(o.tool.name()));
                g.set_d_tool_color(tool_color(o.tool.id()));
                g.set_d_title(s(o.title()));
                g.set_d_folder(s(&o.folder));
                g.set_d_busy(o.busy());
                g.set_d_placeholder(s(if o.busy() { format!("Reply now, {} reads it when this run ends", o.tool.name()) } else { format!("Reply to {}…", hover_office::bot::BOTS[o.bot % 6].0) }));
            }
        });
        if open.is_some() { self.paint_thread(); }
    }

    fn panel_rows(&self, st: &hover_core::json::Json, sessions: &[KiroSession]) -> Panel {
        let mut rows = vec![];
        let mut opens = vec![];
        let stage_of = |s: &KiroSession| Stage::parse(hover_agents::state::stage(s.state, s.phase));
        let bot_color = |b: usize| { let c = hover_office::bot::BOTS[b % 6].1; Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8) };
        let row = |kind: i32| PanelRow { kind, pct: 0.0, ..Default::default() };
        let now = hover_core::time::Stamp::now().unix_ms() as f64;
        match self.page.panel.get() {
            Some("board") => {
                for (h, c, st) in [("Waking", 0xf5b83du32, &[Stage::Waking][..]), ("Doing", 0x9b6bff, &[Stage::Working]), ("Finished", 0x2fae66, &[Stage::Done, Stage::Failed, Stage::Stopped])] {
                    let list: Vec<&KiroSession> = sessions.iter().filter(|s| st.contains(&stage_of(s))).collect();
                    rows.push(PanelRow { text: s(h.to_uppercase()), color: Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8), count: s(list.len().to_string()), ..row(0) });
                    opens.push((None, None));
                    for x in &list {
                        let stg = stage_of(x);
                        let steps = x.current().map_or(0, |t| t.steps.len());
                        rows.push(PanelRow { sub: s(format!("{} · {} · {}", hover_office::bot::BOTS[x.bot % 6].0, x.tool.name(), stg.word())), text: s(x.title()),
                            meta: s(format!("{}{}", ago(now - x.current().map_or(now, |t| t.started_at.unix_ms() as f64)), if steps > 0 { format!(" · {steps} step{}", if steps == 1 { "" } else { "s" }) } else { String::new() })),
                            color: bot_color(x.bot), stage: stg as i32, ..row(1) });
                        opens.push((Some(x.id as i64), None));
                    }
                    if list.is_empty() { rows.push(PanelRow { text: s("Nobody here"), ..row(4) }); opens.push((None, None)); }
                }
                ("Session board".into(), format!("{} of 6 desks in use", sessions.len()), rows, opens)
            }
            Some("tv") => {
                let n = |st: &[Stage]| sessions.iter().filter(|s| st.contains(&stage_of(s))).count().to_string();
                rows.push(PanelRow { s1: s(n(&[Stage::Waking, Stage::Working])), s2: s(n(&[Stage::Done])), s3: s(n(&[Stage::Failed])), s4: s(n(&[Stage::Stopped])), ..row(2) });
                opens.push((None, None));
                rows.push(PanelRow { text: s("CONTEXT USED"), color: Color::from_argb_u8(0, 0, 0, 0), ..row(0) });
                opens.push((None, None));
                for x in sessions {
                    rows.push(PanelRow { sub: s(hover_office::bot::BOTS[x.bot % 6].0), text: s(x.title()), color: bot_color(x.bot), pct: x.context.unwrap_or(0.0) as f32,
                        count: s(x.context.map_or("—".into(), |c| format!("{}%", c.round()))), stage: -1, ..row(3) });
                    opens.push((Some(x.id as i64), None));
                }
                if sessions.is_empty() { rows.push(PanelRow { text: s("No sessions yet. Press + to give an agent a task."), ..row(4) }); opens.push((None, None)); }
                ("Office overview".into(), "Up to 3 tasks run at once, across Kiro, Codex and Cursor".into(), rows, opens)
            }
            Some(_) => {
                let find = self.notch.global::<crate::ui::Office>().get_find().to_string().to_lowercase();
                let all = self.hover.history.as_ref().map(|h| h.entries()).unwrap_or_default();
                let list: Vec<_> = all.iter().filter(|h| find.is_empty() || format!("{} {} {}", h.title, h.folder, h.tool.id()).to_lowercase().contains(&find)).collect();
                let _ = st;
                let mut at = String::new();
                for h in &list {
                    let ms = h.updated.unix_ms() as f64;
                    let d = day(now, ms);
                    if d != at { at = d.clone(); rows.push(PanelRow { text: s(d.to_uppercase()), color: Color::from_argb_u8(0, 0, 0, 0), ..row(0) }); opens.push((None, None)); }
                    let desk = sessions.iter().any(|s| s.key == h.key);
                    let stage = Stage::parse(hover_agents::state::stage(h.state, hover_agents::stream::KiroPhase::Working));
                    rows.push(PanelRow { sub: s(format!("{} · {}{}", stage.word(), h.tool.name(), if desk { " · at a desk" } else { "" })), text: s(&h.title),
                        meta: s(format!("{} · {} turn{} · {}", ago(now - ms), h.turns, if h.turns == 1 { "" } else { "s" }, hover_office::office::short(&h.folder))),
                        color: tool_color(h.tool.id()), stage: stage as i32, key: s(&h.key), desk, ..row(1) });
                    opens.push((None, Some(h.key.clone())));
                }
                if list.is_empty() { rows.push(PanelRow { text: s(if all.is_empty() { "Sessions you start are kept here. Open one to read it, reply to carry on." } else { "Nothing matches." }), ..row(4) }); opens.push((None, None)); }
                ("Session history".into(), format!("{} session{}, kept until you delete them", all.len(), if all.len() == 1 { "" } else { "s" }), rows, opens)
            }
            None => (String::new(), String::new(), rows, opens),
        }
    }

    /// The open session's thread, as the drawer shows it (renderDrawer, through hover-chat).
    fn paint_thread(self: &Rc<Self>) {
        let Some(id) = self.page.open.get() else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        let files = |_: &KiroSession| None;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&hover_agents::state::state(&sess, &files).compact()) else { return };
        let turns = hover_chat::state::turns(&v);
        let which = self.page.target.get();
        let dash = self.dash.borrow();
        let g = if which == 1 { dash.as_ref().map(|d| d.global::<crate::ui::Office>()) } else { Some(self.notch.global::<crate::ui::Office>()) };
        let Some(g) = g else { return };
        // #drawer: min(400, W − 24) wide, the thread between its header (≈ 76) and the
        // composer (≈ 62).
        let (ow, oh) = self.page.size.get();
        let w = (400.0f32).min(ow as f32 - 24.0);
        let h = (oh as f32 - 24.0 - 76.0 - 62.0).max(40.0);
        let _ = (g.get_d_thread_w(), g.get_d_thread_h());
        let mut chat = self.page.thread.borrow_mut();
        if chat.as_ref().is_none_or(|c| c.id != id) {
            let (name, c) = hover_office::bot::BOTS[sess.bot % 6];
            let f = fonts();
            *chat = Some(Chat { id, thread: hover_chat::Thread::new(hover_chat::Shaper::new(&f), name, [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]),
                painter: hover_chat::Painter::new(&f, hover_chat::Images::none()), scroll: f32::MAX, width: 0.0, key: sess.key.clone() });
        }
        let c = chat.as_mut().unwrap();
        c.thread.set(&turns, w);
        c.width = w;
        let max = (c.thread.height - h).max(0.0);
        c.scroll = c.scroll.clamp(0.0, max);
        let k = if which == 1 { self.dash.borrow().as_ref().map_or(1.0, |d| d.window().scale_factor()) } else { self.notch.window().scale_factor() };
        let px = c.painter.paint(&c.thread, c.scroll, (w * k).round() as u32, (h * k).round() as u32, k, [0, 0, 0, 0]);
        let img = Image::from_rgba8_premultiplied(SharedPixelBuffer::clone_from_slice(px.data(), px.width(), px.height()));
        g.set_d_thread(img);
        let _ = &c.key;
    }

    pub fn wire_office(self: &Rc<Self>, g: crate::ui::Office) {
        let a = self.clone();
        g.on_pointer(move |k, x, y| match k {
            0 => a.send(In::Pointer(Some((x as f64, y as f64)))),
            1 => { a.send(In::Pointer(Some((x as f64, y as f64)))); a.send(In::Down(x as f64, y as f64)); }
            2 => a.send(In::Up),
            3 => a.send(In::DoubleClick),
            _ => a.send(In::Pointer(None)),
        });
        let a = self.clone();
        g.on_wheel(move |dy, x, y| a.send(In::Wheel(-dy as f64 * 1.0, x as f64, y as f64)));
        let a = self.clone();
        g.on_key(move |t| if let Some(c) = t.chars().next() { a.send(In::Key(c)); });
        let a = self.clone();
        g.on_set_time(move |m| { a.page.time_mode.set(m); a.send(In::Time(match m { 1 => Some(Time::Night), 2 => Some(Time::Day), _ => None })); crate::local_set("time", ["", "night", "day"][m as usize]); a.office_widgets(); });
        let a = self.clone();
        g.on_open_history(move || { let open = a.page.panel.get() == Some("history"); a.open_panel(if open { None } else { Some("history") }); });
        let a = self.clone();
        g.on_fab_main(move || { let f = a.page.fab.get(); a.close_drawer(); a.page.panel.set(None); a.send(In::Panel(None)); a.page.fab.set(if f == 1 { 0 } else { 1 }); a.office_widgets(); });
        let a = self.clone();
        g.on_pick_tool(move |i| {
            let t = AgentTool::ALL[i as usize];
            if let Some(r) = hover_agents::agents::known(t).filter(|r| !r.ok()) { a.toast(&r.hint); return; }
            a.page.new_tool.set(i as usize);
            a.hover.settings.set_agent_tool(t);
            a.page.fab.set(2);
            a.office_widgets();
        });
        let a = self.clone();
        g.on_new_fold(move || { a.page.fab.set(0); a.office_widgets(); });
        let a = self.clone();
        g.on_new_folder_clicked(move || { if let Some(f) = crate::pick(true) { *a.page.new_folder.borrow_mut() = Some(f); a.office_widgets(); } });
        let a = self.clone();
        g.on_new_go_clicked(move || {
            let text = each_draft(&a).trim().to_owned();
            let Some(folder) = a.page.new_folder.borrow().clone() else { return };
            if text.is_empty() { return; }
            let tool = AgentTool::ALL[a.page.new_tool.get()];
            if !a.hover.settings.kiro_notice_seen() { a.hover.settings.set_kiro_notice_seen(true); }
            match a.hover.sessions.start(tool, &folder, &text, vec![]) {
                Some(_) => { each!(a, |g| g.set_new_draft(s(""))); a.page.fab.set(0); }
                None => a.toast("All six desks are busy. Stop or remove a session first."),
            }
            a.office_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_tag_clicked(move |id| a.open_session(id));
        let a = self.clone();
        g.on_panel_close(move || a.open_panel(None));
        let a = self.clone();
        g.on_row_clicked(move |i| {
            let r = a.page.rows_open.borrow().get(i as usize).cloned();
            match r {
                Some((Some(id), _)) => a.open_session(id as i32),
                Some((None, Some(key))) => {
                    if let Some(s) = a.hover.sessions.all().into_iter().find(|s| s.key == key) { a.open_session(s.id); }
                    else if let Some(s) = a.hover.sessions.wake(&key) { a.office_changed(); a.open_session(s.id); }
                }
                _ => {}
            }
        });
        let a = self.clone();
        g.on_row_delete(move |i| {
            let r = a.page.rows_open.borrow().get(i as usize).cloned();
            if let Some((_, Some(key))) = r {
                let title = a.hover.history.as_ref().and_then(|h| h.entries().into_iter().find(|e| e.key == key)).map(|e| e.title).unwrap_or_default();
                a.ask_delete(None, Some(key), &title, false);
            }
        });
        let a = self.clone();
        g.on_find_edited(move |t| { each!(a, |g| g.set_find(t.clone())); a.office_widgets(); });
        let a = self.clone();
        g.on_confirm_no(move || { *a.page.confirm_key.borrow_mut() = None; each!(a, |g| g.set_confirm(false)); });
        let a = self.clone();
        g.on_confirm_yes(move || {
            let k = a.page.confirm_key.borrow_mut().take();
            each!(a, |g| g.set_confirm(false));
            if let Some((id, key)) = k {
                let key = key.or_else(|| id.and_then(|i| a.hover.sessions.get(i)).map(|s| s.key.clone()));
                if let Some(key) = key { a.hover.sessions.delete(&key); if let Some(h) = &a.hover.history { h.delete(&key); } }
                if id.is_some() && id == a.page.open.get() { a.close_drawer(); }
                a.page.history_sent.set(usize::MAX);
                a.office_changed();
                a.office_widgets();
            }
        });
        let a = self.clone();
        g.on_d_close(move || a.close_drawer());
        let a = self.clone();
        g.on_d_delete(move || { if let Some(s) = a.page.open.get().and_then(|id| a.hover.sessions.get(id)) { a.ask_delete(Some(s.id), None, &s.title(), s.busy()); } });
        let a = self.clone();
        g.on_d_send(move || {
            let Some(id) = a.page.open.get() else { return };
            let text = each_reply(&a).trim().to_owned();
            if text.is_empty() {
                if a.hover.sessions.get(id).is_some_and(|s| s.busy()) { a.hover.sessions.stop(id); }
                return;
            }
            if !a.hover.sessions.reply(id, &text, vec![]) { a.toast("3 tasks are running. Reply when one is done."); return; }
            each!(a, |g| g.set_d_draft(s("")));
            if let Some(c) = &mut *a.page.thread.borrow_mut() { c.scroll = f32::MAX; }
            a.office_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_d_wheel(move |dy| { if let Some(c) = &mut *a.page.thread.borrow_mut() { c.scroll = (c.scroll - dy).max(0.0); } a.paint_thread(); });
    }

    fn ask_delete(self: &Rc<Self>, id: Option<i32>, key: Option<String>, title: &str, busy: bool) {
        *self.page.confirm_key.borrow_mut() = Some((id, key));
        let text = format!("“{title}” goes from the office and the history{}. This can’t be undone.", if busy { ", and its run is stopped" } else { "" });
        each!(self, |g| { g.set_confirm_text(s(&text)); g.set_confirm(true); });
    }
}

fn each_draft(a: &App) -> String {
    let n = a.notch.global::<crate::ui::Office>().get_new_draft().to_string();
    if !n.trim().is_empty() { return n; }
    a.dash.borrow().as_ref().map(|d| d.global::<crate::ui::Office>().get_new_draft().to_string()).unwrap_or_default()
}

fn each_reply(a: &App) -> String {
    let n = a.notch.global::<crate::ui::Office>().get_d_draft().to_string();
    if !n.trim().is_empty() { return n; }
    a.dash.borrow().as_ref().map(|d| d.global::<crate::ui::Office>().get_d_draft().to_string()).unwrap_or_default()
}

/// ago(): "now", "5 min ago", "3 h ago", "2 d ago".
pub fn ago(ms: f64) -> String {
    let m = (ms / 60e3).round() as i64;
    if m < 1 { "now".into() } else if m < 60 { format!("{m} min ago") } else if m < 24 * 60 { format!("{} h ago", (m as f64 / 60.0).round()) } else { format!("{} d ago", (m as f64 / 1440.0).round()) }
}

/// day(): Today, Yesterday, This week, else the month and year.
fn day(now: f64, ms: f64) -> String {
    let off = hover_core::time::local_offset_min(hover_core::time::Stamp::now().ticks) as f64 * 60e3;
    let today = ((now + off) / 864e5).floor() * 864e5 - off;
    let k = (today + 864e5 - ms) / 864e5;
    if ms >= today { "Today".into() } else if k < 2.0 { "Yesterday".into() } else if k < 7.0 { "This week".into() } else {
        let st = hover_core::time::Stamp::from_unix_ms(ms as i64, hover_core::time::Kind::Utc).iso();
        let (y, mo) = (&st[0..4], st[5..7].parse::<usize>().unwrap_or(1));
        format!("{} {y}", ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"][mo - 1])
    }
}

/// The clock's tooltip: the long local date and time.
pub fn full_date() -> String {
    let t = hover_core::time::Stamp::now();
    let secs = (t.ticks - 621_355_968_000_000_000) / 10_000_000 + hover_core::time::local_offset_min(t.ticks) * 60;
    let days = secs.div_euclid(86400);
    let st = hover_core::time::Stamp::from_unix_ms(days * 86_400_000, hover_core::time::Kind::Utc).iso();
    let (y, mo, d) = (&st[0..4], st[5..7].parse::<usize>().unwrap_or(1), st[8..10].trim_start_matches('0').to_owned());
    let wd = ["Thursday", "Friday", "Saturday", "Sunday", "Monday", "Tuesday", "Wednesday"][days.rem_euclid(7) as usize];
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let r = secs.rem_euclid(86400);
    format!("{wd} {d} {} {y} at {:02}:{:02}:{:02}", months[mo - 1], r / 3600, r / 60 % 60, r % 60)
}

/// backdrop-filter: blur(18px) saturate(1.4), at a quarter of the size (a blur that
/// wide loses nothing at that scale): three box passes each way make it near Gaussian
/// (sigma 18 px is a box of about 17 px at full size, 4 at a quarter).
fn blur(rgb: &[u8], w: usize, h: usize) -> SharedPixelBuffer<Rgba8Pixel> {
    let (sw, sh) = (w.div_ceil(4), h.div_ceil(4));
    let mut small = vec![[0f32; 3]; sw * sh];
    for y in 0..sh { for x in 0..sw {
        let mut acc = [0f32; 3];
        let mut n = 0.0;
        for dy in 0..4 { for dx in 0..4 {
            let (px, py) = ((x * 4 + dx).min(w - 1), (y * 4 + dy).min(h - 1));
            let i = (py * w + px) * 3;
            for k in 0..3 { acc[k] += rgb[i + k] as f32; }
            n += 1.0;
        } }
        small[y * sw + x] = acc.map(|v| v / n);
    } }
    let r = 2i64;
    for _ in 0..3 {
        for horizontal in [true, false] {
            let src = small.clone();
            for y in 0..sh { for x in 0..sw {
                let mut acc = [0f32; 3];
                for d in -r..=r {
                    let (px, py) = if horizontal { ((x as i64 + d).clamp(0, sw as i64 - 1) as usize, y) } else { (x, (y as i64 + d).clamp(0, sh as i64 - 1) as usize) };
                    for k in 0..3 { acc[k] += src[py * sw + px][k]; }
                }
                small[y * sw + x] = acc.map(|v| v / (2 * r + 1) as f32);
            } }
        }
    }
    let mut out = SharedPixelBuffer::<Rgba8Pixel>::new(sw as u32, sh as u32);
    for (d, s) in out.make_mut_slice().iter_mut().zip(&small) {
        // saturate(1.4), with the filter's luminance weights.
        let l = 0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2];
        let c = s.map(|v| (l + (v - l) * 1.4).clamp(0.0, 255.0) as u8);
        *d = Rgba8Pixel { r: c[0], g: c[1], b: c[2], a: 255 };
    }
    out
}
