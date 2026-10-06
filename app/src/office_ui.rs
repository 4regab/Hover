//! Owl/KiroPage.cs without the web view: the office (hover-office, on its own thread)
//! fed with Hover's state (KiroPage.Push, every change, at most every 120 ms), and the
//! page's own logic around it: tags, the HUD, the new-task circle, the panels, the chat
//! drawer (hover-chat's thread), the toast, deleting after asking.

use crate::ui::*;
use crate::App;
use hover_agents::ask::AskAnswer;
use hover_agents::session::KiroSession;
use hover_core::model::AgentTool;
use hover_office::bot::Stage;
use hover_office::live::{In, Live};
use hover_office::office::{Click, Time};
use slint::{Color, ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel};
use std::collections::HashMap;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

const TOOLS: [(&str, &str, u32); 5] = [("kiro", "Kiro", 0xb48cff), ("codex", "Codex", 0x3fd6a0), ("cursor", "Cursor", 0x7cc0ff), ("opencode", "OpenCode", 0xe8e8ec), ("claude", "Claude Code", 0xd97757)];
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
    pub(crate) fab: Cell<i32>,
    new_tool: Cell<usize>,
    /// The HUD's menu, and the new-task box's access menu, open.
    pub(crate) menu: Cell<bool>,
    pub(crate) access_menu: Cell<bool>,
    /// The access the new-task box picked, by tool, for the tasks it starts next.
    new_access: RefCell<[Option<&'static str>; AgentTool::ALL.len()]>,
    new_folder: RefCell<Option<String>>,
    /// The new-task box's Kiro Web switch and repo.
    cloud: RefCell<NewCloud>,
    /// The user's Kiro Web sessions that Hover doesn't have yet, listed each time the history opens.
    web: RefCell<WebList>,
    time_mode: Cell<i32>,
    toast_timer: slint::Timer,
    push_timer: slint::Timer,
    dirty: Cell<bool>,
    /// A tool whose status is being looked up (office_push), by AgentTool::ALL's order.
    checking: Cell<[bool; AgentTool::ALL.len()]>,
    confirm_key: RefCell<Option<(Option<i32>, Option<String>)>>,
    /// The question on the card is a rewind (a chat and its folder going back), not a delete.
    confirm_rewind: RefCell<Option<(i32, hover_agents::session::Rewind)>>,
    /// A rewind is putting the files back: another click waits.
    rewinding: Cell<bool>,
    thread: RefCell<Option<Chat>>,
    /// The open chat's turns as last laid out, for a click on the thread.
    turns: RefCell<Vec<hover_chat::Turn>>,
    /// Each chat's unsent reply and its pictures, kept while another chat is open.
    drafts: RefCell<HashMap<i32, (String, Vec<String>)>>,
    copied: slint::Timer,
    rows_open: RefCell<Vec<(Option<i64>, Option<String>)>>,
    pub target: Cell<i32>,
    shown: Cell<Option<bool>>,
    drop_timer: slint::Timer,
    view: Cell<Option<[f64; 3]>>,
    /// qPick: what has been picked and typed for each question, by its ask's id, so a
    /// redraw keeps it; and each question's rows, kept so their buttons stay (a model
    /// made anew loses a press between down and up).
    picks: RefCell<HashMap<String, Picks>>,
    qmodels: RefCell<HashMap<String, QRows>>,
    /// The model menu: 0 closed, 1 the drawer's pill, 2 the new-task box's.
    pub(crate) model_menu: Cell<i32>,
    /// Pictures attached to the reply (0) and the new task (1), as files in kiro-images.
    attached: RefCell<[Vec<String>; 2]>,
    thumbs: RefCell<HashMap<String, Image>>,
    /// The frame and its blurred copy as textures of their own on the windows' GPU
    /// device (Windows, where the office shares it): written in place each frame, so
    /// the windows draw them as they are. A new image each frame made each window
    /// upload a texture of its own for it, per element that shows it, every time it drew.
    #[cfg(windows)]
    gpu: RefCell<[Option<(slint::wgpu_30::wgpu::Texture, Image)>; 2]>,
    /// The frame as RGBA, for the texture, kept between frames.
    #[cfg(windows)]
    scratch: RefCell<Vec<u8>>,
    /// The office's own slot textures as Slint images, when the frame stays on the GPU:
    /// made once per slot, and again only when a resize gives the office new ones.
    #[cfg(windows)]
    slots: RefCell<SlotImages>,
    /// The glass panels' blurred copy of the frame, and its working buffers.
    blur: RefCell<Blur>,
    /// A new task's workspace is being made (a worktree can take a while): another Start waits.
    starting: Cell<bool>,
    /// What the chat's note strip and its More menu do, by position (chat_note, chat_more).
    note_acts: RefCell<Vec<&'static str>>,
    more_acts: RefCell<Vec<String>>,
    /// Expand chat: the open chat fills the app window (the notch never shows it so).
    pub wide: Cell<bool>,
    /// What each row of the expanded chat's session list opens (a live session by id, a saved one by key).
    rows_list: RefCell<Vec<(Option<i32>, Option<String>)>>,
    /// The open chat's workspace branch, for the expanded chat's header.
    branch: RefCell<Branch>,
    /// The desk card and the desk panel (desk_ui.rs).
    pub desk: crate::desk_ui::DeskUi,
}

/// A folder's branch as last looked up (off the UI thread; again after 10 s).
#[derive(Default)]
struct Branch { folder: String, label: String, head: bool, at: Option<Instant>, looking: bool }

/// The office's slots as images, and which slot each window last showed (0 the notch, 1
/// the app window). A window holds its slot until it is given another, so the office
/// never draws over a texture a window still shows.
#[cfg(windows)]
#[derive(Default)]
struct SlotImages { gen: u64, img: Vec<(Image, Image)>, held: [Option<usize>; 2] }

/// A question's rows, and each of its questions' choices.
type QRows = (Rc<VecModel<QData>>, Vec<Rc<VecModel<QOpt>>>);

/// One question's picks: the labels picked for each of its questions, and the words typed.
#[derive(Clone, Default)]
struct Picks { sel: Vec<Vec<String>>, text: Vec<String> }

/// The drawer's thread, laid out and painted by hover-chat.
/// laid: the session's change number and clock second its turns were read at, the
/// thread's width and height then (None: not yet).
struct Chat { id: i32, thread: hover_chat::Thread, painter: hover_chat::Painter, scroll: f32, width: f32, key: String, laid: Option<(u64, i64, f32, f32)>, sel: Sel }

/// A text selection being made in the thread with the pointer (chat-proto's): where it
/// began, by what unit a double or triple click grows it, and the click count.
struct Sel {
    anchor: Option<hover_chat::Pos>,
    unit: hover_chat::Unit,
    unit_anchor: (hover_chat::Pos, hover_chat::Pos, hover_chat::Tail),
    dragging: bool,
    last: Option<(Instant, f32, f32)>,
    clicks: u32,
}

impl Default for Sel {
    fn default() -> Self {
        let p = hover_chat::Pos { section: 0, text: 0, byte: 0 };
        Sel { anchor: None, unit: hover_chat::Unit::Char, unit_anchor: (p, p, hover_chat::Tail::None), dragging: false, last: None, clicks: 0 }
    }
}

impl Sel {
    /// 1, 2, 3…: a press within the system's double-click time and distance of the last.
    fn press(&mut self, x: f32, y: f32) -> u32 {
        let (time, (w, h)) = double_click();
        let near = self.last.is_some_and(|(t, lx, ly)| t.elapsed() <= time && (x - lx).abs() <= w / 2.0 && (y - ly).abs() <= h / 2.0);
        self.clicks = if near { self.clicks + 1 } else { 1 };
        self.last = Some((Instant::now(), x, y));
        self.clicks
    }
}

/// GetDoubleClickTime and SM_CXDOUBLECLK / SM_CYDOUBLECLK (as chat-proto reads them).
#[cfg(windows)]
fn double_click() -> (Duration, (f32, f32)) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK};
    unsafe { (Duration::from_millis(GetDoubleClickTime() as u64), (GetSystemMetrics(SM_CXDOUBLECLK) as f32, GetSystemMetrics(SM_CYDOUBLECLK) as f32)) }
}

/// GTK's and Qt's defaults (400 ms, 5 px): X11 has no setting of its own, and on a Mac this
/// stands for the system's.
#[cfg(not(windows))]
fn double_click() -> (Duration, (f32, f32)) { (Duration::from_millis(400), (10.0, 10.0)) }

/// Whether a repository ("owner/name") matches what was typed in a repo menu's search box:
/// any part of it, capitals ignored. Nothing typed matches all.
pub(crate) fn repo_matches(name: &str, query: &str) -> bool {
    let q = query.trim();
    q.is_empty() || name.to_lowercase().contains(&q.to_lowercase())
}

/// Who is signed in to Kiro, as a SHA-256 of what `kiro-cli whoami` printed (so the
/// saved repo list names no one). None while Kiro isn't signed in. Blocks.
fn repo_account() -> Option<String> {
    use sha2::{Digest, Sha256};
    hover_agents::agents::check(AgentTool::Kiro, false);
    let said = hover_agents::agents::said(AgentTool::Kiro)?;
    Some(Sha256::digest(said.trim().as_bytes()).iter().map(|b| format!("{b:02x}")).collect())
}

/// The connected repos saved last time (repos.json), if they are `account`'s.
fn saved_repos(account: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(hover_core::paths::support().join("repos.json")).ok()?).ok()?;
    if v["account"].as_str() != Some(account) { return None; }
    Some(v["repos"].as_array()?.iter().filter_map(|r| r.as_str().map(str::to_owned)).collect())
}

/// Saves the connected repos for next time. ponytail: one account's list, written in
/// place; a half-written file only fails to read, and the list is asked of Kiro again.
fn save_repos(account: &str, repos: &[String]) {
    let body = serde_json::json!({ "account": account, "repos": repos }).to_string();
    if let Err(e) = hover_core::crypto::write_private(&hover_core::paths::support().join("repos.json"), body.as_bytes()) {
        hover_core::log::line(&format!("repos.json not saved: {e}"));
    }
}

fn fonts() -> Vec<Vec<u8>> { vec![hover_office::canvas::PIXELIFY.to_vec()] }

/// The Kiro Web sessions in the user's Kiro account that the history shows beside Hover's own.
#[derive(Default)]
struct WebList {
    listing: bool,
    list: Vec<hover_agents::acp::CloudSession>,
    /// Why they couldn't be listed this time.
    error: Option<String>,
    /// When the list is empty though Kiro answered: what it said, in words.
    note: String,
    /// The one being opened (its conversation read from the cloud).
    opening: Option<String>,
}

/// A Kiro Web session's row in the history: its id after this.
const WEB_ROW: &str = "kiro-web:";

/// The new-task box in Kiro Web (Kiro only).
#[derive(Default)]
struct NewCloud {
    on: bool,
    menu: bool,
    /// The connected GitHub repos, once Kiro has listed them (Err: why it couldn't).
    repos: Option<Result<Vec<String>, String>>,
    listing: bool,
    /// Who the list is for (repo_account), and when Kiro was last asked for it.
    account: Option<String>,
    asked: Option<Instant>,
    /// The repo picked in the menu: Some(None) is an empty workspace. None: the folder's own.
    pick: Option<Option<String>>,
    /// A folder and the GitHub repo its remote points at, once looked up.
    folder_repo: Option<(String, Option<String>)>,
    looking: Option<String>,
    /// What is typed in the repo menu's search box.
    query: String,
}

impl Default for Page {
    fn default() -> Page {
        Page { live: RefCell::new(None), size: Cell::new((0, 0)), open: Cell::new(None), panel: Cell::new(None), fab: Cell::new(0), new_tool: Cell::new(0), menu: Cell::new(false), access_menu: Cell::new(false), new_access: RefCell::new([None; AgentTool::ALL.len()]),
            new_folder: RefCell::new(None), cloud: Default::default(), web: Default::default(), time_mode: Cell::new(0), toast_timer: Default::default(), push_timer: Default::default(), dirty: Cell::new(true),
            checking: Cell::new([false; AgentTool::ALL.len()]), confirm_key: RefCell::new(None), confirm_rewind: RefCell::new(None), rewinding: Cell::new(false), thread: RefCell::new(None), turns: RefCell::new(vec![]), drafts: Default::default(), copied: Default::default(),
            rows_open: RefCell::new(vec![]), target: Cell::new(0), shown: Cell::new(None), drop_timer: Default::default(), view: Cell::new(None),
            picks: Default::default(), qmodels: Default::default(), model_menu: Cell::new(0), attached: Default::default(), thumbs: Default::default(),
            #[cfg(windows)] gpu: Default::default(),
            #[cfg(windows)] scratch: Default::default(),
            #[cfg(windows)] slots: Default::default(), blur: Default::default(),
            starting: Cell::new(false), note_acts: RefCell::new(vec![]), more_acts: RefCell::new(vec![]), wide: Cell::new(false), rows_list: RefCell::new(vec![]), branch: Default::default(), desk: Default::default() }
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
        let n = self.n.borrow();
        // Below a Mac's camera housing, when the notch has one.
        let below = n.hw.filter(|h| h.real).map_or(0.0, |h| h.height);
        let notch = || ((n.open_size.0 - 16.0) as u32, (n.open_size.1 - 16.0 - below).max(1.0) as u32, 0);
        // The open notch first: it sits over everything, so it is what the user looks at.
        // The app window used to win whenever it was open, even behind other windows or
        // minimised (Slint still calls both visible), and the notch's office froze.
        if n.hover.state != hover_notch::State::Rest { return Some(notch()); }
        if let Some(d) = &*self.dash.borrow() {
            if d.window().is_visible() {
                let sz = d.window().size();
                let k = d.window().scale_factor();
                return Some(((sz.width as f32 / k) as u32, (sz.height as f32 / k) as u32, 1));
            }
        }
        self.headless.then(notch)
    }

    /// Starts the office thread the first time an office is seen, and tells it whether
    /// it is in view (a hidden page draws nothing).
    pub fn office_follow(self: &Rc<Self>) {
        let want = self.office_size();
        let p = &self.page;
        if p.live.borrow().is_none() {
            let Some((w, h, _)) = want else { return };
            // The office's items, gone since it was dropped, are made again.
            each!(self, |g| g.set_built(true));
            let still = !self.look.get().animations;
            let live = Live::start(w, h, still, || crate::ui_do(|a| a.office_frame()));
            // The page made again: the camera where the user left it (office.view), and the
            // chat that was open.
            let view = p.view.get().or_else(|| crate::local_get("view").and_then(|v| {
                let n: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                (n.len() == 3 && n.iter().all(|x| x.is_finite())).then(|| [n[0], n[1], n[2]])
            }));
            if let Some(v) = view { live.send(In::View(v)); }
            // The time of day picked in the menu (office.time), which the scene made again
            // doesn't know: it would follow the clock while the menu still said Night.
            let tm = match p.time_mode.get() { 0 => match crate::local_get("time").as_deref() { Some("night") => 1, Some("day") => 2, _ => 0 }, m => m };
            p.time_mode.set(tm);
            if tm != 0 { live.send(In::Time(Some(if tm == 1 { Time::Night } else { Time::Day }))); }
            if let Some(id) = p.open.get() { live.send(In::Drawer(Some(id as i64))); }
            p.size.set((w, h));
            p.shown.set(None);
            *p.live.borrow_mut() = Some(live);
            p.dirty.set(true);
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
            if p.target.get() != which { hover_core::log::line(if which == 1 { "office: frames go to the app window" } else { "office: frames go to the notch" }); }
            p.target.set(which);
            if p.size.get() != (w, h) { p.size.set((w, h)); live.send(In::Resize(w, h)); }
        }
    }

    pub fn office_changed(&self) { self.page.dirty.set(true); self.desk_session_changed(); }

