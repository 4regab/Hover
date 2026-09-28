//! `hover --selftest DIR` on X11: notch-proto's Windows self-test, adapted. A helper
//! window covering the screen stands in for "the app underneath" and holds the focus;
//! XTEST moves the pointer, clicks and types; the X server says where the notch is,
//! what its input shape is and who has the focus. Screenshots come from the screen
//! itself. The results go to DIR/report.json.

use crate::App;
use hover_notch::State;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::shape::ConnectionExt as _;
use slint::ComponentHandle;

/// What the driver thread asks the UI thread.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen { state: State, openness: f64, notch_win: u32, win: (i32, i32, i32, i32), work: (i32, i32, i32, i32), scale: f64, rest: (f64, f64), in_settings: bool }

fn ask() -> Seen {
    let (tx, rx) = mpsc::channel();
    crate::ui_do(move |a| {
        let n = a.n.borrow();
        let _ = tx.send(Seen {
            state: n.hover.state, openness: n.openness(), notch_win: 0, win: (n.win.left, n.win.top, n.win.right, n.win.bottom),
            work: (n.work.left, n.work.top, n.work.right, n.work.bottom), scale: n.scale, rest: n.rest, in_settings: a.notch_settings.get(),
        });
    });
    rx.recv_timeout(Duration::from_secs(5)).expect("the UI thread answers")
}

fn on_ui(f: impl FnOnce(&Rc<App>) + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    crate::ui_do(move |a| { f(a); let _ = tx.send(()); });
    let _ = rx.recv_timeout(Duration::from_secs(5));
}

pub fn start(app: Rc<App>, dir: PathBuf) {
    std::fs::create_dir_all(&dir).unwrap();
    let notch_win = crate::x11::window_of(app.notch.window()).unwrap_or(0);
    std::thread::spawn(move || {
        let report = run(&dir, notch_win);
        std::fs::write(dir.join("report.json"), report).unwrap();
        crate::ui_do(|_| { let _ = slint::quit_event_loop(); });
    });
}

struct T { checks: Vec<(String, bool, String)> }

