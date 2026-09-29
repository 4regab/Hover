//! The windows' shared view state: the palette in use (Theme), Settings over the office
//! (OfficeView, SettingsPage) and what each click in it does (Pages.cs's handlers).
//! Every window with an office gets the same state through its own Slint globals.

use crate::ui::*;
use hover_app::keys::{self, Recorded};
use hover_app::pages::{self, Block as B, Control, Lead, Section, Tint};
use hover_core::model::{AgentTool, SavedTheme};
use hover_core::palette::{InstalledTheme, Palette};
use hover_core::platform::Autostart;
use slint::{Color, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub fn argb(c: u32) -> Color { Color::from_argb_u8((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8) }

/// Theme.Publish: the palette's colours into a window's Pal global.
macro_rules! publish {
    ($w:expr, $p:expr, $motion:expr) => {{
        let g = $w.global::<crate::ui::Pal>();
        let p = $p;
        g.set_dark(p.dark);
        g.set_ink(crate::view::argb(p.ink)); g.set_ink_dim(crate::view::argb(p.ink_dim)); g.set_ink_faint(crate::view::argb(p.ink_faint));
        g.set_fill(crate::view::argb(p.fill)); g.set_wash(crate::view::argb(p.wash)); g.set_wash_strong(crate::view::argb(p.wash_strong));
        g.set_separator(crate::view::argb(p.separator)); g.set_surface(crate::view::argb(p.surface)); g.set_sheet(crate::view::argb(p.sheet));
        g.set_sheet_edge(crate::view::argb(p.sheet_edge)); g.set_panel(crate::view::argb(p.panel)); g.set_panel_edge(crate::view::argb(p.panel_edge));
        g.set_thumb(crate::view::argb(p.thumb)); g.set_row_hover(crate::view::argb(p.row_hover)); g.set_switch_off(crate::view::argb(p.switch_off));
        g.set_handle(crate::view::argb(p.handle)); g.set_blue(crate::view::argb(p.blue)); g.set_green(crate::view::argb(p.green)); g.set_purple(crate::view::argb(p.purple));
        g.set_yellow(crate::view::argb(p.yellow)); g.set_teal(crate::view::argb(p.teal)); g.set_orange(crate::view::argb(p.orange)); g.set_red(crate::view::argb(p.red));
        g.set_motion($motion);
    }};
}
pub(crate) use publish;

/// The picker's menu while open: the row that asked, its options, where.
pub type OpenMenu = (String, Vec<(String, bool)>, f32, f32);

/// Page's state that isn't in the settings.
pub struct Pane {
    pub section: Section,
    pub recording: bool,
    /// The shortcut field's words while recording ("Press keys…", the modifier hint).
    pub field: Option<String>,
    pub import_status: String,
    pub menu: Option<OpenMenu>,
    /// Palette.Installed with each theme read, once per run (as the C#'s Lazy).
    pub installed: Option<Rc<Vec<(InstalledTheme, SavedTheme)>>>,
}

impl Default for Pane {
    fn default() -> Self { Pane { section: Section::General, recording: false, field: None, import_status: String::new(), menu: None, installed: None } }
}

pub fn installed(p: &mut Pane) -> Rc<Vec<(InstalledTheme, SavedTheme)>> {
    p.installed.get_or_insert_with(|| {
        Rc::new(hover_core::palette::installed().into_iter().filter_map(|s| hover_core::palette::read(&s.path, Some(&s.label), Some(s.dark)).map(|t| (s, t))).collect())
    }).clone()
}

fn tint(t: Tint, p: &Palette) -> Color {
    match t {
        Tint::Gray => Color::from_rgb_u8(0x8e, 0x8e, 0x93),
        Tint::Bot => Color::from_rgb_u8(0x9b, 0x6b, 0xff),
        Tint::Purple => argb(p.purple), Tint::Green => argb(p.green), Tint::Blue => argb(p.blue),
        Tint::Orange => argb(p.orange), Tint::Teal => argb(p.teal),
    }
}

fn s(v: impl AsRef<str>) -> SharedString { SharedString::from(v.as_ref()) }
fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> { ModelRc::new(VecModel::from(v)) }

/// The rows of a model already shown, changed in place: a new model makes each repeated
/// element anew, and one made again between a press and its release (the office's tags
/// at 10–30 fps, rows while a session runs) loses the click. Returns the model to set
/// when there is none yet to change.
pub fn sync<T: Clone + PartialEq + 'static>(cur: ModelRc<T>, v: &[T]) -> Option<ModelRc<T>> {
    use slint::Model;
    let Some(m) = cur.as_any().downcast_ref::<VecModel<T>>() else { return Some(model(v.to_vec())) };
    for (i, t) in v.iter().enumerate() {
        if i < m.row_count() { if m.row_data(i).as_ref() != Some(t) { m.set_row_data(i, t.clone()); } } else { m.push(t.clone()); }
    }
    while m.row_count() > v.len() { m.remove(m.row_count() - 1); }
    None
}

/// Settings' blocks, changed in place down to their rows: the page is built again on
/// every change, and a row made anew loses what it holds (the shortcut field's
/// keyboard focus while it records, a press).
pub fn sync_blocks(cur: ModelRc<Block>, v: Vec<Block>) -> Option<ModelRc<Block>> {
    use slint::Model;
    let Some(m) = cur.as_any().downcast_ref::<VecModel<Block>>() else { return Some(model(v)) };
    let n = v.len();
    for (i, mut b) in v.into_iter().enumerate() {
        if let Some(old) = (i < m.row_count()).then(|| m.row_data(i)).flatten().filter(|o| o.kind == b.kind) {
            let rows: Vec<RowData> = b.rows.iter().collect();
            b.rows = sync(old.rows.clone(), &rows).unwrap_or(old.rows.clone());
            let tiles: Vec<TileData> = b.tiles.iter().collect();
            b.tiles = sync(old.tiles.clone(), &tiles).unwrap_or(old.tiles.clone());
            if old != b { m.set_row_data(i, b); }
        } else if i < m.row_count() { m.set_row_data(i, b); } else { m.push(b); }
    }
    while m.row_count() > n { m.remove(m.row_count() - 1); }
    None
}

/// pages.rs's blocks as Slint's.
pub fn blocks(bs: &[B], p: &Palette) -> Vec<Block> {
    bs.iter().map(|b| {
        let mut o = Block::default();
        match b {
            B::Title(t) => { o.kind = 0; o.text = s(t); }
            B::Heading(t, first) => { o.kind = 1; o.text = s(t); o.first = *first; }
            B::Footnote(t) => { o.kind = 3; o.text = s(t); }
            B::Link { id, name, icon, text, dim, status } => {
                o.kind = 5; o.id = s(id); o.name = s(name); o.icon = s(icon_path(icon)); o.text = s(text); o.dim = *dim; o.status = s(status);
            }
            B::Tiles(ts) => {
                o.kind = 4;
                o.tiles = model(ts.iter().map(|t| TileData {
                    id: s(&t.id), name: s(&t.name), from: s(&t.from), picked: t.picked,
                    panel: argb(t.palette.panel), surface: argb(t.palette.surface), ink: argb(t.palette.ink), ink_dim: argb(t.palette.ink_dim),
                    blue: argb(t.palette.blue), green: argb(t.palette.green), orange: argb(t.palette.orange), red: argb(t.palette.red),
                    purple: argb(t.palette.purple), teal: argb(t.palette.teal),
                }).collect());
            }
            B::Group(rows) => {
                o.kind = 2;
                o.rows = model(rows.iter().map(|r| {
                    let mut d = RowData { label: s(&r.label), sub: s(r.sub.as_deref().unwrap_or("")), sub_id: s(r.sub_id.as_deref().unwrap_or("")), enabled: r.enabled, button_enabled: true, ring: -1.0, picked: -1, ..Default::default() };
                    match &r.control {
                        Control::None => d.control = 0,
                        Control::Switch { id, name, on } => { d.control = 1; d.id = s(id); d.name = s(name); d.on = *on; }
                        Control::Button { id, name, text, enabled } => { d.control = 2; d.id = s(id); d.name = s(name); d.text = s(text); d.button_enabled = *enabled; }
                        Control::Shortcut { text } => { d.control = 3; d.text = s(text); }
                        Control::Segments { id, labels, picked } => {
                            d.control = 4; d.id = s(id); d.picked = *picked;
                            d.longest = s(labels.iter().max_by_key(|l| l.chars().count()).cloned().unwrap_or_default());
                            d.labels = model(labels.iter().map(s).collect());
                        }
                        Control::Picker { id, name, shown, options } => {
                            d.control = 5; d.id = s(id); d.name = s(name); d.text = s(shown);
                            d.options = model(options.iter().map(|(l, on)| Opt { label: s(l), on: *on }).collect());
                        }
                        Control::Text(t) => { d.control = 6; d.text = s(t); }
                    }
                    match &r.lead {
                        Lead::None => d.lead = 0,
                        Lead::Tile(icon, t) => { d.lead = 1; d.icon = s(icon_path(icon)); d.tint = tint(*t, p); }
                        Lead::Ring(v) => { d.lead = 2; d.ring = v.map_or(-1.0, |x| x as f32); }
                    }
                    d
                }).collect());
            }
        }
        o
    }).collect()
}

/// An icon's path data from icons.slint's table, by its Lucide name.
pub fn icon_path(name: &str) -> String {
    crate::icons::path(name).to_owned()
}

pub fn sections(p: &Palette) -> Vec<Side> {
    Section::ALL.iter().map(|x| {
        let (icon, t) = x.glyph();
        Side { title: s(x.title()), icon: s(icon_path(icon)), tint: tint(t, p) }
    }).collect()
}

/// A handler for what Settings asks (the app implements it).
pub trait Host {
    fn hover(&self) -> &hover_app::app::Hover;
    fn system_dark(&self) -> bool;
    /// Something the notch or the tray draw from changed (OwlApp.SettingsChanged).
    fn settings_changed(&self);
    fn theme_changed(&self);
    fn shortcut_changed(&self);
    fn quit(&self);
    fn choose_folder(&self) -> Option<String>;
    fn choose_theme_file(&self) -> Option<std::path::PathBuf>;
    /// Page's state was rebuilt: push it to the windows.
    fn refresh(&self);
    /// A tool's status check finished off the UI thread: rebuild if still showing it.
    fn recheck(&self, tool: AgentTool, fresh: bool);
}

pub fn build(h: &dyn Host, pane: &mut Pane) -> Vec<B> {
    let hv = h.hover();
    let installed = installed(pane);
    let field = pane.field.clone().unwrap_or_else(|| hv.settings.sc_workspace().label());
    let reading = |id: &str| hv.quotas.reading(id);
    let ready = |t: AgentTool| hover_agents::agents::known(t);
    let input = pages::Input {
        settings: &hv.settings,
        launch_at_login: hover_core::platform::SystemAutostart.enabled(),
        shortcut: field,
        reading: &reading,
        ready: &ready,
        installed: &installed,
        system_dark: h.system_dark(),
        import_status: pane.import_status.clone(),
        kiro_agents: hover_agents::kiro_agents(hv.settings.kiro_folder().as_deref()),
    };
    pages::build(pane.section, &input)
}

/// What a click in Settings does; the page is rebuilt after each.
pub fn toggled(h: &dyn Host, id: &str, on: bool) {
    let hv = h.hover();
    let st = &hv.settings;
    match id {
        "LaunchAtLogin" => {
            if let Err(e) = hover_core::platform::SystemAutostart.set(on) { hover_core::log::line(&format!("launch at login: {e}")); }
        }
        "HoverOpens" => st.set_hover_opens_workspace(on),
        "KiroRequireMcp" => st.set_agent_options(AgentTool::Kiro, hover_core::model::AgentOptions { require_mcp: on, ..st.agent_options(AgentTool::Kiro) }),
        _ if id.starts_with("NotchItem") => {
            st.set_notch_item(&id["NotchItem".len()..], on);
            h.settings_changed();
            hv.refresh_quotas(true);
        }
        _ if id.ends_with("ShowSteps") => {
            let t = tool_of(&id[..id.len() - "ShowSteps".len()]);
            st.set_agent_options(t, hover_core::model::AgentOptions { hide_steps: !on, ..st.agent_options(t) });
        }
        _ => {}
    }
    h.refresh();
}

fn tool_of(name: &str) -> AgentTool { AgentTool::ALL.into_iter().find(|t| t.name() == name).unwrap_or(AgentTool::Kiro) }

pub fn pressed(h: &dyn Host, pane: &RefCell<Pane>, id: &str) {
    let hv = h.hover();
    match id {
        "Quit" => { h.quit(); return; }
        "RefreshQuotas" => hv.refresh_quotas(true),
        "ImportTheme" => {
            if let Some(f) = h.choose_theme_file() {
                match hover_core::palette::read(&f, None, None) {
                    Some(t) => { apply_theme(h, pane, Some(t)); return; }
                    None => pane.borrow_mut().import_status = "That file has no VS Code theme colours in it.".into(),
                }
            }
        }
        "SettingsKiroFolder" => { if let Some(f) = h.choose_folder() { hv.settings.set_kiro_folder(Some(&f)); } }
        "KiroNoticeAgain" => { hv.settings.set_kiro_notice_seen(false); hv.sessions.raise_changed(); }
        _ if id.ends_with("Recheck") => { h.recheck(tool_of(&id[..id.len() - "Recheck".len()]), true); }
        _ => {}
    }
    h.refresh();
}

pub fn apply_theme(h: &dyn Host, pane: &RefCell<Pane>, t: Option<SavedTheme>) {
    h.hover().settings.set_theme(t);
    pane.borrow_mut().import_status.clear();
    h.theme_changed();
    h.refresh();
}

pub fn tile(h: &dyn Host, pane: &RefCell<Pane>, id: &str) {
    if id == "ThemeHover" { return apply_theme(h, pane, None); }
    let inst = installed(&mut pane.borrow_mut());
    if let Some((_, t)) = inst.iter().find(|(s, _)| format!("Theme{}", s.label) == id) { apply_theme(h, pane, Some(t.clone())); }
}

pub fn picked_seg(h: &dyn Host, id: &str, i: usize) {
    let hv = h.hover();
    let st = &hv.settings;
    match id {
        "WorkspaceSize" => { st.set_workspace_size(pages::SIZES[i].0); h.settings_changed(); }
        "Appearance" => { st.set_theme(None); st.set_appearance(pages::APPEARANCES[i].0); h.theme_changed(); }
        _ => {
            for t in AgentTool::ALL {
                let o = st.agent_options(t);
                let n = t.name();
                let offers = st.agent_offers(t);
                let new = if id == format!("{n}Effort") { pages::pick_effort(t, &o, &offers, i) }
                    else if id == format!("{n}Tools") {
                        use hover_core::model::AgentApproval as A;
                        // Read only keeps the asking it had; the rest are full access.
                        let approval = match i { 1 => A::Risky, 2 => A::Always, 3 => o.approval, _ => A::Autopilot };
                        hover_core::model::AgentOptions { read_only: i == 3, approval, ..o }
                    }
                    else if id == format!("{n}Idle") { hover_core::model::AgentOptions { idle_minutes: hover_core::model::AgentOptions::IDLE_CHOICES[i], ..o } }
                    else { continue };
                st.set_agent_options(t, new);
            }
        }
    }
    h.refresh();
}

pub fn menu_pick(h: &dyn Host, pane: &RefCell<Pane>, id: &str, i: usize) {
    pane.borrow_mut().menu = None;
    let st = &h.hover().settings;
    for t in AgentTool::ALL {
        let o = st.agent_options(t);
        let offers = st.agent_offers(t);
        if id == format!("{}Model", t.name()) { st.set_agent_options(t, pages::pick_model(t, &o, &offers, i)); }
        if t == AgentTool::OpenCode && id == "OpenCodeAgent" { st.set_agent_options(t, pages::pick_opencode_agent(&o, &offers, i)); }
        if t == AgentTool::Kiro && id == "KiroAgent" {
            st.set_agent_options(t, pages::pick_agent(&o, &offers, &hover_agents::kiro_agents(st.kiro_folder().as_deref()), i));
        }
    }
    h.refresh();
}

/// The shortcut field: click to record, then a chord (ShortcutField).
pub fn record(h: &dyn Host, pane: &RefCell<Pane>) {
    let mut p = pane.borrow_mut();
    if p.recording { p.recording = false; p.field = None; } else { p.recording = true; p.field = Some("Press keys…".into()); }
    drop(p);
    h.refresh();
}

pub fn chord(h: &dyn Host, pane: &RefCell<Pane>, text: &str, m: hover_core::shortcut::Modifiers) -> bool {
    if !pane.borrow().recording { return false; }
    match keys::record(text, m) {
        Recorded::Wait => return true,
        Recorded::NeedModifier => { pane.borrow_mut().field = Some(if cfg!(windows) { "Add Ctrl, Alt, Shift or Win" } else { "Add Ctrl, Alt, Shift or Super" }.into()); }
        Recorded::Stop => { let mut p = pane.borrow_mut(); p.recording = false; p.field = None; }
        Recorded::Chord(sc) => {
            let st = &h.hover().settings;
            let changed = sc != st.sc_workspace();
            if changed { st.set_sc_workspace(sc); }
            { let mut p = pane.borrow_mut(); p.recording = false; p.field = None; }
            if changed { h.shortcut_changed(); }
        }
    }
    h.refresh();
    true
}

/// Pushes the page into one window's Page global.
macro_rules! show_page {
    ($w:expr, $pane:expr, $blocks:expr, $pal:expr) => {{
        let g = $w.global::<crate::ui::Page>();
        let pane = $pane;
        if let Some(m) = crate::view::sync(g.get_sections(), &crate::view::sections($pal)) { g.set_sections(m); }
        g.set_current(pane.section as i32);
        if let Some(m) = crate::view::sync_blocks(g.get_blocks(), $blocks) { g.set_blocks(m); }
        g.set_recording(pane.recording);
        match &pane.menu {
            Some((id, opts, x, y)) => {
                g.set_menu_open(true);
                g.set_menu_id(id.as_str().into());
                g.set_menu_x(*x);
                g.set_menu_y(*y);
                g.set_menu(crate::view::model_of(opts.iter().map(|(l, on)| crate::ui::Opt { label: l.as_str().into(), on: *on }).collect()));
            }
            None => g.set_menu_open(false),
        }
    }};
}
pub(crate) use show_page;

pub fn model_of<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> { model(v) }

/// The picker's options for a row, from the blocks shown.
pub fn picker_options(bs: &[B], id: &str) -> Vec<(String, bool)> {
    for b in bs {
        if let B::Group(rows) = b {
            for r in rows {
                if let Control::Picker { id: rid, options, .. } = &r.control { if rid == id { return options.clone(); } }
            }
        }
    }
    vec![]
}

/// Wires one window's Page and Office globals to the app.
macro_rules! wire_page {
    ($w:expr, $app:expr, $which:expr) => {{
        let g = $w.global::<crate::ui::Page>();
        let a = $app.clone();
        g.on_section(move |i| { a.pane.borrow_mut().section = hover_app::pages::Section::ALL[i as usize]; a.pane.borrow_mut().menu = None; a.refresh_page(true); });
        let a = $app.clone();
        g.on_toggled(move |id, on| crate::view::toggled(&*a, &id, on));
        let a = $app.clone();
        g.on_pressed(move |id| crate::view::pressed(&*a, &a.pane, &id));
        let a = $app.clone();
        g.on_picked_seg(move |id, i| crate::view::picked_seg(&*a, &id, i as usize));
        let a = $app.clone();
        g.on_open_picker(move |id, x, y| {
            let opts = crate::view::picker_options(&a.last_blocks.borrow(), &id);
            a.pane.borrow_mut().menu = Some((id.to_string(), opts, x, y));
            a.refresh_page(false);
        });
        let a = $app.clone();
        g.on_menu_pick(move |id, i| crate::view::menu_pick(&*a, &a.pane, &id, i as usize));
        let a = $app.clone();
        g.on_menu_close(move || { a.pane.borrow_mut().menu = None; a.refresh_page(false); });
        let a = $app.clone();
        g.on_tile(move |id| crate::view::tile(&*a, &a.pane, &id));
        let a = $app.clone();
        g.on_record(move || crate::view::record(&*a, &a.pane));
        let a = $app.clone();
        g.on_chord(move |e| {
            let m = hover_app::keys::modifiers(e.modifiers.alt, e.modifiers.control, e.modifiers.shift, e.modifiers.meta);
            crate::view::chord(&*a, &a.pane, &e.text, m)
        });
        let o = $w.global::<crate::ui::Office>();
        let a = $app.clone();
        o.on_toggle_beats(move || a.toggle_beats());
        let a = $app.clone();
        o.on_open_settings(move || a.show_settings_in($which, hover_app::pages::Section::General));
        let a = $app.clone();
        o.on_fold(move || a.collapse());
        let a = $app.clone();
        o.on_open_app(move || a.open_dashboard(false));
    }};
}
pub(crate) use wire_page;