    /// The office thread goes, and its GPU memory with it.
    fn office_drop(self: &Rc<Self>) {
        let p = &self.page;
        if p.shown.get() != Some(false) { return; }
        p.push_timer.stop();
        self.desk_leave();
        *p.live.borrow_mut() = None;
        *p.thread.borrow_mut() = None;
        // The last frame and its blurred copy would otherwise stay in the globals.
        let clear = |g: crate::ui::Office| { g.set_scene(Image::default()); g.set_tags(ModelRc::default()); g.set_d_thread(Image::default()); g.set_built(false); };
        clear(self.notch.global::<crate::ui::Office>());
        self.notch.global::<crate::ui::Backdrop>().set_blurred(Image::default());
        if let Some(d) = &*self.dash.borrow() { clear(d.global::<crate::ui::Office>()); d.global::<crate::ui::Backdrop>().set_blurred(Image::default()); }
        // The office's items go when the window next draws (a minimised app window's go
        // when it is shown again).
        self.notch.window().request_redraw();
        #[cfg(windows)]
        { *p.gpu.borrow_mut() = Default::default(); *p.scratch.borrow_mut() = Vec::new(); *p.slots.borrow_mut() = Default::default(); }
        *p.blur.borrow_mut() = Blur::default();
        // The chat's copy of the open session's turns (made again when it is drawn), and
        // the thumbnails (read again from their files).
        *p.turns.borrow_mut() = Vec::new();
        p.thumbs.borrow_mut().clear();
        hover_core::log::line("office dropped after 30 s hidden");
        crate::bench::dropped();
        // glibc keeps what the office thread freed (its arena, the software GPU's
        // buffers) mapped; the drop is for the memory, so give it back once the thread
        // has gone.
        #[cfg(target_os = "linux")]
        slint::Timer::single_shot(Duration::from_secs(2), || unsafe { libc::malloc_trim(0); });
        // Windows: the office draws on the windows' GPU device, which frees the office's
        // textures and buffers only when it is next polled. mimalloc then gives back the
        // pages the office freed: it purges only while it allocates, and a resting notch
        // hardly does.
        #[cfg(windows)]
        slint::Timer::single_shot(Duration::from_secs(2), || {
            hover_office::render::flush_shared();
            unsafe { libmimalloc_sys::mi_collect(true) };
        });
    }

    /// KiroPage.Push: every session and what the page needs to show them. The office
    /// thread reads the desks from it (who sits where, each turn's stage and step count),
    /// never the answers, the steps' changes and output, or the history: those stay out.
    pub fn office_push(self: &Rc<Self>) {
        let p = &self.page;
        p.dirty.set(false);
        let hv = &self.hover;
        let sessions = hv.sessions.all_light();
        let folder = hv.settings.kiro_folder().filter(|f| hover_agents::usable_folder(Some(f)));
        let ready = |t: AgentTool| hover_agents::agents::known(t);
        let files = |_: &KiroSession| None;
        let o = hover_agents::state::Office { window: p.target.get() == 1, open: p.open.get(), settings: &hv.settings, folder: folder.clone(), history: None, ready: &ready, files: &files };
        let msg = hover_agents::state::push(&o, &sessions);
        if let Some(l) = &*p.live.borrow() { l.send(In::State(msg)); }
        if p.new_folder.borrow().is_none() { *p.new_folder.borrow_mut() = folder; }
        // Each tool's status is looked up once (agents::check keeps it five minutes); a
        // push every 120 ms used to start another thread for it until the first answered.
        for (i, t) in AgentTool::ALL.into_iter().enumerate() {
            if hover_agents::agents::known(t).is_none() && !p.checking.get()[i] {
                let mut c = p.checking.get();
                c[i] = true;
                p.checking.set(c);
                std::thread::spawn(move || { hover_agents::agents::check(t, false); crate::ui_do(move |a| { let mut c = a.page.checking.get(); c[i] = false; a.page.checking.set(c); a.office_changed(); }); });
            }
        }
        self.office_widgets();
    }

    /// A new frame from the office thread: the picture, the tags, the tooltip; the clicks.
    pub fn office_frame(self: &Rc<Self>) {
        let out = match &*self.page.live.borrow() { Some(l) => l.take(), None => return };
        if let Some(e) = &out.error { hover_core::log::line(&format!("office: {e}")); return; }
        let fresh = !out.rgb.is_empty() || out.slot.is_some();
        if !fresh && out.clicks.is_empty() { return; }
        if fresh { crate::bench::office_frame(); }
        if fresh && self.page.view.get() != Some(out.view) {
            self.page.view.set(Some(out.view));
            crate::local_set("view", &format!("{},{},{}", out.view[0], out.view[1], out.view[2]));
        }
        let mut out = out;
        // The office's own textures when the frame stayed on the GPU, else the picture it
        // read back. A slot whose textures Slint won't take leaves the office to read
        // frames back from now on, and this frame only brings its clicks.
        let images = match (fresh, out.slot) {
            (false, _) => None,
            (true, Some((gen, i))) => self.slot_images(gen, i),
            (true, None) => {
                let im = self.frame_images(&out.rgb, out.w, out.h);
                if let Some(l) = &*self.page.live.borrow() { l.recycle(std::mem::take(&mut out.rgb)); }
                Some(im)
            }
        };
        if let Some((img, blurred)) = images {
            // renderAsks: the question over the head while the session waits.
            let asking = self.hover.sessions.asking_now();
            // The card of a bot that asks shows the question itself, with the same buttons:
            // its bubble over the head would sit half under the card.
            let card = self.page.desk.card.get();
            let tags: Vec<TagData> = out.tags.iter().map(|t| {
                let ask = asking.iter().find(|x| x.0 as i64 == t.id && card != Some(x.0)).map(|(_, a, n)| self.ask_data(a, *n));
                TagData {
                    id: t.id as i32, x: t.x as f32, y: t.y as f32, name: s(t.name), color: Color::from_rgb_u8(t.color[0], t.color[1], t.color[2]),
                    tool: s(tool_name(&t.tool)), tool_color: tool_color(&t.tool), text: s(&t.text), stage: t.stage as i32, hot: t.hot,
                    tool_id: s(&t.tool), asking: ask.is_some(), ask: ask.unwrap_or_default(),
                }
            }).collect();
            let hint = if out.hint == "clock" { full_date() } else { out.hint.clone() };
            let (tx, ty) = out.pointer.unwrap_or((0.0, 0.0));
            let which = self.page.target.get();
            // The tip over a bot ("Chat with Juno") or a desk with a session at it ("Juno’s desk"):
            // not while a desk card is open, nor over the bot whose chat is open.
            let over = {
                let find = |id: i64| out.tags.iter().find(|t| t.id == id).map(|t| (t.name, Color::from_rgb_u8(t.color[0], t.color[1], t.color[2])));
                let none = (0, "", Color::default());
                if self.page.desk.card.get().is_some() { none } else {
                    match out.hovered {
                        Some(hover_office::office::Hover::Bot(id)) if self.page.open.get() != Some(id as i32) => find(id).map_or(none, |(n, c)| (1, n, c)),
                        Some(hover_office::office::Hover::Desk(id)) => find(id).map_or(none, |(n, c)| (2, n, c)),
                        _ => none,
                    }
                }
            };
            self.desk_note_helpers(&out.tags);
            let set = |g: crate::ui::Office| {
                g.set_over_kind(over.0);
                g.set_over_name(s(over.1));
                g.set_over_color(over.2);
                g.set_scene(img.clone());
                if let Some(m) = crate::view::sync(g.get_tags(), &tags) { g.set_tags(m); }
                g.set_hint(s(&hint));
                g.set_tip_x(tx as f32);
                g.set_tip_y(ty as f32);
            };
            // A texture written in place is the same image as before: the window is told
            // to draw it again.
            if which == 1 {
                if let Some(d) = &*self.dash.borrow() { set(d.global::<crate::ui::Office>()); d.global::<crate::ui::Backdrop>().set_blurred(blurred); d.window().request_redraw(); }
            } else {
                set(self.notch.global::<crate::ui::Office>());
                self.notch.global::<crate::ui::Backdrop>().set_blurred(blurred);
                self.notch.window().request_redraw();
            }
            // This window now shows this slot; the one it showed before is free. Slint has
            // the new images, so no drawing it does from here on reads the old textures,
            // and any draw it already sent runs before the office's next write to them
            // (both go to the one shared queue, in order).
            if let Some((gen, i)) = out.slot { self.hold_slot(gen, i, which); }
        }
        for c in out.clicks {
            match c {
                Click::Open(id) => self.open_session(id as i32),
                Click::Desk(id, x, y) => self.desk_open_card(id as i32, x as f32, y as f32),
                Click::Panel(p) => self.open_panel(Some(p)),
                Click::Toast(_) => self.toast(&full_date()),
                Click::NewTask => { self.desk_leave(); self.close_drawer(); self.open_panel(None); self.page.fab.set(1); self.office_widgets(); }
                Click::Time(t) => { self.page.time_mode.set(if t == Time::Night { 1 } else { 2 }); crate::local_set("time", if t == Time::Night { "night" } else { "day" }); self.office_widgets(); }
                Click::Fold => self.collapse(),
                Click::Nothing => {
                    if self.page.fab.get() != 0 { self.page.fab.set(0); self.office_widgets(); }
                    else if self.page.open.get().is_some() { self.close_drawer(); }
                    else if self.page.panel.get().is_some() { self.open_panel(None); }
                }
            }
        }
    }

    /// A slot's page and glass textures as Slint images. The images are made once per set
    /// of slots, not per frame: the office draws into the very textures the windows
    /// sample, so nothing is copied here at all.
    #[cfg(windows)]
    fn slot_images(&self, gen: u64, i: usize) -> Option<(Image, Image)> {
        let live = self.page.live.borrow();
        let live = live.as_ref()?;
        let mut s = self.page.slots.borrow_mut();
        if s.gen != gen {
            let (have, tex) = live.slot_textures();
            // The office has moved on to another set already; this frame's slot is gone.
            if have != gen { return None; }
            let mut img = Vec::with_capacity(tex.len());
            for (page, glass) in tex {
                match (Image::try_from(page), Image::try_from(glass)) {
                    (Ok(a), Ok(b)) => img.push((a, b)),
                    (a, b) => {
                        let e = a.err().map(|e| e.to_string()).or_else(|| b.err().map(|e| e.to_string())).unwrap_or_default();
                        hover_core::log::line(&format!("office texture: {e}"));
                        live.send(In::NoGpu);
                        *s = SlotImages::default();
                        return None;
                    }
                }
            }
            *s = SlotImages { gen, img, held: [None; 2] };
        }
        s.img.get(i).cloned()
    }

    /// Only on Windows, where Slint and the office share one DX12 device. On Linux Slint draws
    /// with OpenGL, which cannot sample the office's wgpu texture, and on a Mac the office's
    /// frames are read back as well, so this is never reached there.
    #[cfg(not(windows))]
    fn slot_images(&self, _gen: u64, _i: usize) -> Option<(Image, Image)> { None }

    /// This window now shows slot `i`: the slot it showed before is handed back.
    #[cfg(windows)]
    fn hold_slot(&self, gen: u64, i: usize, which: i32) {
        let mut s = self.page.slots.borrow_mut();
        if s.gen != gen { return; }
        let w = (which == 1) as usize;
        let prev = s.held[w].replace(i);
        drop(s);
        if let (Some(prev), Some(l)) = (prev, &*self.page.live.borrow()) {
            if prev != i { l.release(gen, prev); }
        }
    }

    #[cfg(not(windows))]
    fn hold_slot(&self, _gen: u64, _i: usize, _which: i32) {}