impl T {
    fn check(&mut self, name: &str, ok: bool, detail: String) {
        eprintln!("selftest {} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        self.checks.push((name.into(), ok, detail));
    }
}

fn run(dir: &std::path::Path, notch: u32) -> String {
    let (c, n) = x11rb::connect(None).expect("the display");
    let s = c.setup().roots[n].clone();
    let root = s.root;
    let mut t = T { checks: vec![] };
    let atom = |name: &str| c.intern_atom(false, name.as_bytes()).unwrap().reply().unwrap().atom;
    let focus = || c.get_input_focus().unwrap().reply().unwrap().focus;
    let pause = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
    let motion = |x: i16, y: i16| { let _ = c.xtest_fake_input(xproto::MOTION_NOTIFY_EVENT, 0, 0, root, x, y, 0); let _ = c.flush(); };
    let button = |press: bool| { let _ = c.xtest_fake_input(if press { xproto::BUTTON_PRESS_EVENT } else { xproto::BUTTON_RELEASE_EVENT }, 1, 0, root, 0, 0, 0); let _ = c.flush(); };
    let keycode = |sym: u32| {
        let m = c.get_keyboard_mapping(s_min(&c), s_max(&c) - s_min(&c) + 1).unwrap().reply().unwrap();
        let per = m.keysyms_per_keycode as usize;
        m.keysyms.chunks(per).position(|k| k.contains(&sym)).map(|i| s_min(&c) + i as u8).unwrap_or(0)
    };
    let key = |code: u8, press: bool| { let _ = c.xtest_fake_input(if press { xproto::KEY_PRESS_EVENT } else { xproto::KEY_RELEASE_EVENT }, code, 0, root, 0, 0, 0); let _ = c.flush(); };
    let shot = |name: &str, r: (i32, i32, i32, i32)| {
        let (w, h) = ((r.2 - r.0) as u16, (r.3 - r.1) as u16);
        if let Ok(Ok(img)) = c.get_image(xproto::ImageFormat::Z_PIXMAP, root, r.0 as i16, r.1 as i16, w, h, !0).map(|x| x.reply()) {
            let mut out = image::RgbImage::new(w as u32, h as u32);
            for (i, p) in img.data.chunks(4).enumerate() { out.put_pixel((i % w as usize) as u32, (i / w as usize) as u32, image::Rgb([p[2], p[1], p[0]])); }
            let _ = out.save(dir.join(name));
        }
    };

    // The app underneath: a window over the whole screen in a colour of its own, focused.
    let helper = c.generate_id().unwrap();
    c.create_window(s.root_depth, helper, root, 0, 0, s.width_in_pixels, s.height_in_pixels, 0, xproto::WindowClass::INPUT_OUTPUT, 0,
        &xproto::CreateWindowAux::new().background_pixel(0x3a4a5e).event_mask(xproto::EventMask::BUTTON_PRESS | xproto::EventMask::KEY_PRESS)).unwrap();
    c.map_window(helper).unwrap();
    c.flush().unwrap();
    pause(300);
    c.set_input_focus(xproto::InputFocus::PARENT, helper, x11rb::CURRENT_TIME).unwrap();
    // The notch above it again (it was mapped first).
    on_ui(|a| { let n = a.n.borrow(); crate::notch::Plat::raise(&*n.plat); });
    // A quota on, so the rest is a pill that can be clicked.
    on_ui(|a| { a.hover.settings.set_notch_item("claude", true); a.update_rest(); });
    pause(700);

    let seen = ask();
    let geo = c.get_geometry(notch).unwrap().reply().unwrap();
    let tr = c.translate_coordinates(notch, root, 0, 0).unwrap().reply().unwrap();
    let cx = (seen.work.0 + seen.work.2) / 2;
    let expect_w = seen.win.2 - seen.win.0;
    t.check("placement_primary_work_area", tr.dst_x as i32 == cx - expect_w / 2 && tr.dst_y as i32 == seen.work.1 && geo.width as i32 == expect_w,
        format!("window at {},{} size {}x{}; work {:?}, scale {}", tr.dst_x, tr.dst_y, geo.width, geo.height, seen.work, seen.scale));
    let attrs = c.get_window_attributes(notch).unwrap().reply().unwrap();
    let wtype = c.get_property(false, notch, atom("_NET_WM_WINDOW_TYPE"), xproto::AtomEnum::ATOM, 0, 4).unwrap().reply().unwrap();
    let dock = wtype.value32().is_some_and(|mut v| v.any(|a| a == atom("_NET_WM_WINDOW_TYPE_DOCK")));
    t.check("override_redirect_dock_argb", attrs.override_redirect && dock && geo.depth == 32,
        format!("override-redirect {}, dock {dock}, depth {}", attrs.override_redirect, geo.depth));
    t.check("resting_notch_leaves_focus", focus() == helper, format!("focus 0x{:x}, helper 0x{helper:x}", focus()));

    let shape = c.shape_get_rectangles(notch, x11rb::protocol::shape::SK::INPUT).unwrap().reply().unwrap();
    let rects: Vec<_> = shape.rectangles.iter().map(|r| (r.x, r.y, r.width, r.height)).collect();
    let pill_w = (seen.rest.0 * seen.scale).round() as i32;
    let one = rects.len() == 1 && (rects[0].2 as i32 - pill_w).abs() <= 2;
    t.check("input_shape_is_the_pill", one, format!("input shape {rects:?}, rest {:?}", seen.rest));
    // A click on the empty part lands on the helper, not the notch.
    let (qx, qy) = (seen.win.0 as i16 + 60, seen.win.1 as i16 + 200);
    motion(qx, qy);
    pause(100);
    let under = c.query_pointer(root).unwrap().reply().unwrap().child;
    t.check("click_through_empty_area", under == helper, format!("under the pointer at {qx},{qy}: 0x{under:x}"));
    shot("x11-rest-pill.png", (seen.win.0, seen.win.1, seen.win.2, seen.win.1 + 80));

    // Hover the top centre: a peek, without the focus.
    motion(cx as i16, seen.work.1 as i16 + 2);
    // The first opening says hello: 940 ms.
    pause(1300);
    let peek = ask();
    t.check("hover_peeks_without_focus", peek.state == State::Peek && peek.openness > 0.99 && focus() == helper,
        format!("state {:?}, openness {:.2}, focus 0x{:x}", peek.state, peek.openness, focus()));
    shot("x11-peek.png", (seen.win.0, seen.win.1, seen.win.2, seen.win.3));
    let shape = c.shape_get_rectangles(notch, x11rb::protocol::shape::SK::INPUT).unwrap().reply().unwrap();
    t.check("input_shape_grows_open", shape.rectangles.first().is_some_and(|r| r.height as f64 >= 400.0 * seen.scale),
        format!("{:?}", shape.rectangles.iter().map(|r| (r.x, r.y, r.width, r.height)).collect::<Vec<_>>()));
    // Leave: it folds after the leave grace.
    motion(qx, (seen.win.3 + 100) as i16);
    pause(900);
    let gone = ask();
    t.check("peek_folds_after_leave_grace", gone.state == State::Rest && gone.openness < 0.01, format!("state {:?}, openness {:.2}", gone.state, gone.openness));

    // The shortcut (Alt+N): open with the keyboard.
    let (alt, n_key, esc) = (keycode(0xffe9), keycode(0x6e), keycode(0xff1b));
    key(alt, true); key(n_key, true); key(n_key, false); key(alt, false);
    pause(700);
    let open = ask();
    let has = focus();
    t.check("hotkey_opens_with_keyboard", open.state == State::Open && has == notch, format!("state {:?}, focus 0x{has:x}, notch 0x{notch:x}", open.state));
    shot("x11-open.png", (seen.win.0, seen.win.1, seen.win.2, seen.win.3));
    // The gear: Settings over the office.
    on_ui(|a| a.show_settings_in(0, hover_app::pages::Section::Integrations));
    pause(300);
    shot("x11-open-settings.png", (seen.win.0, seen.win.1, seen.win.2, seen.win.3));
    on_ui(|a| { a.notch_settings.set(false); a.notch.set_in_settings(false); });
    // Esc folds it and gives the focus back.
    key(esc, true); key(esc, false);
    pause(600);
    let back = ask();
    t.check("esc_collapses_and_restores_focus", back.state == State::Rest && focus() == helper, format!("state {:?}, focus 0x{:x}", back.state, focus()));

    // A click on the pill opens it (in place, no focus taken).
    motion(cx as i16, seen.work.1 as i16 + 12);
    pause(60);
    button(true); button(false);
    pause(700);
    let clicked = ask();
    t.check("click_on_pill_opens", clicked.state != State::Rest, format!("state {:?}", clicked.state));
    // Click away onto the helper: it folds.
    on_ui(|a| a.collapse());
    pause(500);

    // A taken shortcut: someone else grabs Ctrl+Alt+K first; Hover must say so.
    let k = keycode(0x6b);
    let _ = c.grab_key(true, root, xproto::ModMask::CONTROL | xproto::ModMask::M1, k, xproto::GrabMode::ASYNC, xproto::GrabMode::ASYNC).unwrap().check();
    let (tx, rx) = mpsc::channel();
    crate::ui_do(move |a| {
        let sc = hover_core::shortcut::Shortcut { key: hover_core::shortcut::Key(44 + 10), modifiers: hover_core::shortcut::Modifiers::CONTROL | hover_core::shortcut::Modifiers::ALT };
        let ok = a.hotkey.borrow().as_ref().is_none_or(|f| f(&sc));
        let _ = tx.send(ok);
        // And back to the default.
        let d = hover_core::shortcut::Shortcut::DEFAULT;
        if let Some(f) = a.hotkey.borrow().as_ref() { f(&d); }
    });
    let refused = !rx.recv_timeout(Duration::from_secs(5)).unwrap_or(true);
    t.check("taken_shortcut_is_refused", refused, format!("Ctrl+Alt+K held by another client: refused {refused}"));

    // Idle: nothing redraws while resting.
    let frames0 = frames();
    pause(3000);
    let frames1 = frames();
    t.check("no_redraws_while_resting", frames1 == frames0, format!("{} frames in 3 s", frames1 - frames0));

    let _ = c.destroy_window(helper);
    let _ = c.flush();
    let passed = t.checks.iter().filter(|c| c.1).count();
    let items: Vec<String> = t.checks.iter().map(|(n, ok, d)| format!("    {{ \"check\": \"{n}\", \"pass\": {ok}, \"detail\": {:?} }}", d)).collect();
    format!("{{\n  \"display\": \"X11\",\n  \"passed\": {passed},\n  \"total\": {},\n  \"checks\": [\n{}\n  ]\n}}\n", t.checks.len(), items.join(",\n"))
}

fn s_min(c: &x11rb::rust_connection::RustConnection) -> u8 { c.setup().min_keycode }
fn s_max(c: &x11rb::rust_connection::RustConnection) -> u8 { c.setup().max_keycode }

/// The notch's redraws so far (counted by the rendering notifier).
fn frames() -> u64 { crate::FRAMES.load(std::sync::atomic::Ordering::Relaxed) }