    /// The frame and its blurred copy as images for the windows: textures written in
    /// place where the windows share the office's GPU device (Windows), else new pixel
    /// buffers.
    fn frame_images(&self, rgb: &[u8], w: u32, h: u32) -> (Image, Image) {
        let mut b = self.page.blur.borrow_mut();
        blur(rgb, w as usize, h as usize, &mut b);
        #[cfg(windows)]
        if let Some((device, queue)) = hover_office::render::shared() {
            let mut scratch = self.page.scratch.borrow_mut();
            scratch.clear();
            scratch.reserve(w as usize * h as usize * 4);
            for p in rgb.chunks(3) { scratch.extend_from_slice(&[p[0], p[1], p[2], 255]); }
            let mut gpu = self.page.gpu.borrow_mut();
            let [a, bl] = &mut *gpu;
            if let (Some(a), Some(bl)) = (upload(&device, &queue, a, w, h, &scratch), upload(&device, &queue, bl, b.w, b.h, &b.out)) {
                return (a, bl);
            }
        }
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
        for (d, s) in buf.make_mut_slice().iter_mut().zip(rgb.chunks(3)) { *d = Rgba8Pixel { r: s[0], g: s[1], b: s[2], a: 255 }; }
        (Image::from_rgba8(buf), Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&b.out, b.w, b.h)))
    }

    pub fn toast(self: &Rc<Self>, text: &str) {
        each!(self, |g| { g.set_toast(s(text)); g.set_toast_shown(true); });
        let a = self.clone();
        self.page.toast_timer.start(slint::TimerMode::SingleShot, Duration::from_millis(2800), move || each!(a, |g| g.set_toast_shown(false)));
    }

    pub(crate) fn send(&self, m: In) { if let Some(l) = &*self.page.live.borrow() { l.send(m); } }

    pub fn open_session(self: &Rc<Self>, id: i32) {
        self.desk_leave();
        self.page.fab.set(0);
        self.page.panel.set(None);
        self.send(In::Panel(None));
        self.keep_draft();
        self.page.open.set(Some(id));
        self.send(In::Drawer(Some(id as i64)));
        *self.page.thread.borrow_mut() = None;
        // This chat's own draft, never another's.
        let (text, pics) = self.page.drafts.borrow_mut().remove(&id).unwrap_or_default();
        self.page.attached.borrow_mut()[0] = pics;
        each!(self, |g| { g.set_d_draft(s(&text)); g.set_d_compose(false); });
        self.office_widgets();
    }

    pub fn close_drawer(self: &Rc<Self>) {
        self.keep_draft();
        self.page.wide.set(false);
        self.page.open.set(None);
        self.send(In::Drawer(None));
        *self.page.thread.borrow_mut() = None;
        each!(self, |g| { g.set_d_draft(s("")); g.set_d_compose(false); });
        self.office_widgets();
    }

    /// Expand chat: the open session, large, in the app window. It is the same session with
    /// the same draft, queue and scroll; only the window and the layout change.
    pub fn expand_chat(self: &Rc<Self>, id: i32) {
        if self.page.open.get() != Some(id) { self.open_session(id); }
        let draft = self.shown_draft();
        self.page.wide.set(true);
        self.open_dashboard(false);
        each!(self, |g| g.set_d_draft(draft.clone()));
        // A second Expand brings the window that has the chat to the front.
        if let Some(d) = &*self.dash.borrow() {
            use slint::winit_030::WinitWindowAccessor;
            d.window().set_minimized(false);
            d.window().with_winit_window(|w| w.focus_window());
        }
        self.office_widgets();
    }

    /// The reply being written is in the box of the window that shows the chat (each window has
    /// its own box); the window the user goes to gets it, so nothing typed is lost.
    fn shown_draft(&self) -> SharedString {
        if self.page.target.get() == 1 { if let Some(d) = &*self.dash.borrow() { return d.global::<crate::ui::Office>().get_d_draft(); } }
        self.notch.global::<crate::ui::Office>().get_d_draft()
    }

    /// Back to the office: the chat is the small drawer again, the details beside it are put away.
    pub fn collapse_chat(self: &Rc<Self>) {
        let draft = self.shown_draft();
        self.page.wide.set(false);
        each!(self, |g| g.set_d_draft(draft.clone()));
        self.desk_leave();
        self.office_widgets();
    }

    /// The branch (and whether it is a linked worktree) of the open chat's workspace, for the
    /// expanded chat's header. Git is asked off the UI thread; the last answer shows meanwhile.
    fn branch_of(self: &Rc<Self>, folder: &str) -> String {
        if !hover_agents::usable_folder(Some(folder)) { return String::new(); }
        let (label, start) = {
            let b = self.page.branch.borrow();
            (if b.folder == folder { b.label.clone() } else { String::new() }, !b.looking && (b.folder != folder || b.at.is_none_or(|t| t.elapsed() > Duration::from_secs(10))))
        };
        if start {
            self.page.branch.borrow_mut().looking = true;
            let folder = folder.to_owned();
            std::thread::Builder::new().name("branch".into()).spawn(move || {
                let (label, head) = match hover_agents::workspace::inspect(&folder) {
                    Ok(i) => (format!("{}{}", i.branch.unwrap_or_else(|| "detached HEAD".into()), if i.linked { " · worktree" } else { "" }), i.head.is_some()),
                    Err(_) => (String::new(), false),
                };
                crate::ui_do(move |a| {
                    let changed = { let mut b = a.page.branch.borrow_mut(); let c = b.folder != folder || b.label != label; *b = Branch { folder, label, head, at: Some(Instant::now()), looking: false }; c };
                    if changed { a.office_widgets(); }
                });
            }).ok();
        }
        label
    }

    /// The strip over the reply box: a usage limit the provider gave (Continue at the reset, Retry now, Snooze,
    /// Cancel), or replies held since Stop (Send them). The text, then the buttons as (action, label).
    fn chat_note(&self, o: &KiroSession) -> (String, Vec<(&'static str, &'static str)>) {
        use hover_agents::limit::Mode;
        let now = hover_core::time::Stamp::now().unix_ms();
        if let Some(l) = self.hover.limits.of(&o.key).filter(|l| !matches!(l.mode, Mode::Snoozed(u) if u > now)) {
            let at = l.limit.reset_at.map(|a| hover_agents::sched::civil_text(a, hover_agents::sched::Tz::Local));
            let said = l.limit.reason.trim().trim_end_matches('.').to_owned();
            return match (l.mode, at) {
                (Mode::Auto, Some(at)) => (format!("Usage limit: {said}. Hover continues this task after {at}."), vec![("retry", "Retry now"), ("cancel", "Cancel")]),
                (_, Some(at)) => (format!("Usage limit: {said}. It lifts at {at}."), vec![("arm", "Continue at the reset"), ("retry", "Retry now"), ("snooze", "Snooze"), ("cancel", "Cancel")]),
                (_, None) => (format!("Usage limit: {said}. The agent didn’t say when it lifts."), vec![("retry", "Retry now"), ("snooze", "Snooze"), ("cancel", "Cancel")]),
            };
        }
        if o.held && o.turns.iter().any(|t| t.queued) { return ("You stopped this run, so the replies waiting behind it are held.".into(), vec![("resume", "Send them now")]); }
        (String::new(), vec![])
    }

    /// The chat header's More menu: continue with another agent, fork, and bring a fork's findings back.
    /// (action id, label) for each. Nothing for a Kiro Web chat, which lives in Kiro's cloud.
    fn chat_more(&self, o: &KiroSession) -> Vec<(String, String)> {
        if o.cloud.is_some() { return vec![]; }
        let here = hover_agents::session::provider_id(o);
        let mut items = vec![];
        for t in AgentTool::ALL {
            if t.id() != here && hover_agents::agents::known(t).is_some_and(|r| r.ok()) { items.push((format!("to:{}", t.id()), format!("Continue with {}", t.name()))); }
        }
        for p in self.hover.customs.providers().into_iter().filter(|p| p.ready && p.id != here) { items.push((format!("to:{}", p.id), format!("Continue with {}", p.name))); }
        if o.turns.iter().any(|t| t.result.is_some() && !t.queued) { items.push(("fork".into(), "Fork this chat".into())); }
        if o.ext.lineage.as_ref().is_some_and(|l| l.fork.is_some()) { items.push(("back".into(), "Bring findings back to the original".into())); }
        items
    }

    /// A note strip button: what it does to the open chat.
    fn note_act(self: &Rc<Self>, i: usize) {
        let Some(id) = self.page.open.get() else { return };
        let Some(s) = self.hover.sessions.get(id) else { return };
        let act = self.page.note_acts.borrow().get(i).copied().unwrap_or("");
        let now = hover_core::time::Stamp::now().unix_ms();
        match act {
            "arm" => match self.hover.limits.arm(&s.key) { Ok(_) => {} Err(e) => self.toast(&e) },
            "retry" => if let Err(e) = self.hover.limits.retry_now(&s.key) { self.toast(&e); },
            "snooze" => { let until = self.hover.limits.of(&s.key).and_then(|l| l.limit.reset_at).filter(|a| *a > now).unwrap_or(now + 3_600_000); self.hover.limits.snooze(&s.key, until); }
            "cancel" => self.hover.limits.cancel(&s.key),
            "resume" => if !self.hover.sessions.resume_queue(id) { self.toast("A task is still running or every desk is busy. Try again in a moment."); },
            _ => {}
        }
        self.office_changed();
        self.office_widgets();
    }

    /// A More menu item: switch the open chat to another agent, fork it, or bring a fork's findings back.
    fn more_act(self: &Rc<Self>, i: usize) {
        each!(self, |g| g.set_d_more(false));
        let Some(id) = self.page.open.get() else { return };
        let Some(act) = self.page.more_acts.borrow().get(i).cloned() else { return };
        let Some(s) = self.hover.sessions.get(id) else { return };
        if let Some(to) = act.strip_prefix("to:") {
            let Some(target) = hover_agents::session::Target::parse(to) else { return };
            match self.hover.sessions.switch_provider(id, &target) {
                Ok(r) => self.toast(&match r.mode { "native" => "Back with its earlier conversation.".to_owned(), "fresh" => "Switched. Nothing had been said yet.".to_owned(), _ => format!("Switched. The new agent gets an account of the chat ({} message{}).", r.carried, if r.carried == 1 { "" } else { "s" }) }),
                Err(e) => self.toast(&e),
            }
        } else if act == "back" {
            match self.hover.sessions.bring_findings_back(&s.key, None) {
                Ok(0) => self.toast("Nothing new to bring back."),
                Ok(_) => self.toast("Brought the findings back as one message."),
                Err(e) => self.toast(&e),
            }
        } else if act == "fork" {
            self.fork_chat(&s);
        }
        self.office_changed();
        self.office_widgets();
    }

    /// Fork from the last ended turn, with the same agent, in a workspace of its own when the project is a Git one (as a new task gets).
    fn fork_chat(self: &Rc<Self>, s: &KiroSession) {
        let Some(turn) = s.turns.iter().rposition(|t| t.result.is_some() && !t.queued) else { self.toast("Nothing has ended yet to fork from."); return };
        let Some(target) = hover_agents::session::Target::parse(&hover_agents::session::provider_id(s)) else { return };
        if self.page.starting.replace(true) { return; }
        self.toast("Forking…");
        let source = s.ext.workspace.as_ref().map(|w| w.source.clone()).unwrap_or_else(|| s.folder.clone());
        let (key, title) = (s.key.clone(), s.title());
        let use_folder = self.hover.settings.automation().use_folder;
        let root = hover_core::paths::support().join("worktrees");
        std::thread::Builder::new().name("fork".into()).spawn(move || {
            use hover_agents::workspace::Choice;
            let choice = if use_folder { Choice::Folder } else { Choice::Own { base: None } };
            let made = hover_agents::workspace::prepare(&source, &choice, &format!("fork {title}"), &root, false, false, &hover_agents::cancel::Cancel::new());
            crate::ui_do(move |a| {
                a.page.starting.set(false);
                let p = match made { Ok(p) => p, Err(e) => { a.toast(&e); return; } };
                match a.hover.sessions.fork(&key, turn, &target, &p.folder, p.binding.clone()) {
                    Ok(f) => { a.toast("Forked. This is the copy; the original is unchanged."); a.office_changed(); a.open_session(f.id); }
                    Err(e) => {
                        if let Some(b) = p.binding.filter(|b| b.is_worktree()) { let _ = hover_agents::workspace::remove(&b, &p.folder, false, false, hover_agents::workspace::RemoveOpts { delete_branch: true, ..Default::default() }); }
                        a.toast(&e);
                    }
                }
            });
        }).ok();
    }

    /// The new-task box's line about where the task will work, from the same lookup as the branch.
    /// Empty until Git has answered. The tasks' choice itself is made in workspace::prepare.
    fn plan_line(self: &Rc<Self>, folder: &str, read_only: bool) -> String {
        if read_only { return "Read only: it looks at the folder itself.".into(); }
        if self.hover.settings.automation().use_folder { return "Works in the folder itself, as set in Settings.".into(); }
        let label = self.branch_of(folder);
        let b = self.page.branch.borrow();
        if b.folder != folder || b.at.is_none() { return String::new(); }
        if label.is_empty() || !b.head { return "Works in the folder itself: it isn’t a Git project with a commit yet.".into(); }
        format!("Gets its own worktree and branch, cut from {}.", label.trim_end_matches(" · worktree"))
    }

    /// Shots: a task typed into the new-task box for this folder, shown (`start` false) or started with Start.
    pub fn shot_new_task(self: &Rc<Self>, folder: &str, text: &str, start: bool) {
        *self.page.new_folder.borrow_mut() = Some(folder.into());
        self.page.new_tool.set(0);
        self.page.fab.set(2);
        each!(self, |g| g.set_new_draft(s(text)));
        self.office_widgets();
        if start { self.notch.global::<crate::ui::Office>().invoke_new_go_clicked(); }
    }

    /// The expanded chat's session list: the sessions open now, then the saved ones.
    fn list_rows(&self, sessions: &[KiroSession], open: Option<i32>) -> (Vec<ListRow>, Vec<(Option<i32>, Option<String>)>) {
        let stage_of = |s: &KiroSession| Stage::parse(hover_agents::state::stage(s.state, s.phase)) as i32;
        let (mut rows, mut opens) = (vec![], vec![]);
        let head = |text: &str| ListRow { head: true, text: s(text), tool: s(""), stage: 0, on: false };
        if !sessions.is_empty() {
            rows.push(head("Open now"));
            opens.push((None, None));
            for x in sessions {
                rows.push(ListRow { head: false, text: s(x.title()), tool: s(x.tool.id()), stage: stage_of(x), on: open == Some(x.id) });
                opens.push((Some(x.id), None));
            }
        }
        let mut saved: Vec<_> = self.hover.history.as_ref().map(|h| h.entries()).unwrap_or_default().into_iter().filter(|h| !sessions.iter().any(|x| x.key == h.key)).collect();
        saved.sort_by_key(|h| std::cmp::Reverse(h.updated.unix_ms()));
        // ponytail: the 30 newest; the history panel lists them all, with search.
        if !saved.is_empty() {
            rows.push(head("Saved"));
            opens.push((None, None));
            for h in saved.into_iter().take(30) {
                rows.push(ListRow { head: false, text: s(&h.title), tool: s(h.tool.id()), stage: Stage::parse(hover_agents::state::stage(h.state, hover_agents::stream::KiroPhase::Working)) as i32, on: false });
                opens.push((None, Some(h.key)));
            }
        }
        (rows, opens)
    }

    /// The folder in front in the office: the open chat's, else the new-task box's
    /// while it shows (voice's active project).
    pub fn selected_folder(&self) -> Option<String> {
        if let Some(id) = self.page.open.get() { return self.hover.sessions.get(id).map(|s| s.folder); }
        if self.page.fab.get() == 0 { return None; }
        self.page.new_folder.borrow().clone()
    }

    /// The open chat's reply, kept for when it opens again.
    fn keep_draft(&self) {
        let Some(id) = self.page.open.get() else { return };
        let text = each_reply(self);
        let pics = std::mem::take(&mut self.page.attached.borrow_mut()[0]);
        if text.trim().is_empty() && pics.is_empty() { self.page.drafts.borrow_mut().remove(&id); }
        else { self.page.drafts.borrow_mut().insert(id, (text, pics)); }
    }

    pub fn open_panel(self: &Rc<Self>, p: Option<&'static str>) {
        if p.is_some() { self.desk_leave(); self.page.wide.set(false); self.page.open.set(None); self.send(In::Drawer(None)); self.page.fab.set(0); }
        self.page.panel.set(p);
        self.send(In::Panel(p));
        if p == Some("history") { self.list_web(); }
        self.office_widgets();
    }

    /// Asks Kiro for the user's Kiro Web sessions, each time the history opens (the account may have
    /// changed), off the UI thread. The ones Hover already has are left out.
    fn list_web(self: &Rc<Self>) {
        if self.headless || self.page.web.borrow().listing { return; }
        // Not where Kiro isn't set up: it would be started to list nothing.
        if hover_agents::agents::known(AgentTool::Kiro).is_some_and(|r| !r.ok()) {
            let mut w = self.page.web.borrow_mut();
            (w.list, w.note, w.error) = (vec![], String::new(), Some("Kiro isn’t set up on this computer.".into()));
            return;
        }
        let Some(host) = self.hover.hosts.iter().find(|h| h.tool() == AgentTool::Kiro).cloned() else { return };
        self.page.web.borrow_mut().listing = true;
        let history = self.hover.history.clone();
        std::thread::spawn(move || {
            let got = host.cloud_sessions().map(|found| {
                // ponytail: every Kiro session in the history is read for its id, each time; an index of ids is the upgrade.
                let have: std::collections::HashSet<String> = history.map(|h| h.entries().into_iter().filter(|e| e.tool == AgentTool::Kiro)
                    .filter_map(|e| h.load(&e.key)).filter_map(|s| s.acp_id).collect()).unwrap_or_default();
                let total = found.sessions.len();
                let fresh: Vec<_> = found.sessions.into_iter().filter(|c| !have.contains(&c.id)).collect();
                // All of them already in Hover's history is not a failure.
                let note = if fresh.is_empty() && total > 0 { format!("All {total} are in Hover’s history already.") } else { found.note };
                (fresh, note)
            });
            crate::ui_do(move |a| {
                {
                    let mut w = a.page.web.borrow_mut();
                    w.listing = false;
                    // Another account's list never stays: a failure shows none.
                    match got { Ok((l, note)) => { w.list = l; w.note = note; w.error = None; } Err(e) => { w.list.clear(); w.note.clear(); w.error = Some(e); } }
                }
                a.office_widgets();
            });
        });
    }

    /// A Kiro Web session from the history's list: its conversation is read from the cloud, off the UI
    /// thread, and it comes to a desk as a chat (title and why, when it can't be read).
    fn open_web(self: &Rc<Self>, id: &str) {
        let Some(c) = self.page.web.borrow().list.iter().find(|c| c.id == id).cloned() else { return };
        if self.page.web.borrow().opening.is_some() { return; }
        if let Some(s) = self.hover.sessions.all().into_iter().find(|s| s.kiro_id.as_deref() == Some(id)) { return self.open_session(s.id); }
        let Some(host) = self.hover.hosts.iter().find(|h| h.tool() == AgentTool::Kiro).cloned() else { return };
        // It works in its own sandbox; here it has the default workspace, as a Kiro Web task started with no folder.
        let folder = match self.hover.settings.default_workspace().path().map(|p| hover_core::projects::ensure_folder(&p)) {
            Some(Ok(p)) => p.to_string_lossy().into_owned(),
            Some(Err(e)) => return self.toast(&e),
            None => return self.toast("Hover can’t find your home folder for the default workspace."),
        };
        self.page.web.borrow_mut().opening = Some(c.id.clone());
        self.toast("Opening it from Kiro Web…");
        self.office_widgets();
        std::thread::spawn(move || {
            let turns = host.cloud_transcript(&c.id, &folder);
            crate::ui_do(move |a| {
                a.page.web.borrow_mut().opening = None;
                match a.hover.sessions.adopt_cloud(&c.id, &c.title, &folder, c.updated, turns) {
                    Some(s) => {
                        a.page.web.borrow_mut().list.retain(|x| x.id != c.id);
                        a.office_changed();
                        a.open_session(s.id);
                    }
                    None => { a.toast("All six desks are busy. Stop or remove a session first."); a.office_widgets(); }
                }
            });
        });
    }

    /// The repo a Kiro Web task clones: the one picked, else the folder's own (None: an
    /// empty workspace). `wait` looks the folder's up now if it isn't known yet.
    fn cloud_repo(&self, folder: Option<&str>, wait: bool) -> Option<String> {
        let c = self.page.cloud.borrow();
        if let Some(p) = &c.pick { return p.clone(); }
        let f = folder?;
        if let Some((k, r)) = &c.folder_repo { if k == f { return r.clone(); } }
        drop(c);
        if !wait { return None; }
        let r = hover_agents::desk::Desk::shared().github_repo(f);
        self.page.cloud.borrow_mut().folder_repo = Some((f.to_owned(), r.clone()));
        r
    }

    /// The connected repos Kiro listed (empty until it has), and what to say instead while
    /// there are none: still loading, why it couldn't, or that none are connected.
    pub(crate) fn connected_repos(&self) -> (Vec<String>, String) {
        let c = self.page.cloud.borrow();
        match &c.repos {
            // The list there is stays while Kiro is asked again.
            Some(Ok(l)) if !l.is_empty() => (l.clone(), if c.listing { "Checking for new repositories…".into() } else { String::new() }),
            _ if c.listing => (vec![], "Loading your connected repositories…".to_owned()),
            Some(Err(e)) => (vec![], e.clone()),
            Some(Ok(_)) => (vec![], "No GitHub repositories are connected. Connect GitHub in Kiro Web.".into()),
            None => (vec![], String::new()),
        }
    }

    /// Lists the connected repos off the UI thread, then runs `done` (again once Kiro has
    /// answered). The list saved last time shows first, if it is for the account signed in
    /// now. Kiro is asked every time, so a repository made since shows up; a failure keeps
    /// the list there was. Kiro starts if it isn't up. The office's repo menu shares the list.
    pub(crate) fn load_repos(self: &Rc<Self>, done: fn(&Rc<Self>)) {
        if self.headless { return; }
        let had = {
            let mut c = self.page.cloud.borrow_mut();
            if c.listing { return; }
            c.listing = true;
            c.asked = Some(Instant::now());
            c.account.clone()
        };
        let host = self.hover.hosts.iter().find(|h| h.tool() == AgentTool::Kiro).cloned();
        std::thread::spawn(move || {
            let who = repo_account();
            // The first time this run, or after signing in as someone else: the saved list, or none.
            if who.is_some() && who != had {
                let (w, saved) = (who.clone(), who.as_deref().and_then(saved_repos));
                crate::ui_do(move |a| { { let mut c = a.page.cloud.borrow_mut(); c.account = w; c.repos = saved.map(Ok); } done(a); });
            }
            let got = host.map_or_else(|| Err("Kiro isn’t set up.".to_owned()), |h| h.repos());
            if let (Ok(l), Some(w)) = (&got, &who) { save_repos(w, l); }
            crate::ui_do(move |a| {
                {
                    let mut c = a.page.cloud.borrow_mut();
                    c.listing = false;
                    if got.is_ok() || !matches!(c.repos, Some(Ok(_))) { c.repos = Some(got); }
                }
                done(a);
            });
        });
    }

    /// A search that matches none of the repos asks Kiro again (at most every 15 s): the
    /// repository may have been made since the list was.
    pub(crate) fn repos_missed(self: &Rc<Self>, q: &str, done: fn(&Rc<Self>)) {
        let miss = {
            let c = self.page.cloud.borrow();
            !q.trim().is_empty() && c.asked.is_none_or(|t| t.elapsed() > Duration::from_secs(15))
                && matches!(&c.repos, Some(Ok(l)) if !l.iter().any(|r| repo_matches(r, q)))
        };
        if miss { self.load_repos(done); }
    }

    /// The shots' history: Kiro Web sessions as Kiro would list them, without Kiro.
    pub fn web_shot(self: &Rc<Self>, list: Vec<hover_agents::acp::CloudSession>, note: &str) {
        *self.page.web.borrow_mut() = WebList { list, note: note.into(), ..Default::default() };
        self.office_widgets();
    }

    /// The shots' Kiro Web box: a repo picked and the list Kiro would give, without Kiro.
    pub fn cloud_shot(self: &Rc<Self>, pick: Option<&str>, repos: Vec<String>) {
        { let mut c = self.page.cloud.borrow_mut(); c.pick = Some(pick.map(str::to_owned)); c.repos = Some(Ok(repos)); }
        self.office_widgets();
    }

    /// Looks up the folder's GitHub repo off the UI thread, once per folder.
    fn cloud_look(&self, folder: Option<&str>) {
        let Some(f) = folder.map(str::to_owned) else { return };
        let mut c = self.page.cloud.borrow_mut();
        if c.folder_repo.as_ref().is_some_and(|(k, _)| *k == f) || c.looking.as_deref() == Some(f.as_str()) { return; }
        c.looking = Some(f.clone());
        std::thread::spawn(move || {
            let r = hover_agents::desk::Desk::shared().github_repo(&f);
            crate::ui_do(move |a| {
                { let mut c = a.page.cloud.borrow_mut(); c.looking = None; c.folder_repo = Some((f, r)); }
                a.office_widgets();
            });
        });
    }

    /// The repo menu's rows (no repo first, then the folder's, then the connected ones that
    /// match the search) and its note.
    fn repo_rows(&self, current: Option<&str>, folder_repo: Option<&str>) -> (Vec<AccessOpt>, String) {
        let c = self.page.cloud.borrow();
        let q = c.query.as_str();
        let mut names: Vec<&str> = folder_repo.into_iter().collect();
        if let Some(Ok(list)) = &c.repos { for r in list { if !names.contains(&r.as_str()) { names.push(r); } } }
        let mut rows = vec![AccessOpt { id: s(""), label: s("Empty workspace"), note: s("No repository: the agent starts in an empty folder."), on: current.is_none() }];
        rows.extend(names.iter().filter(|r| Some(**r) == folder_repo || repo_matches(r, q)).map(|r| AccessOpt { id: s(*r), label: s(*r), note: s(if Some(*r) == folder_repo { "This folder’s repository" } else { "" }), on: current == Some(*r) }));
        // The list there is stays while Kiro is asked again.
        let checking = if c.listing { " Checking for new repositories…" } else { "" };
        let note = match &c.repos {
            Some(Ok(l)) if !l.is_empty() && !l.iter().any(|r| repo_matches(r, q)) => format!("No repository matches “{}”.{checking}", q.trim()),
            Some(Ok(l)) if !l.is_empty() => checking.trim_start().to_owned(),
            _ if c.listing => "Loading your connected repositories…".to_owned(),
            Some(Err(e)) => e.clone(),
            Some(Ok(_)) => "No GitHub repositories are connected. Connect GitHub in Kiro Web.".into(),
            _ => String::new(),
        };
        (rows, note)
    }

    /// Everything around the scene, from the state and the page's own state.
    pub fn office_widgets(self: &Rc<Self>) {
        let p = &self.page;
        let sessions = self.hover.sessions.all_light();
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
        let plan_note = |a: &Rc<Self>| -> String { match (&folder, tool == AgentTool::Kiro && p.cloud.borrow().on) { (Some(f), false) => a.plan_line(f, self.new_access(nt) == "read"), _ => String::new() } };
        let note = if !ready { hover_agents::agents::known(tool).map(|r| r.hint).unwrap_or_default() } else if !can { "3 tasks are running. Start another when one is done.".into() } else if full { "All six desks are busy. Stop or remove a session first.".into() } else { plan_note(self) };
        // The panel's rows.
        let (title, sub, rows, opens) = self.panel_rows(&sessions);
        *p.rows_open.borrow_mut() = opens;
        let open = p.open.get().and_then(|id| sessions.iter().find(|s| s.id == id).cloned());
        let summary = if sessions.is_empty() { "The office is quiet.".to_owned() } else { format!("{} session{} in the office", sessions.len(), if sessions.len() == 1 { "" } else { "s" }) };
        let fab = p.fab.get();
        let time_mode = p.time_mode.get();
        let beats = self.beats.want();
        let (menu, access_menu) = (p.menu.get(), p.access_menu.get());
        let cloud_shown = tool == AgentTool::Kiro;
        let cloud_on = cloud_shown && p.cloud.borrow().on;
        if cloud_on { self.cloud_look(folder.as_deref()); }
        let repo = if cloud_on { self.cloud_repo(folder.as_deref(), false) } else { None };
        let folder_repo = p.cloud.borrow().folder_repo.as_ref().filter(|(k, _)| Some(k) == folder.as_ref()).and_then(|(_, r)| r.clone());
        let repo_menu = cloud_on && p.cloud.borrow().menu;
        let (repo_opts, repo_note) = if repo_menu { self.repo_rows(repo.as_deref(), folder_repo.as_deref()) } else { (vec![], String::new()) };
        // Kiro Web has no asking: its tasks are Full.
        let acc = if cloud_on { "full" } else { self.new_access(nt) };
        let access_opts: Vec<AccessOpt> = ACCESS.iter().filter(|(id, ..)| *id != "read" || hover_agents::agents::read_only_works(tool))
            .map(|(id, label, _)| AccessOpt { id: s(*id), label: s(*label), note: s(access_note(id, tool)), on: *id == acc }).collect();
        // renderPill for the box's tool and the open chat's, and the menu of the one open.
        let n_pill = self.pill(tool);
        let d_pill = open.as_ref().map(|o| self.pill(o.tool));
        let mm = match p.model_menu.get() { 1 => open.as_ref().map(|o| o.tool), 2 => Some(tool), _ => None };
        let menu_rows = mm.map(|t| self.model_rows(t));
        let notice = !self.hover.settings.kiro_notice_seen();
        let shots: Vec<Vec<Image>> = p.attached.borrow().iter().map(|l| l.iter().map(|f| self.thumb(f)).collect()).collect();
        // The strip over the reply box, and the header's More menu, for the open chat.
        let (cnote, note_btns, more) = match &open {
            Some(o) => { let (t, b) = self.chat_note(o); (t, b, self.chat_more(o)) }
            None => (String::new(), vec![], vec![]),
        };
        *p.note_acts.borrow_mut() = note_btns.iter().map(|b| b.0).collect();
        *p.more_acts.borrow_mut() = more.iter().map(|m| m.0.clone()).collect();
        let note_labels: Vec<SharedString> = note_btns.iter().map(|b| s(b.1)).collect();
        let more_items: Vec<MOpt> = more.iter().map(|m| MOpt { id: s(&m.0), label: s(&m.1), on: false }).collect();
        // The expanded chat's header and list; only the app window shows it large.
        let wide = p.wide.get() && open.is_some();
        let (list_rows, list_opens) = if wide { self.list_rows(&sessions, p.open.get()) } else { (vec![], vec![]) };
        *p.rows_list.borrow_mut() = list_opens;
        let (status, stage, branch) = match &open {
            Some(o) if wide => {
                let st = Stage::parse(hover_agents::state::stage(o.state, o.phase));
                (if o.stopping { "Stopping…" } else { st.word() }, st as i32, if o.cloud.is_some() { String::new() } else { self.branch_of(&o.folder) })
            }
            _ => ("", 0, String::new()),
        };
        let acc_label = access_label(acc);
        let acc_tip = if cloud_on { format!("{acc_label}: Kiro Web runs every task with full access.") } else { format!("{acc_label}: {} Click to change.", access_note(acc, tool)) };
        each!(self, |g| {
            g.set_notice(notice);
            g.set_n_model(s(&n_pill.0));
            g.set_n_model_effort(s(&n_pill.1));
            g.set_n_model_shown(n_pill.2);
            if let Some(d) = &d_pill { g.set_d_model(s(&d.0)); g.set_d_model_effort(s(&d.1)); g.set_d_model_shown(d.2); }
            g.set_model_menu(if menu_rows.is_some() { p.model_menu.get() } else { 0 });
            if let Some((head, models, ehead, efforts, note)) = &menu_rows {
                g.set_mm_head(s(head));
                g.set_mm_cur(models.iter().position(|m| m.on).unwrap_or(0) as i32);
                if let Some(m) = crate::view::sync(g.get_mm_models(), models) { g.set_mm_models(m); }
                g.set_mm_effort_head(s(ehead));
                if let Some(m) = crate::view::sync(g.get_mm_efforts(), efforts) { g.set_mm_efforts(m); }
                g.set_mm_note(s(note));
            }
            if let Some(m) = crate::view::sync(g.get_d_shots(), &shots[0]) { g.set_d_shots(m); }
            if let Some(m) = crate::view::sync(g.get_n_shots(), &shots[1]) { g.set_n_shots(m); }
            g.set_menu(menu);
            g.set_access_menu(access_menu);
            g.set_access_head(s(format!("{} may", tool.name()).to_uppercase()));
            if let Some(m) = crate::view::sync(g.get_access_opts(), &access_opts) { g.set_access_opts(m); }
            g.set_new_access(s(acc_label));
            g.set_new_access_full(acc == "full");
            g.set_new_access_tip(s(&acc_tip));
            g.set_new_cloud_shown(cloud_shown);
            g.set_new_cloud(cloud_on);
            g.set_new_repo(s(repo.as_deref().unwrap_or("Empty workspace")));
            g.set_repo_menu(repo_menu);
            if let Some(m) = crate::view::sync(g.get_repo_opts(), &repo_opts) { g.set_repo_opts(m); }
            g.set_repo_note(s(&repo_note));
            if let Some(m) = crate::view::sync(g.get_tools(), &tools) { g.set_tools(m); }
            g.set_new_tool(nt as i32);
            g.set_fab(fab);
            g.set_new_folder(s(folder.as_deref().map(hover_office::office::short).unwrap_or_else(|| "Choose a folder".into())));
            g.set_new_note(s(&note));
            g.set_new_go(ready && can && !full && (folder.is_some() || cloud_on) && (!g.get_new_draft().trim().is_empty() || !shots[1].is_empty()));
            g.set_time_mode(time_mode);
            g.set_beats(beats);
            g.set_summary(s(&summary));
            g.set_panel(match p.panel.get() { Some("board") => 1, Some("tv") => 2, Some("history") => 3, _ => 0 });
            g.set_panel_title(s(&title));
            g.set_panel_sub(s(&sub));
            if let Some(m) = crate::view::sync(g.get_rows(), &rows) { g.set_rows(m); }
            g.set_drawer(open.is_some());
            g.set_d_note(s(&cnote));
            if let Some(m) = crate::view::sync(g.get_d_note_btns(), &note_labels) { g.set_d_note_btns(m); }
            if let Some(m) = crate::view::sync(g.get_d_more_items(), &more_items) { g.set_d_more_items(m); }
            g.set_d_status(s(status));
            g.set_d_stage(stage);
            g.set_d_branch(s(&branch));
            if let Some(m) = crate::view::sync(g.get_list(), &list_rows) { g.set_list(m); }
            if let Some(o) = &open {
                let (name, c) = hover_office::bot::BOTS[o.bot % 6];
                let _ = name;
                g.set_d_name(s(hover_office::bot::BOTS[o.bot % 6].0));
                g.set_d_color(Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8));
                g.set_d_tool(s(o.tool.name()));
                g.set_d_tool_id(s(o.tool.id()));
                // The session's own tool access, or the tool's setting.
                let access = o.access.clone().unwrap_or_else(|| hover_agents::state::tool_access(&self.hover.settings, o.tool).to_owned());
                g.set_d_access(s(ACCESS.iter().find(|a| a.0 == access).map_or("", |a| a.1)));
                g.set_d_access_note(s(if ACCESS.iter().any(|a| a.0 == access) { access_note(&access, o.tool) } else { "" }));
                g.set_d_access_id(s(&access));
                g.set_d_ctx(o.context.map_or(-1.0, |c| c.round_ties_even() as f32));
                g.set_d_cloud(o.cloud.is_some() && o.kiro_id.is_some());
                g.set_d_asking(o.waiting());
                g.set_d_ask(o.asking().map(|a| self.ask_data(a, o.asks.len())).unwrap_or_default());
                g.set_d_tool_color(tool_color(o.tool.id()));
                g.set_d_title(s(o.title()));
                g.set_d_folder(s(hover_office::office::short(&o.folder)));
                g.set_d_busy(o.busy());
                g.set_d_stopping(o.stopping);
                let bot = hover_office::bot::BOTS[o.bot % 6].0;
                // A reply never answers what the agent asked: while it waits, it queues too.
                g.set_d_placeholder(s(if o.busy() { "Queue a reply".to_owned() } else { format!("Reply to {bot}") }));
                g.set_d_reply_label(s(format!("Reply to {bot}")));
            }
        });
        self.notch.global::<crate::ui::Office>().set_d_wide(false);
        if let Some(d) = &*self.dash.borrow() { d.global::<crate::ui::Office>().set_d_wide(wide); }
        if open.is_some() { self.paint_thread(); }
        self.desk_sync();
    }

    fn panel_rows(&self, sessions: &[KiroSession]) -> Panel {
        let mut rows = vec![];
        let mut opens = vec![];
        let stage_of = |s: &KiroSession| Stage::parse(hover_agents::state::stage(s.state, s.phase));
        let bot_color = |b: usize| { let c = hover_office::bot::BOTS[b % 6].1; Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, c as u8) };
        let row = |kind: i32| PanelRow { kind, pct: 0.0, ..Default::default() };
        let now = hover_core::time::Stamp::now().unix_ms() as f64;
        match self.page.panel.get() {
            Some("board") => {
                for (h, c, st) in [("Waking", 0xf5b83du32, &[Stage::Waking][..]), ("Doing", 0x9b6bff, &[Stage::Working, Stage::Waiting]), ("Finished", 0x2fae66, &[Stage::Done, Stage::Failed, Stage::Stopped])] {
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
                rows.push(PanelRow { s1: s(n(&[Stage::Waking, Stage::Working, Stage::Waiting])), s2: s(n(&[Stage::Done])), s3: s(n(&[Stage::Failed])), s4: s(n(&[Stage::Stopped])), ..row(2) });
                opens.push((None, None));
                rows.push(PanelRow { text: s("CONTEXT USED"), color: Color::from_argb_u8(0, 0, 0, 0), ..row(0) });
                opens.push((None, None));
                for x in sessions {
                    rows.push(PanelRow { sub: s(hover_office::bot::BOTS[x.bot % 6].0), text: s(x.title()), color: bot_color(x.bot), pct: x.context.unwrap_or(0.0) as f32,
                        count: s(x.context.map_or("—".into(), |c| format!("{}%", c.round()))), stage: -1, ..row(3) });
                    opens.push((Some(x.id as i64), None));
                }
                if sessions.is_empty() { rows.push(PanelRow { text: s("No sessions yet. Press + to give an agent a task."), ..row(4) }); opens.push((None, None)); }
                ("Office overview".into(), "Up to 3 tasks run at once, across Kiro, Codex, Cursor, OpenCode and Claude Code".into(), rows, opens)
            }
            Some(_) => {
                let find = self.notch.global::<crate::ui::Office>().get_find().to_string().to_lowercase();
                let all = self.hover.history.as_ref().map(|h| h.entries()).unwrap_or_default();
                let list: Vec<_> = all.iter().filter(|h| find.is_empty() || format!("{} {} {}", h.title, h.folder, h.tool.id()).to_lowercase().contains(&find)).collect();
                // The user's Kiro Web sessions Hover doesn't have, in the same list by date.
                let web = self.page.web.borrow();
                let clouds: Vec<_> = web.list.iter().filter(|c| !sessions.iter().any(|x| x.kiro_id.as_deref() == Some(c.id.as_str())))
                    .filter(|c| find.is_empty() || format!("{} kiro web", c.title).to_lowercase().contains(&find)).collect();
                enum Item<'a> { Here(&'a hover_core::history::HistoryEntry), Web(&'a hover_agents::acp::CloudSession) }
                let mut items: Vec<(f64, Item)> = list.iter().map(|h| (h.updated.unix_ms() as f64, Item::Here(h)))
                    .chain(clouds.iter().map(|c| (c.updated.map_or(0.0, |u| u.unix_ms() as f64), Item::Web(c)))).collect();
                items.sort_by(|a, b| b.0.total_cmp(&a.0));
                // Why there are no Kiro Web sessions, in words at the top, not in the small print.
                if let Some(e) = &web.error { rows.push(PanelRow { text: s(format!("Kiro Web sessions couldn’t be listed. {e}")), ..row(4) }); opens.push((None, None)); }
                else if !web.listing && web.list.is_empty() && !web.note.is_empty() { rows.push(PanelRow { text: s(format!("No Kiro Web sessions to add. {}", web.note)), ..row(4) }); opens.push((None, None)); }
                let mut at = String::new();
                for (ms, item) in &items {
                    let ms = *ms;
                    let d = if ms > 0.0 { day(now, ms) } else { "Earlier".to_owned() };
                    if d != at { at = d.clone(); rows.push(PanelRow { text: s(d.to_uppercase()), color: Color::from_argb_u8(0, 0, 0, 0), ..row(0) }); opens.push((None, None)); }
                    let h = match item {
                        Item::Here(h) => *h,
                        Item::Web(c) => {
                            // Kiro's logo, the title, and that it is in Kiro Web; open 1 marks it (no bin: Hover doesn't keep it yet).
                            let opening = web.opening.as_deref() == Some(c.id.as_str());
                            let title = if c.title.is_empty() { "Kiro Web session" } else { c.title.as_str() };
                            rows.push(PanelRow { sub: s("kiro"), text: s(title), meta: s(if opening { "Opening…" } else { "Not opened yet" }), s1: s(if ms > 0.0 { stamp(now, ms) } else { String::new() }),
                                count: s("click to read it"), stage: Stage::Stopped as i32, open: 1, ..row(6) });
                            opens.push((None, Some(format!("{WEB_ROW}{}", c.id))));
                            continue;
                        }
                    };
                    let live = sessions.iter().find(|s| s.key == h.key);
                    let desk = live.is_some();
                    // The saved entry holds the last finished turn's state, so a reply running now
                    // would read "Done". A session at a desk that is running says what it is doing.
                    let stage = match live.filter(|l| l.busy()) {
                        Some(l) => stage_of(l),
                        None => Stage::parse(hover_agents::state::stage(h.state, hover_agents::stream::KiroPhase::Working)),
                    };
                    // .hr: the tool's logo, the task and its date, then how it went · turns · where.
                    rows.push(PanelRow { sub: s(h.tool.id()), text: s(&h.title), meta: s(stage.word()), s1: s(stamp(now, ms)),
                        // What the session cost in all, where the tool says (Kiro); else only where.
                        count: s(match h.credits { Some(c) => format!("{} · {}", hover_chat::state::credits(c), hover_office::office::short(&h.folder)), None => hover_office::office::short(&h.folder) }),
                        stage: stage as i32, key: s(&h.key), desk, ..row(6) });
                    opens.push((None, Some(h.key.clone())));
                }
                if items.is_empty() { rows.push(PanelRow { text: s(if all.is_empty() && web.list.is_empty() { "Sessions you start are kept here. Open one to read it, reply to carry on." } else { "Nothing matches." }), ..row(4) }); opens.push((None, None)); }
                let mut sub = format!("{} session{}, kept until you delete them", all.len(), if all.len() == 1 { "" } else { "s" });
                if web.listing { sub.push_str(" · looking up Kiro Web…"); }
                ("Session history".into(), sub, rows, opens)
            }
            None => (String::new(), String::new(), rows, opens),
        }
    }

    /// An image the open chat asked for is in (or failed): the sections showing it in an
    /// answer are laid out again, now that its size is known; a prompt's thumbnail (a fixed
    /// square) is only painted again.
    pub fn image_arrived(self: &Rc<Self>, url: &str) {
        self.desk_image_arrived(url);
        let open = self.page.thread.borrow_mut().as_mut().is_some_and(|c| {
            if c.thread.image_changed(url) { c.laid = None; }
            true
        });
        if open { self.paint_thread(); }
    }

    /// The open session's thread, as the drawer shows it (renderDrawer, through hover-chat).
    fn paint_thread(self: &Rc<Self>) {
        let Some(id) = self.page.open.get() else { return };
        let Some((rev, _busy)) = self.hover.sessions.rev(id) else { return };
        let now = hover_core::time::Stamp::now();
        // Nothing in the thread moves with the clock: it is laid out again only when the
        // session changed.
        let second = 0;
        let which = self.page.target.get();
        let dash = self.dash.borrow();
        let g = if which == 1 { dash.as_ref().map(|d| d.global::<crate::ui::Office>()) } else { Some(self.notch.global::<crate::ui::Office>()) };
        let Some(g) = g else { return };
        // #drawer: min(400, W − 24) wide, the thread between its header (≈ 76) and the
        // composer (≈ 62).
        let (ow, oh) = self.page.size.get();
        // The thread's own box once Slint has laid it out (the question card over the
        // composer takes some of it), else the estimate.
        let (tw, th) = (g.get_d_thread_w(), g.get_d_thread_h());
        let w = if tw > 0.0 { tw } else { (400.0f32).min(ow as f32 - 24.0) };
        let h = if th > 0.0 { th } else { (oh as f32 - 24.0 - 76.0 - 62.0).max(40.0) };
        let mut chat = self.page.thread.borrow_mut();
        let same = chat.as_ref().is_some_and(|c| c.id == id && c.laid == Some((rev, second, w, h)));
        if !same {
            // Only now is the whole session copied and read: a scroll, a click or a redraw
            // of an unchanged chat paints what is laid out already.
            let Some(sess) = self.hover.sessions.get(id) else { return };
            let files = |s: &KiroSession| crate::net::files_host(&s.key, &s.folder);
            let v = to_serde(&hover_agents::state::state(&sess, &files));
            let off = hover_core::time::local_offset_min(now.ticks);
            let mut turns = hover_chat::state::turns_at(&v, now.unix_ms() as f64, &|ms| hover_chat::state::hm(ms, off));
            // Checkpoints offer themselves where one was kept and nothing runs (the agent could be writing).
            let quiet = !sess.busy() && !sess.turns.iter().any(|t| t.queued);
            let n = sess.turns.len();
            for (i, (t, k)) in turns.iter_mut().zip(&sess.turns).enumerate() {
                t.again = quiet && k.before.is_some();
                t.restore = quiet && i + 1 < n && k.after.is_some();
            }
            if chat.as_ref().is_none_or(|c| c.id != id) {
                let (name, c) = hover_office::bot::BOTS[sess.bot % 6];
                let f = fonts();
                let mut thread = hover_chat::Thread::new(hover_chat::Shaper::new(&f), name, [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255]);
                // Prompts' pictures, and an answer's from the web or the session's folder,
                // loaded off the UI thread into a cache the layout and the painter share.
                let images = crate::net::images();
                thread.use_images(images.clone());
                let (host, folder) = (files(&sess), sess.folder.clone());
                thread.image_rule = Box::new(move |src| hover_md::image::image_for(&hover_md::image::Session { files: host.as_deref(), folder: &folder }, src));
                *chat = Some(Chat { id, thread, painter: hover_chat::Painter::new(&f, images), scroll: f32::MAX, width: 0.0, key: sess.key.clone(), laid: None, sel: Sel::default() });
            }
            let c = chat.as_mut().unwrap();
            // Follows the bottom only when it was there (within 40 px): reading older
            // turns never jumps, a new answer or not.
            let was_near = c.thread.height - c.scroll - c.thread.view_h < 40.0;
            c.thread.tool = sess.tool.id().into();
            c.thread.view_h = h;
            // Room under the last turn for the reply circle over the thread's corner.
            c.thread.extra_bottom = 40.0;
            // A new width (the chat expanded, or came back) keeps the reader at the same place:
            // the turn at the top of the view, and how far down it (as a share of the turn).
            let anchor = (c.width > 0.0 && c.width != w && !was_near).then(|| c.thread.sections.iter().position(|x| x.y + x.h > c.scroll).map(|i| (i, (c.scroll - c.thread.sections[i].y) / c.thread.sections[i].h.max(1.0)))).flatten();
            c.thread.set(&turns, w);
            if let Some((i, frac)) = anchor { if let Some(x) = c.thread.sections.get(i) { c.scroll = x.y + frac * x.h; } }
            if was_near { c.scroll = f32::MAX; }
            *self.page.turns.borrow_mut() = turns;
            c.width = w;
            c.laid = Some((rev, second, w, h));
        }
        let c = chat.as_mut().unwrap();
        let max = (c.thread.height - h).max(0.0);
        c.scroll = c.scroll.clamp(0.0, max);
        // .jump: "Latest" once the reader is well above the end.
        g.set_d_jump(max - c.scroll > 160.0);
        let k = if which == 1 { dash.as_ref().map_or(1.0, |d| d.window().scale_factor()) } else { self.notch.window().scale_factor() };
        let px = c.painter.paint(&c.thread, c.scroll, (w * k).round() as u32, (h * k).round() as u32, k, [0, 0, 0, 0]);
        let img = Image::from_rgba8_premultiplied(SharedPixelBuffer::clone_from_slice(px.data(), px.width(), px.height()));
        g.set_d_thread(img);
        let _ = &c.key;
    }

    pub fn wire_office(self: &Rc<Self>, g: crate::ui::Office) {
        self.wire_office_more(&g);
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
        g.on_fab_main(move || { let f = a.page.fab.get(); a.desk_leave(); a.close_drawer(); a.page.panel.set(None); a.send(In::Panel(None)); a.page.fab.set(if f == 1 { 0 } else { 1 }); a.office_widgets(); });
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
        g.on_new_draft_edited(move || a.office_widgets());
        let a = self.clone();
        g.on_new_go_clicked(move || {
            let text = each_draft(&a).trim().to_owned();
            let images = a.page.attached.borrow()[1].clone();
            if text.is_empty() && images.is_empty() { return; }
            let tool = AgentTool::ALL[a.page.new_tool.get()];
            // Enter starts it too, past the Start button's own gate: what keeps the button
            // off is said here instead of nothing happening.
            if let Some(r) = hover_agents::agents::known(tool).filter(|r| !r.ok()) { a.toast(&r.hint); return; }
            let cloud_on = tool == AgentTool::Kiro && a.page.cloud.borrow().on;
            let folder = a.page.new_folder.borrow().clone().filter(|f| hover_agents::usable_folder(Some(f)));
            // Kiro Web works in its own sandbox; the session still has a folder here, the
            // default workspace when none was picked.
            let folder = if cloud_on && folder.is_none() {
                match a.hover.settings.default_workspace().path().map(|p| hover_core::projects::ensure_folder(&p)) {
                    Some(Ok(p)) => Some(p.to_string_lossy().into_owned()),
                    Some(Err(e)) => { a.toast(&e); return; }
                    None => None,
                }
            } else { folder };
            let Some(folder) = folder else { a.toast(&format!("Choose a folder for {} to work in first.", tool.name())); return };
            if !a.hover.sessions.can_start() { a.toast("3 tasks are running. Start another when one is done."); return; }
            if !a.hover.settings.kiro_notice_seen() { a.hover.settings.set_kiro_notice_seen(true); }
            // The access picked in the new-task box, for this session only.
            let access = a.new_access(a.page.new_tool.get());
            let picked = a.page.new_folder.borrow().clone();
            let cloud = cloud_on.then(|| a.cloud_repo(picked.as_deref(), true).into_iter().collect::<Vec<_>>());
            // Where it works (workspace.rs): a worktree of its own by default. Git makes it off the UI thread.
            if a.page.starting.replace(true) { return; }
            use hover_agents::workspace::Choice;
            let choice = if a.hover.settings.automation().use_folder { Choice::Folder } else { Choice::Own { base: None } };
            let read_only = access == "read";
            if matches!(choice, Choice::Own { .. }) && !cloud_on && !read_only { a.toast("Setting up the task’s workspace…"); }
            let title: String = text.lines().next().unwrap_or("task").chars().take(60).collect();
            let root = hover_core::paths::support().join("worktrees");
            std::thread::Builder::new().name("workspace".into()).spawn(move || {
                let made = hover_agents::workspace::prepare(&folder, &choice, &title, &root, read_only, cloud_on, &hover_agents::cancel::Cancel::new());
                crate::ui_do(move |a| {
                    a.page.starting.set(false);
                    let p = match made { Ok(p) => p, Err(e) => { a.toast(&e); return; } };
                    // What was decided, in words, unless it is what the user chose or a Kiro Web task's own place.
                    if let (Choice::Own { .. }, Some(n)) = (&choice, &p.note) { if !cloud_on { a.toast(n); } }
                    let ext = hover_core::ext::SessionExt { workspace: p.binding.clone(), ..Default::default() };
                    match a.hover.sessions.start_bound(tool, &p.folder, &text, images, Some(access), cloud, ext) {
                        Some(_) => { each!(a, |g| g.set_new_draft(s(""))); a.page.attached.borrow_mut()[1].clear(); a.page.fab.set(0); a.page.cloud.borrow_mut().menu = false; }
                        None => {
                            // No desk: the worktree just made would stay behind, empty.
                            if let Some(b) = p.binding.filter(|b| b.is_worktree()) { let _ = hover_agents::workspace::remove(&b, &p.folder, false, false, hover_agents::workspace::RemoveOpts { delete_branch: true, ..Default::default() }); }
                            a.toast("All six desks are busy. Stop or remove a session first.");
                        }
                    }
                    a.office_changed();
                    a.office_widgets();
                });
            }).ok();
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
                Some((None, Some(key))) if key.starts_with(WEB_ROW) => a.open_web(&key[WEB_ROW.len()..]),
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
            if let Some((_, Some(key))) = r.filter(|r| !r.1.as_deref().is_some_and(|k| k.starts_with(WEB_ROW))) {
                let title = a.hover.history.as_ref().and_then(|h| h.entries().into_iter().find(|e| e.key == key)).map(|e| e.title).unwrap_or_default();
                a.ask_delete(None, Some(key), &title, false);
            }
        });
        let a = self.clone();
        g.on_find_edited(move |t| { each!(a, |g| g.set_find(t.clone())); a.office_widgets(); });
        let a = self.clone();
        g.on_confirm_no(move || { *a.page.confirm_key.borrow_mut() = None; *a.page.confirm_rewind.borrow_mut() = None; each!(a, |g| g.set_confirm(false)); });
        let a = self.clone();
        g.on_confirm_yes(move || {
            let k = a.page.confirm_key.borrow_mut().take();
            let rewind = a.page.confirm_rewind.borrow_mut().take();
            each!(a, |g| g.set_confirm(false));
            if let Some((id, to)) = rewind { a.rewind(id, to); return; }
            if let Some((id, key)) = k {
                let key = key.or_else(|| id.and_then(|i| a.hover.sessions.get(i)).map(|s| s.key.clone()));
                if let Some(key) = key { a.hover.sessions.delete(&key); hover_agents::desk::forget_apps(&key); if let Some(h) = &a.hover.history { h.delete(&key); } }
                if id.is_some() && id == a.page.open.get() { a.close_drawer(); }
                a.office_changed();
                a.office_widgets();
            }
        });
        let a = self.clone();
        g.on_toggle_menu(move || { a.page.menu.set(!a.page.menu.get()); a.office_widgets(); });
        let a = self.clone();
        g.on_open_access(move || {
            if AgentTool::ALL[a.page.new_tool.get()] == AgentTool::Kiro && a.page.cloud.borrow().on { a.toast("Kiro Web runs every task with full access."); return; }
            a.page.access_menu.set(!a.page.access_menu.get());
            a.office_widgets();
        });
        let a = self.clone();
        g.on_toggle_cloud(move || {
            { let mut c = a.page.cloud.borrow_mut(); c.on = !c.on; c.menu = false; }
            a.page.access_menu.set(false);
            a.office_widgets();
        });
        let a = self.clone();
        g.on_open_repos(move || {
            let open = {
                let mut c = a.page.cloud.borrow_mut();
                c.menu = !c.menu;
                c.query.clear();
                c.menu
            };
            a.page.access_menu.set(false);
            // Asked of Kiro each time it opens (the saved list shows meanwhile).
            if open { a.load_repos(|a| a.office_widgets()); }
            a.office_widgets();
        });
        let a = self.clone();
        g.on_pick_repo(move |id| {
            { let mut c = a.page.cloud.borrow_mut(); c.pick = Some(Some(id.to_string()).filter(|r| !r.is_empty())); c.menu = false; c.query.clear(); }
            a.office_widgets();
        });
        let a = self.clone();
        g.on_repo_search(move |q| { a.page.cloud.borrow_mut().query = q.to_string(); a.repos_missed(&q, |a| a.office_widgets()); a.office_widgets(); });
        let a = self.clone();
        // Enter in the search box picks the first repository that matches.
        g.on_repo_search_enter(move || {
            let folder = a.page.new_folder.borrow().clone();
            let first = {
                let c = a.page.cloud.borrow();
                let q = c.query.trim();
                let mut names: Vec<String> = c.folder_repo.as_ref().filter(|(k, _)| Some(k) == folder.as_ref()).and_then(|(_, r)| r.clone()).into_iter().collect();
                if let Some(Ok(l)) = &c.repos { names.extend(l.iter().cloned()); }
                if q.is_empty() { None } else { names.into_iter().find(|r| repo_matches(r, q)) }
            };
            if let Some(r) = first {
                { let mut c = a.page.cloud.borrow_mut(); c.pick = Some(Some(r)); c.menu = false; c.query.clear(); }
                a.office_widgets();
            }
        });
        let a = self.clone();
        g.on_d_open_cloud(move || {
            let id = a.page.open.get().and_then(|i| a.hover.sessions.get(i)).filter(|s| s.cloud.is_some()).and_then(|s| s.kiro_id);
            if let Some(id) = id { crate::open_url(&format!("{}{id}", hover_agents::acp::KIRO_WEB_SESSION)); }
        });
        let a = self.clone();
        g.on_pick_access(move |id| {
            let id = ACCESS.iter().map(|a| a.0).find(|x| *x == id.as_str());
            a.page.new_access.borrow_mut()[a.page.new_tool.get()] = id;
            a.page.access_menu.set(false);
            a.office_widgets();
        });
        let a = self.clone();
        g.on_answer(move |id, ask, how| {
            // The office answered what the agent asked: over its head, or in its chat (-1).
            let Some(id) = (if id < 0 { a.page.open.get() } else { Some(id) }) else { return };
            let answer = match how.as_str() { "allow" => AskAnswer::Allow, "trust" => AskAnswer::Trust, "trustAll" => AskAnswer::TrustAll, _ => AskAnswer::Deny };
            if let Some(s) = a.hover.sessions.get(id) { hover_core::log::line(&format!("{} run {}: {:?} from the office", s.tool.id(), s.id, answer).to_lowercase()); }
            a.hover.sessions.answer(id, &ask, answer);
            a.office_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_d_close(move || a.close_drawer());
        let a = self.clone();
        g.on_d_note_act(move |i| a.note_act(i as usize));
        let a = self.clone();
        g.on_d_more_act(move |i| a.more_act(i as usize));
        let a = self.clone();
        g.on_d_expand(move || if let Some(id) = a.page.open.get() { a.expand_chat(id); });
        let a = self.clone();
        g.on_d_collapse(move || a.collapse_chat());
        let a = self.clone();
        g.on_d_stop(move || if let Some(id) = a.page.open.get() { a.hover.sessions.stop(id); a.office_changed(); a.office_widgets(); });
        let a = self.clone();
        g.on_d_editor(move || if let Some(id) = a.page.open.get() { a.editor_for(id); });
        let a = self.clone();
        g.on_d_details(move || if let Some(id) = a.page.open.get() { a.desk_details(id); });
        let a = self.clone();
        g.on_list_clicked(move |i| {
            let r = a.page.rows_list.borrow().get(i as usize).cloned();
            match r {
                Some((Some(id), _)) => a.open_session(id),
                Some((None, Some(key))) => {
                    if let Some(s) = a.hover.sessions.all().into_iter().find(|s| s.key == key) { a.open_session(s.id); }
                    else if let Some(s) = a.hover.sessions.wake(&key) { a.office_changed(); a.open_session(s.id); }
                }
                _ => {}
            }
        });
        let a = self.clone();
        g.on_d_delete(move || { if let Some(s) = a.page.open.get().and_then(|id| a.hover.sessions.get(id)) { a.ask_delete(Some(s.id), None, &s.title(), s.busy()); } });
        let a = self.clone();
        g.on_d_send(move || {
            let Some(id) = a.page.open.get() else { return };
            let text = each_reply(&a).trim().to_owned();
            let images = a.page.attached.borrow()[0].clone();
            if text.is_empty() && images.is_empty() {
                // Pause, not Stop: the tool cancels the turn, the conversation stays, and
                // the next queued reply goes once it says the turn has ended.
                if a.hover.sessions.get(id).is_some_and(|s| s.busy() && !s.stopping) { a.hover.sessions.pause(id); a.office_widgets(); }
                return;
            }
            // A reply while a question waits is its answer, in the user's own words, where
            // the question takes one. A permission it asked is left for its own buttons:
            // the reply is queued behind it and never says yes or no to it.
            if let Some(q) = a.hover.sessions.get(id).and_then(|s| s.asking().cloned()) {
                if let Some(qs) = q.questions.as_ref().filter(|q| !q.is_empty()) {
                    if qs.len() == 1 && qs[0].custom && images.is_empty() {
                        a.page.picks.borrow_mut().insert(q.id.clone(), Picks { sel: vec![vec![]], text: vec![text.clone()] });
                        if a.send_answers(id, &q) { each!(a, |g| { g.set_d_draft(s("")); g.set_d_compose(false); }); }
                        return;
                    }
                    a.toast("Answer the question above first, or skip it.");
                    return;
                }
            }
            if !a.hover.sessions.reply(id, &text, images) { a.toast("3 tasks are running. Reply when one is done."); return; }
            a.page.attached.borrow_mut()[0].clear();
            a.page.drafts.borrow_mut().remove(&id);
            // Sending closes the box; the thread shows the reply at its end.
            each!(a, |g| { g.set_d_draft(s("")); g.set_d_compose(false); });
            if let Some(c) = &mut *a.page.thread.borrow_mut() { c.scroll = f32::MAX; }
            a.office_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_d_latest(move || { if let Some(c) = &mut *a.page.thread.borrow_mut() { c.scroll = f32::MAX; } a.paint_thread(); });
        let a = self.clone();
        g.on_d_click(move |x, y| a.thread_click(x, y));
        let a = self.clone();
        g.on_d_pointer(move |kind, x, y, shift| a.thread_pointer(kind, x, y, shift));
        let a = self.clone();
        g.on_d_copy(move || {
            // Ctrl+C with text selected in the thread: that text, as the page copied it.
            let text = a.page.thread.borrow().as_ref().map(|c| c.thread.selected_text()).unwrap_or_default();
            if text.is_empty() { return false; }
            if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) { hover_core::log::line(&format!("clipboard: {e}")); }
            true
        });
        let a = self.clone();
        g.on_d_resized(move || { let a = a.clone(); slint::Timer::single_shot(Duration::ZERO, move || a.paint_thread()); });
        let a = self.clone();
        g.on_d_wheel(move |dy| { if let Some(c) = &mut *a.page.thread.borrow_mut() { c.scroll = (c.scroll - dy).max(0.0); } a.paint_thread(); });
        let a = self.clone();
        g.on_d_type(move |t| {
            // One printable character (not a control, nor one of Slint's keys, which are
            // private-use characters): the reply box opens with it written.
            let mut cs = t.chars();
            let (Some(c), None) = (cs.next(), cs.next()) else { return false };
            if c.is_control() || ('\u{e000}'..='\u{f8ff}').contains(&c) || a.page.open.get().is_none() { return false; }
            each!(a, |g| { let d = g.get_d_draft(); g.set_d_draft(s(format!("{d}{c}"))); g.set_d_compose(true); });
            true
        });
    }

    fn wire_office_more(self: &Rc<Self>, g: &crate::ui::Office) {
        let a = self.clone();
        g.on_q_pick(move |id, ask, qi, label| {
            let Some(id) = (if id < 0 { a.page.open.get() } else { Some(id) }) else { return };
            let Some(q) = a.hover.sessions.get(id).and_then(|s| s.asking().cloned()).filter(|q| q.id == ask.as_str()) else { return };
            let Some(qs) = q.questions.as_ref() else { return };
            let (qi, label) = (qi as usize, label.to_string());
            let Some(one) = qs.get(qi) else { return };
            let mut picks = a.page.picks.borrow_mut();
            let p = picks.entry(q.id.clone()).or_insert_with(|| Picks { sel: vec![vec![]; qs.len()], text: vec![String::new(); qs.len()] });
            let sel = &mut p.sel[qi];
            *sel = if sel.contains(&label) { sel.iter().filter(|x| **x != label).cloned().collect() } else if one.multiple { let mut v = sel.clone(); v.push(label); v } else { vec![label] };
            drop(picks);
            a.office_widgets();
        });
        let a = self.clone();
        g.on_q_text(move |id, ask, qi, text| {
            let Some(id) = (if id < 0 { a.page.open.get() } else { Some(id) }) else { return };
            let Some(q) = a.hover.sessions.get(id).and_then(|s| s.asking().cloned()).filter(|q| q.id == ask.as_str()) else { return };
            let n = q.questions.as_ref().map_or(0, Vec::len);
            let mut picks = a.page.picks.borrow_mut();
            let p = picks.entry(q.id.clone()).or_insert_with(|| Picks { sel: vec![vec![]; n], text: vec![String::new(); n] });
            if let Some(t) = p.text.get_mut(qi as usize) { *t = text.to_string(); }
        });
        let a = self.clone();
        g.on_q_send(move |id, ask| {
            let Some(id) = (if id < 0 { a.page.open.get() } else { Some(id) }) else { return };
            if let Some(q) = a.hover.sessions.get(id).and_then(|s| s.asking().cloned()).filter(|q| q.id == ask.as_str()) { a.send_answers(id, &q); }
        });
        let a = self.clone();
        g.on_q_open(move |id| a.open_session(id));
        let a = self.clone();
        g.on_open_model(move |which, x, y| {
            a.page.model_menu.set(which);
            if which != 0 { a.page.access_menu.set(false); each!(a, |g| { g.set_model_x(x); g.set_model_y(y); }); }
            a.office_widgets();
        });
        let a = self.clone();
        g.on_pick_model(move |id| {
            // The pick is the tool's default from then on, as in Settings, from its next turn.
            if let Some(t) = a.menu_tool() {
                let o = a.hover.settings.agent_options(t);
                // Default is no model: an empty id would be sent as one (and OpenCode would
                // look for a model called "").
                a.hover.settings.set_agent_options(t, hover_core::model::AgentOptions { model: (!id.is_empty()).then(|| id.to_string()), ..o });
            }
            a.page.model_menu.set(0);
            a.office_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_pick_effort(move |e| {
            if let Some(t) = a.menu_tool() {
                let o = a.hover.settings.agent_options(t);
                a.hover.settings.set_agent_options(t, hover_core::model::AgentOptions { effort: Some(e.to_string()), ..o });
            }
            a.office_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_notice_ok(move || {
            a.hover.settings.set_kiro_notice_seen(true);
            // The other view (notch or app window) may be showing the note too.
            a.hover.sessions.raise_changed();
            a.office_widgets();
        });
        let a = self.clone();
        g.on_attach(move |which| {
            let k = if which == 1 { 0 } else { 1 };
            if a.page.attached.borrow()[k].len() >= hover_core::images::MAX_IMAGES { a.toast("Four images at most."); return; }
            let Some(file) = crate::pick_image() else { return };
            match attach_file(&file) {
                Ok(saved) => { a.page.attached.borrow_mut()[k].push(saved); }
                Err(why) => a.toast(&why),
            }
            a.office_widgets();
        });
        let a = self.clone();
        g.on_paste_image(move |which| {
            // The page's paste handler: a picture on the clipboard is attached; anything
            // else is left to the box, which pastes the text.
            let Ok(img) = arboard::Clipboard::new().and_then(|mut c| c.get_image()) else { return false };
            let k = if which == 1 { 0 } else { 1 };
            if a.page.attached.borrow()[k].len() >= hover_core::images::MAX_IMAGES { a.toast("Four images at most."); return true; }
            match attach_pixels(img.width as u32, img.height as u32, img.bytes.into_owned()) {
                Ok(saved) => a.page.attached.borrow_mut()[k].push(saved),
                Err(why) => a.toast(&why),
            }
            a.office_widgets();
            true
        });
        let a = self.clone();
        g.on_unattach(move |which, i| {
            let k = if which == 1 { 0 } else { 1 };
            let mut l = a.page.attached.borrow_mut();
            if (i as usize) < l[k].len() { l[k].remove(i as usize); }
            drop(l);
            a.office_widgets();
        });
    }

    /// The tool whose model menu is open.
    fn menu_tool(&self) -> Option<AgentTool> {
        match self.page.model_menu.get() {
            1 => self.page.open.get().and_then(|id| self.hover.sessions.get(id)).map(|s| s.tool),
            2 => Some(AgentTool::ALL[self.page.new_tool.get()]),
            _ => None,
        }
    }

    /// sendAnswers: each question's picks and typed words; every one needs one.
    fn send_answers(self: &Rc<Self>, id: i32, q: &hover_agents::ask::AgentAsk) -> bool {
        let n = q.questions.as_ref().map_or(0, Vec::len);
        let p = self.page.picks.borrow().get(&q.id).cloned().unwrap_or(Picks { sel: vec![vec![]; n], text: vec![String::new(); n] });
        let answers: Vec<Vec<String>> = (0..n).map(|i| {
            let mut v = p.sel.get(i).cloned().unwrap_or_default();
            if let Some(t) = p.text.get(i).map(|t| t.trim()).filter(|t| !t.is_empty()) { v.push(t.to_owned()); }
            v
        }).collect();
        if answers.iter().any(Vec::is_empty) { self.toast(if n > 1 { "Answer each question first." } else { "Pick an answer first." }); return false; }
        if let Some(s) = self.hover.sessions.get(id) { hover_core::log::line(&format!("{} run {}: answered a question from the office", s.tool.id(), s.id)); }
        if !self.hover.sessions.answer_question(id, &q.id, answers) { self.toast("Pick an answer first."); return false; }
        self.page.picks.borrow_mut().remove(&q.id);
        self.page.qmodels.borrow_mut().remove(&q.id);
        self.office_changed();
        self.office_widgets();
        true
    }

    /// renderPill: the model's name, its effort when the model (or tool) takes that one,
    /// and whether the tool offers models at all.
    pub(crate) fn pill(&self, t: AgentTool) -> (String, String, bool) {
        let st = &self.hover.settings;
        let models = hover_agents::state::models_with_levels(st, t);
        let o = st.agent_options(t);
        let (tool_efforts, now) = hover_agents::state::efforts(st, t);
        let model = o.model.clone().or_else(|| models.first().map(|m| m.0.clone())).unwrap_or_default();
        let m = models.iter().find(|m| m.0 == model).or(models.first());
        let effort = o.effort.clone().or(now);
        let eff = effort.filter(|e| hover_agents::state::efforts_of(&models, &model, &tool_efforts).contains(e)).map(|e| effort_word(&e)).unwrap_or_default();
        (m.map_or("Default".into(), |m| short_model(&m.1)), eff, !models.is_empty())
    }

    /// openMenu's rows: the heading, the models, the effort's heading and choices, the note.
    pub(crate) fn model_rows(&self, t: AgentTool) -> (String, Vec<MOpt>, String, Vec<MOpt>, String) {
        let st = &self.hover.settings;
        let models = hover_agents::state::models_with_levels(st, t);
        let o = st.agent_options(t);
        let (tool_efforts, now) = hover_agents::state::efforts(st, t);
        let cur = o.model.clone().unwrap_or_else(|| models.first().map_or(String::new(), |m| m.0.clone()));
        let model = o.model.clone().or_else(|| models.first().map(|m| m.0.clone())).unwrap_or_default();
        let effort = o.effort.clone().or(now);
        let efforts = hover_agents::state::efforts_of(&models, &model, &tool_efforts);
        (format!("{} model", t.name()).to_uppercase(),
            models.iter().map(|m| MOpt { id: s(&m.0), label: s(short_model(&m.1)), on: m.0 == cur }).collect(),
            hover_agents::runtime::caps(t).effort_label.to_uppercase(),
            efforts.iter().map(|e| MOpt { id: s(e), label: s(effort_word(e)), on: effort.as_deref() == Some(e.as_str()) }).collect(),
            format!("Used by {} from its next turn.", t.name()))
    }

    /// Pictures from elsewhere (voice's screenshots while dictating) for the reply, as a paste
    /// adds them: up to the most a message takes.
    pub(crate) fn attach_reply(self: &Rc<Self>, files: Vec<String>) {
        if files.is_empty() { return; }
        let mut l = self.page.attached.borrow_mut();
        for f in files { if l[0].len() < hover_core::images::MAX_IMAGES { l[0].push(f); } }
        drop(l);
        self.office_widgets();
    }

    /// A picture's thumbnail, read once.
    pub(crate) fn thumb(&self, file: &str) -> Image {
        if let Some(i) = self.page.thumbs.borrow().get(file) { return i.clone(); }
        let img = image::open(file).ok().map(|i| i.thumbnail(104, 104).to_rgba8()).map(|i| {
            Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(i.as_raw(), i.width(), i.height()))
        }).unwrap_or_default();
        self.page.thumbs.borrow_mut().insert(file.to_owned(), img.clone());
        img
    }

    /// A question as its card shows it; a question's rows are kept for its id.
    fn ask_data(&self, a: &hover_agents::ask::AgentAsk, n: usize) -> AskData {
        let mut d = AskData { id: s(&a.id), title: s(hover_agents::words::ask_title(a)), command: s(a.command.as_deref().unwrap_or("")), path: s(a.path.as_deref().unwrap_or("")),
            preview: s(a.preview.as_deref().unwrap_or("")), reason: s(&a.reason), danger: a.danger, allow: s(hover_agents::words::ask_allow(a)), more: n as i32 - 1,
            question: a.is_question(), qs: ModelRc::default() };
        let Some(qs) = a.questions.as_ref().filter(|q| !q.is_empty()) else { return d };
        let picks = self.page.picks.borrow().get(&a.id).cloned().unwrap_or_default();
        let mut cache = self.page.qmodels.borrow_mut();
        if cache.len() > 20 { cache.clear(); }
        let (rows, opts) = cache.entry(a.id.clone()).or_insert_with(|| {
            let opts: Vec<Rc<VecModel<QOpt>>> = qs.iter().map(|q| Rc::new(VecModel::from(q.options.iter().map(|(l, dsc)| QOpt { label: s(l), description: s(dsc), on: false }).collect::<Vec<_>>()))).collect();
            let rows = Rc::new(VecModel::from(qs.iter().zip(&opts).enumerate().map(|(i, (q, o))| QData { header: s(q.header.to_uppercase()), question: s(&q.question), options: ModelRc::from(o.clone()),
                multiple: q.multiple, custom: q.custom, text: s(picks.text.get(i).map_or("", String::as_str)) }).collect::<Vec<_>>()));
            (rows, opts)
        });
        // The picks change in place: the same buttons, now pressed or not.
        for (i, (q, o)) in qs.iter().zip(opts.iter()).enumerate() {
            for (j, (l, _)) in q.options.iter().enumerate() {
                let on = picks.sel.get(i).is_some_and(|x| x.contains(l));
                if let Some(mut row) = o.row_data(j) { if row.on != on { row.on = on; o.set_row_data(j, row); } }
            }
        }
        d.qs = ModelRc::from(rows.clone());
        d
    }

    /// The open chat's thread and turns, for the screenshots.
    pub fn page_thread(&self) -> Option<std::cell::RefMut<'_, hover_chat::Thread>> {
        std::cell::RefMut::filter_map(self.page.thread.borrow_mut(), |c| c.as_mut().map(|c| &mut c.thread)).ok()
    }
    pub fn page_turns(&self) -> Vec<hover_chat::Turn> { self.page.turns.borrow().clone() }
    pub fn page_scroll(&self) -> f32 { self.page.thread.borrow().as_ref().map_or(0.0, |c| c.scroll) }

    /// The pointer in the open chat's thread (0 press, 1 move while pressed, 2 release):
    /// a press on text starts a selection (a double click the word and the spaces after
    /// it, as WebView2 on Windows; a third the paragraph), a drag grows it by that unit,
    /// Shift+press extends it; a press elsewhere clears it. Links, summaries and Copy are
    /// the click's (thread_click).
    fn thread_pointer(self: &Rc<Self>, kind: i32, x: f32, y: f32, shift: bool) {
        use hover_chat::{Hit, Pos, Unit};
        {
            let mut chat = self.page.thread.borrow_mut();
            let Some(c) = chat.as_mut() else { return };
            let yy = y + c.scroll;
            let n = if kind == 0 { c.sel.press(x, y) } else { 0 };
            let (th, s) = (&mut c.thread, &mut c.sel);
            match (kind, th.hit(x, yy)) {
                (0, Hit::Text(p)) if shift || n == 1 => {
                    if shift { if let Some(an) = s.anchor { th.select(an, p); } } else { s.anchor = Some(p); th.select(p, p); }
                    s.unit = Unit::Char;
                    s.dragging = true;
                }
                (0, Hit::Text(p)) => {
                    s.unit = if n == 2 { Unit::Word } else { Unit::Para };
                    let (a0, mut a1, tail) = th.unit_at(p, s.unit);
                    if n == 2 { a1 = th.trailing_space(a1); }
                    s.unit_anchor = (a0, a1, tail);
                    s.anchor = Some(a0);
                    th.select_units(s.unit_anchor, p, s.unit);
                    s.dragging = true;
                }
                (0, Hit::Link(_) | Hit::Toggle(_) | Hit::Act(..)) => return,
                (0, _) => { let p0 = Pos { section: 0, text: 0, byte: 0 }; th.select(p0, p0); s.anchor = None; }
                (1, Hit::Text(p)) if s.dragging && s.unit != Unit::Char => th.select_units(s.unit_anchor, p, s.unit),
                (1, Hit::Text(p)) if s.dragging => { if let Some(an) = s.anchor { th.select(an, p); } }
                (2, _) => { s.dragging = false; return; }
                _ => return,
            }
        }
        self.paint_thread();
    }

    /// A click in the open chat's thread: a summary line folds or opens its timeline, a
    /// step its change or output; Copy puts a code block on the clipboard; a link opens.
    fn thread_click(self: &Rc<Self>, x: f32, y: f32) {
        let hit = {
            let chat = self.page.thread.borrow();
            let Some(c) = chat.as_ref() else { return };
            c.thread.hit(x, y + c.scroll)
        };
        let turns = self.page.turns.borrow().clone();
        match hit {
            hover_chat::Hit::Toggle(i) => { if let Some(c) = &mut *self.page.thread.borrow_mut() { c.thread.toggle_steps(&turns, i); } }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::Step(j, now)) => { if let Some(c) = &mut *self.page.thread.borrow_mut() { c.thread.toggle_step(&turns, i, j, now); } }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::Flag(j, k)) => { if let Some(c) = &mut *self.page.thread.borrow_mut() { c.thread.toggle_flag(&turns, i, j, k); } }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::OpenDiff(j)) => {
                // The change opens in the timeline, its row near the top of the view.
                if let Some(c) = &mut *self.page.thread.borrow_mut() { if let Some(y) = c.thread.open_diff(&turns, i, j) { c.scroll = (y - 36.0).max(0.0); } }
            }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::Retry) => {
                // The newest turn's prompt again, as a reply; never while one is in flight.
                let Some(id) = self.page.open.get() else { return };
                let Some(sess) = self.hover.sessions.get(id).filter(|s| !s.busy() && !s.turns.iter().any(|t| t.queued)) else { return };
                let Some(t) = sess.turns.get(i).filter(|_| i + 1 == sess.turns.len()) else { return };
                if !self.hover.sessions.reply(id, &t.prompt, t.images.clone()) { self.toast("3 tasks are running. Retry when one is done."); return; }
                if let Some(c) = &mut *self.page.thread.borrow_mut() { c.scroll = f32::MAX; }
                self.office_changed();
                self.office_widgets();
                return;
            }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::Restore) => { self.ask_rewind(hover_agents::session::Rewind::After(i)); return; }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::TryAgain) => { self.ask_rewind(hover_agents::session::Rewind::Before(i)); return; }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::SendNow) => {
                let Some(id) = self.page.open.get() else { return };
                let Some(uid) = self.hover.sessions.get(id).and_then(|s| s.turns.get(i).map(|t| t.uid.clone())) else { return };
                match self.hover.sessions.send_now(id, &uid) {
                    Ok(hover_agents::session::SendNow::Steering) => self.toast("Stopping the run to send this now."),
                    Ok(_) => {}
                    Err(hover_agents::session::QueueError::Invalid(m)) => self.toast(&m),
                    Err(_) => self.toast("That message was already sent."),
                }
                self.office_changed();
                self.office_widgets();
                return;
            }
            // Edit: the message leaves the queue and its words go into the reply box; sending it queues it again. If it started meanwhile, nothing is lost: it is already sent.
            hover_chat::Hit::Act(i, hover_chat::doc::Act::Edit) => {
                let Some(id) = self.page.open.get() else { return };
                let Some(uid) = self.hover.sessions.get(id).and_then(|s| s.turns.get(i).map(|t| t.uid.clone())) else { return };
                match self.hover.sessions.remove_queued(id, &uid) {
                    Ok(m) => {
                        let keep = each_reply(self);
                        let text = if keep.trim().is_empty() { m.text } else { format!("{keep}\n{}", m.text) };
                        each!(self, |g| { g.set_d_draft(s(&text)); g.set_d_compose(true); });
                        if !m.images.is_empty() { self.attach_reply(m.images); }
                    }
                    Err(_) => self.toast("That message was already sent."),
                }
                self.office_changed();
                self.office_widgets();
                return;
            }
            hover_chat::Hit::Act(i, hover_chat::doc::Act::Cancel) => {
                let Some(id) = self.page.open.get() else { return };
                if self.hover.sessions.cancel_queued(id, i) { self.office_changed(); self.office_widgets(); }
                return;
            }
            hover_chat::Hit::Act(_, hover_chat::doc::Act::Copy(text)) => {
                if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) { hover_core::log::line(&format!("clipboard: {e}")); }
                if let Some(c) = &mut *self.page.thread.borrow_mut() { c.thread.set_copied(&turns, Some(text)); }
                // "Copied" for 1.4 s, then Copy again.
                let a = self.clone();
                self.page.copied.start(slint::TimerMode::SingleShot, Duration::from_millis(1400), move || {
                    let turns = a.page.turns.borrow().clone();
                    if let Some(c) = &mut *a.page.thread.borrow_mut() { c.thread.set_copied(&turns, None); }
                    a.paint_thread();
                });
            }
            hover_chat::Hit::Link(url) => crate::open_url(&url),
            _ => return,
        }
        self.paint_thread();
    }

    /// The bench's click on a painted act of the open chat's turn `turn` (`restore` or
    /// `try`): where the button is drawn, through the same handler the pointer uses.
    pub(crate) fn bench_act(self: &Rc<Self>, which: &str, turn: usize) -> bool {
        let want = if which == "restore" { hover_chat::doc::Act::Restore } else { hover_chat::doc::Act::TryAgain };
        let at = {
            let mut chat = self.page.thread.borrow_mut();
            let Some(c) = chat.as_mut() else { return false };
            let Some(s) = c.thread.sections.get(turn) else { return false };
            let Some((r, _)) = s.frag.hits.iter().find(|(_, a)| *a == want) else { return false };
            let (x, ty) = (r[0] + r[2] / 2.0 + hover_chat::theme::THREAD_PAD[3], s.y + r[1] + r[3] / 2.0);
            c.scroll = (ty - 100.0).max(0.0);
            (x, ty - c.scroll)
        };
        self.thread_click(at.0, at.1);
        true
    }
    /// newAccessOf: the access the box picked for this tool, else the tool's setting; Read
    /// only only where it works.
    fn new_access(&self, i: usize) -> &'static str {
        let t = AgentTool::ALL[i];
        let a = self.page.new_access.borrow()[i].unwrap_or_else(|| hover_agents::state::tool_access(&self.hover.settings, t));
        if a == "read" && !hover_agents::agents::read_only_works(t) { "full" } else { a }
    }

    fn ask_delete(self: &Rc<Self>, id: Option<i32>, key: Option<String>, title: &str, busy: bool) {
        *self.page.confirm_key.borrow_mut() = Some((id, key));
        *self.page.confirm_rewind.borrow_mut() = None;
        let text = format!("“{title}” goes from the office and the history{}. This can’t be undone.", if busy { ", and its run is stopped" } else { "" });
        each!(self, |g| { g.set_confirm_title(s("Delete this session?")); g.set_confirm_ok(s("Delete")); g.set_confirm_text(s(&text)); g.set_confirm(true); });
    }

    /// Restore and Try again change the project's files, so they ask first.
    fn ask_rewind(self: &Rc<Self>, to: hover_agents::session::Rewind) {
        use hover_agents::session::Rewind;
        let Some(id) = self.page.open.get() else { return };
        let Some(sess) = self.hover.sessions.get(id) else { return };
        if sess.busy() { self.toast("Stop the run first."); return; }
        if self.page.rewinding.get() { return; }
        let folder = std::path::Path::new(&sess.folder).file_name().map_or(sess.folder.clone(), |f| f.to_string_lossy().into_owned());
        let (Rewind::After(i) | Rewind::Before(i)) = to;
        let later = sess.turns.len().saturating_sub(i + 1);
        let leave = |n: usize| if n == 1 { "1 message after it leaves".to_owned() } else { format!("{n} messages after it leave") };
        let (title, yes, text) = match to {
            Rewind::After(_) => ("Restore to here?", "Restore",
                format!("The files in “{folder}” go back to how they were after this answer, and the {} this chat. Changes made since, by the agent or by you, are undone.", leave(later))),
            Rewind::Before(_) => ("Try again from here?", "Try again",
                format!("The files in “{folder}” go back to how they were before this message, and it is sent again.{} Changes made since, by the agent or by you, are undone.",
                    if later == 0 { String::new() } else { format!(" The {} this chat.", leave(later)) })),
        };
        *self.page.confirm_rewind.borrow_mut() = Some((id, to));
        *self.page.confirm_key.borrow_mut() = None;
        each!(self, |g| { g.set_confirm_title(s(title)); g.set_confirm_ok(s(yes)); g.set_confirm_text(s(&text)); g.set_confirm(true); });
    }

    /// Puts the chat and its folder back (the files take a moment in a big folder, so off
    /// the UI thread), then shows the chat as it is.
    fn rewind(self: &Rc<Self>, id: i32, to: hover_agents::session::Rewind) {
        if self.page.rewinding.replace(true) { return; }
        self.toast("Putting the files back…");
        let sessions = self.hover.sessions.clone();
        std::thread::spawn(move || {
            let r = sessions.rewind(id, to);
            crate::ui_do(move |a| {
                a.page.rewinding.set(false);
                match r {
                    Ok(()) => {
                        a.toast(if matches!(to, hover_agents::session::Rewind::Before(_)) { "Files put back. Sending it again." } else { "Files and chat put back." });
                        if let Some(c) = &mut *a.page.thread.borrow_mut() { c.scroll = f32::MAX; }
                    }
                    Err(e) => a.toast(&e),
                }
                a.office_changed();
                a.office_widgets();
            });
        });
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

/// main.js ACCESS: what a session may do on its own, picked when it starts.
pub const ACCESS: [(&str, &str, &str); 4] = [
    ("full", "Trust all", "Never asks. Edits, runs commands and goes online on its own."),
    ("risky", "Ask first", "Asks before commands, deletes, the network and anything outside the folder."),
    ("always", "Ask always", "Asks before every change and every command."),
    ("read", "Read only", "Reads and searches. Changes nothing."),
];

/// A model as the pill and its menu say it: Claude's are "Opus 5.5", "Sonnet 5", not "Claude Opus 5.5".
fn short_model(name: &str) -> String {
    if let Some(r) = name.strip_prefix("Claude ") { return r.to_owned(); }
    // An id used as the name ("claude-opus-5.5"): "Opus 5.5".
    match name.strip_prefix("claude-") {
        Some(r) if !r.is_empty() => { let r = r.replace('-', " "); let mut c = r.chars(); c.next().map_or(r.clone(), |f| f.to_uppercase().chain(c).collect()) }
        _ => name.to_owned(),
    }
}

pub(crate) fn access_label(id: &str) -> &'static str { ACCESS.iter().find(|a| a.0 == id).map_or("Trust all", |a| a.1) }

/// accessNote: Codex's Ask first is its own preset, which lets the rest run.
pub fn access_note(id: &str, tool: AgentTool) -> &'static str {
    if tool == AgentTool::Codex && id == "risky" { return "Asks to write outside the folder or go online. Codex runs the rest."; }
    ACCESS.iter().find(|a| a.0 == id).map_or("", |a| a.2)
}

/// EFFORT: "X-High" for xhigh, else the word with a capital.
fn effort_word(e: &str) -> String {
    if e == "xhigh" { return "X-High".into(); }
    let mut c = e.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// A picked picture, into kiro-images as a pasted one is (PNG, JPEG, GIF, WebP; 8 MiB).
fn attach_file(file: &str) -> Result<String, String> {
    let ext = std::path::Path::new(file).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kind = match ext.as_str() { "png" => "png", "jpg" | "jpeg" => "jpeg", "gif" => "gif", "webp" => "webp", _ => return Err("Only PNG, JPEG, GIF and WebP images.".into()) };
    let bytes = std::fs::read(file).map_err(|e| format!("Couldn’t read that image: {e}"))?;
    if bytes.len() > hover_core::images::MAX_IMAGE_BYTES { return Err("That image is over 8 MB.".into()); }
    keep_image(kind, &bytes)
}

/// Into kiro-images, named as KiroPage.SaveImages names them: the file's path.
fn keep_image(kind: &str, bytes: &[u8]) -> Result<String, String> {
    let url = format!("data:image/{kind};base64,{}", hover_agents::http::base64(bytes));
    let dir = hover_core::images::folder(hover_core::paths::support());
    hover_core::images::save(&[hover_core::json::Json::str(url)], &dir).into_iter().next().map(|p| p.to_string_lossy().into_owned()).ok_or_else(|| "Couldn’t keep that image.".into())
}

/// A pasted picture (the clipboard's bitmap, which Chromium handed the page as a PNG), as
/// main.js shrink() sent it: the long side at most 2000 px, PNG, else JPEG 90 when that
/// is still over the 8 MiB a picture may be.
fn attach_pixels(w: u32, h: u32, rgba: Vec<u8>) -> Result<String, String> {
    let (kind, bytes) = pasted_picture(w, h, rgba)?;
    keep_image(kind, &bytes)
}

/// attach_pixels' picture: its kind and bytes.
fn pasted_picture(w: u32, h: u32, rgba: Vec<u8>) -> Result<(&'static str, Vec<u8>), String> {
    let img = image::RgbaImage::from_raw(w, h, rgba).ok_or("That picture couldn’t be read.")?;
    let k = (2000.0 / w.max(h).max(1) as f64).min(1.0);
    let img = if k < 1.0 {
        image::imageops::resize(&img, ((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1), image::imageops::FilterType::Triangle)
    } else { img };
    let mut png = std::io::Cursor::new(vec![]);
    img.write_to(&mut png, image::ImageFormat::Png).map_err(|e| format!("That picture couldn’t be kept: {e}"))?;
    if png.get_ref().len() <= hover_core::images::MAX_IMAGE_BYTES { return Ok(("png", png.into_inner())); }
    // JPEG has no alpha: transparent pixels come out black, as a canvas's toDataURL.
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut jpg = std::io::Cursor::new(vec![]);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&rgb).map_err(|e| format!("That picture couldn’t be kept: {e}"))?;
    Ok(("jpeg", jpg.into_inner()))
}

/// hover-core's JSON as serde's, directly: the chat reads the session's state as the
/// page did, without writing it out as text and parsing it back.
fn to_serde(j: &hover_core::json::Json) -> serde_json::Value {
    use hover_core::json::Json;
    use serde_json::Value;
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Num(n) => n.parse::<serde_json::Number>().map_or(Value::Null, Value::Number),
        Json::Str(s) => Value::String(s.clone()),
        Json::Arr(a) => Value::Array(a.iter().map(to_serde).collect()),
        Json::Obj(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), to_serde(v))).collect()),
    }
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

/// stamp(): a history row's date, beside the day heading over it: the time today and
/// yesterday ("1:47 PM"), the weekday this week ("Mon"), and the date before that
/// ("Sep 12"), with the year when it isn't this one ("Sep 12, 2025").
fn stamp(now: f64, ms: f64) -> String {
    use hover_core::time::{local_offset_min, Kind, Stamp};
    let off = |t: f64| local_offset_min(Stamp::from_unix_ms(t as i64, Kind::Utc).ticks);
    let local_day = |t: f64| ((t + off(t) as f64 * 60e3) / 864e5).floor() as i64;
    let (d, n) = (local_day(ms), local_day(now));
    let k = n - d;
    if k < 2 { return hover_chat::state::hm(ms, off(ms)); }
    if k < 7 { return ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][d.rem_euclid(7) as usize].into(); }
    let iso = |day: i64| Stamp::from_unix_ms(day * 86_400_000, Kind::Utc).iso();
    let (st, year_now) = (iso(d), iso(n)[0..4].to_owned());
    let (y, mo, dd) = (&st[0..4], st[5..7].parse::<usize>().unwrap_or(1), st[8..10].trim_start_matches('0'));
    let m = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][mo - 1];
    if y == year_now { format!("{m} {dd}") } else { format!("{m} {dd}, {y}") }
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

/// RGBA pixels into a texture of the slot's, made (and wrapped as an image) when there
/// is none of that size yet.
#[cfg(windows)]
fn upload(device: &slint::wgpu_30::wgpu::Device, queue: &slint::wgpu_30::wgpu::Queue, slot: &mut Option<(slint::wgpu_30::wgpu::Texture, Image)>, w: u32, h: u32, rgba: &[u8]) -> Option<Image> {
    use slint::wgpu_30::wgpu;
    if slot.as_ref().is_none_or(|(t, _)| (t.width(), t.height()) != (w, h)) {
        let t = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("office frame"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm,
            // Slint takes a texture it can also render to.
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST, view_formats: &[],
        });
        let img = match Image::try_from(t.clone()) { Ok(i) => i, Err(e) => { hover_core::log::line(&format!("office texture: {e}")); return None; } };
        *slot = Some((t, img));
    }
    let (t, img) = slot.as_ref()?;
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    Some(img.clone())
}

/// backdrop-filter: blur(18px) saturate(1.4), at a quarter of the size (a blur that
/// wide loses nothing at that scale): three box passes each way make it near Gaussian
/// (sigma 18 px is a box of about 17 px at full size, 4 at a quarter). Into `b.out`
/// (RGBA, b.w x b.h), with buffers kept between frames: new ones for every frame were a
/// quarter of a MB each time.
fn blur(rgb: &[u8], w: usize, h: usize, b: &mut Blur) {
    let (sw, sh) = (w.div_ceil(4), h.div_ceil(4));
    let small = &mut b.small;
    small.clear();
    small.resize(sw * sh, [0f32; 3]);
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
            // Each row (or column) is read from a copy of itself, so the pass can write in place.
            let (lines, len) = if horizontal { (sh, sw) } else { (sw, sh) };
            let at = |line: usize, i: usize| if horizontal { line * sw + i } else { i * sw + line };
            for line in 0..lines {
                b.line.clear();
                b.line.extend((0..len).map(|i| small[at(line, i)]));
                for i in 0..len {
                    let mut acc = [0f32; 3];
                    for d in -r..=r {
                        let s = b.line[(i as i64 + d).clamp(0, len as i64 - 1) as usize];
                        for k in 0..3 { acc[k] += s[k]; }
                    }
                    small[at(line, i)] = acc.map(|v| v / (2 * r + 1) as f32);
                }
            }
        }
    }
    b.out.clear();
    b.out.reserve(sw * sh * 4);
    for s in small.iter() {
        // saturate(1.4), with the filter's luminance weights.
        let l = 0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2];
        let c = s.map(|v| (l + (v - l) * 1.4).clamp(0.0, 255.0) as u8);
        b.out.extend_from_slice(&[c[0], c[1], c[2], 255]);
    }
    (b.w, b.h) = (sw as u32, sh as u32);
}

/// blur's working buffers and its result, kept between frames.
#[derive(Default)]
struct Blur { small: Vec<[f32; 3]>, line: Vec<[f32; 3]>, out: Vec<u8>, w: u32, h: u32 }

#[cfg(test)]
mod tests {
    /// A pasted picture goes as a PNG when it fits, its long side cut to 2000 px; one
    /// that is still over 8 MiB as a PNG (noise doesn't compress) goes as a JPEG.
    #[test]
    fn a_pasted_picture_is_kept_as_the_page_sent_it() {
        let (kind, b) = super::pasted_picture(3000, 1200, vec![200; 3000 * 1200 * 4]).unwrap();
        let d = image::load_from_memory(&b).unwrap();
        assert_eq!((kind, d.width(), d.height()), ("png", 2000, 800));
        let mut x: u32 = 1;
        let noise: Vec<u8> = (0..1900 * 1900 * 4).map(|_| { x ^= x << 13; x ^= x >> 17; x ^= x << 5; x as u8 }).collect();
        let (kind, b) = super::pasted_picture(1900, 1900, noise).unwrap();
        assert_eq!(kind, "jpeg");
        assert!(b.len() <= hover_core::images::MAX_IMAGE_BYTES);
        assert!(super::pasted_picture(10, 10, vec![0; 7]).is_err(), "too few bytes for the size");
    }

    /// The blur with its buffers kept between frames gives the very bytes the one that
    /// made new ones each frame gave (that one, as it was, is the reference here).
    #[test]
    fn the_kept_buffer_blur_is_the_old_blur_byte_for_byte() {
        fn old(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
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
            for _ in 0..3 { for horizontal in [true, false] {
                let src = small.clone();
                for y in 0..sh { for x in 0..sw {
                    let mut acc = [0f32; 3];
                    for d in -r..=r {
                        let (px, py) = if horizontal { ((x as i64 + d).clamp(0, sw as i64 - 1) as usize, y) } else { (x, (y as i64 + d).clamp(0, sh as i64 - 1) as usize) };
                        for k in 0..3 { acc[k] += src[py * sw + px][k]; }
                    }
                    small[y * sw + x] = acc.map(|v| v / (2 * r + 1) as f32);
                } }
            } }
            small.iter().flat_map(|s| { let l = 0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2]; let c = s.map(|v| (l + (v - l) * 1.4).clamp(0.0, 255.0) as u8); [c[0], c[1], c[2], 255] }).collect()
        }
        let mut b = super::Blur::default();
        let mut seed = 7u32;
        for (w, h) in [(1104, 424), (37, 5), (1, 1), (824, 324)] {
            let rgb: Vec<u8> = (0..w * h * 3).map(|_| { seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345); (seed >> 16) as u8 }).collect();
            super::blur(&rgb, w, h, &mut b);
            assert_eq!((b.w as usize, b.h as usize), (w.div_ceil(4), h.div_ceil(4)));
            assert!(b.out == old(&rgb, w, h), "{w}x{h}");
        }
    }

    /// A history row's date: the time today and yesterday, the weekday this week, then
    /// the date, with the year only when it isn't this one.
    #[test]
    fn a_history_rows_date_as_the_page_gives_it() {
        // Noon local on Wednesday 30 September 2026 (a day kept clear of any DST change).
        let off = hover_core::time::local_offset_min(hover_core::time::Stamp::from_unix_ms(1_790_726_400_000, hover_core::time::Kind::Utc).ticks) as f64 * 60e3;
        let now = 1_790_726_400_000.0 - off + 12.0 * 3600e3;
        let day = 864e5;
        let today = super::stamp(now, now - 2.0 * 3600e3);
        assert!(today.ends_with(" AM") && today.starts_with("10:"), "{today}");
        assert_eq!(super::stamp(now, now - day), "12:00 PM");
        assert_eq!(super::stamp(now, now - 3.0 * day), "Sun");
        assert_eq!(super::stamp(now, now - 20.0 * day), "Sep 10");
        assert_eq!(super::stamp(now, now - 400.0 * day), "Aug 26, 2025");
    }
}
