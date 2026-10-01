//! hover-measure: Hover's memory and timing measured from outside the process.
//!
//!   hover-measure sample --pid PID --out samples.csv [--interval-ms 250] [--seconds N]
//!   hover-measure run --exe hoverai.exe --script S.hms --out DIR [--data DIR] [--interval-ms 250]
//!                     [--env K=V]... [--var K=V]... [--path-first DIR]
//!   hover-measure summarize DIR... [--md out.md]
//!   hover-measure elements --pid PID        (Windows: the named controls UI Automation sees)
//!
//! A run starts the app with HOVER_BENCH=1 (its stdin/stdout line channel, bench.rs),
//! plays the script, samples the whole process tree into DIR/samples.csv and writes
//! DIR/markers.csv. Script lines (see tools/scenarios/*.hms):
//!
//!   launch                    start the app (timed: "startup" ends at `bench visible`)
//!   send LINE                 a bench command on the app's stdin
//!   expect PREFIX [SECS] [as LABEL]   wait for an output line starting with PREFIX;
//!                             with a label, the time since the last action is recorded
//!   mark NAME                 a scenario starts here (it ends at the next mark)
//!   sleep SECS
//!   repeat N ... end
//!   move X Y | click X Y | wheel X Y N    real pointer input (physical pixels;
//!                             X may be `notch` for the primary display's top centre)
//!   press CHORD               real keys, e.g. alt+n, esc, shift+enter
//!   type TEXT                 real typed characters
//!   click-name NAME [SECS]    Windows: the control UI Automation names NAME (exact, or
//!                             the start of its name with a trailing *), clicked in its middle
//!   expect-name NAME [SECS]   Windows: wait until such a control is on screen
//!   expect-no-name NAME [SECS]
//!   quit [SECS]               ask the app to quit; timed as "exit"; its tree must be gone
//!   kill                      end the app's tree at once (a crash)
//!   note TEXT                 a line in the markers

mod gpu;
mod input;
mod procs;
mod regions;
mod sampler;
mod summary;

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn arg_all(args: &[String], f: &str) -> Vec<String> {
    args.windows(2).filter(|w| w[0] == f).map(|w| w[1].clone()).collect()
}
fn arg(args: &[String], f: &str) -> Option<String> { arg_all(args, f).into_iter().next() }

fn main() {
    let args: Vec<String> = std::env::args().collect();
    input::init();
    let code = match args.get(1).map(String::as_str) {
        Some("sample") => {
            let pid: u32 = arg(&args, "--pid").and_then(|p| p.parse().ok()).expect("--pid");
            let out = PathBuf::from(arg(&args, "--out").expect("--out"));
            let every = Duration::from_millis(arg(&args, "--interval-ms").and_then(|v| v.parse().ok()).unwrap_or(250));
            let secs: u64 = arg(&args, "--seconds").and_then(|v| v.parse().ok()).unwrap_or(u64::MAX / 2);
            let s = sampler::Sampler::start(out, every, pid);
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(secs) && alive(pid) { std::thread::sleep(Duration::from_millis(200)); }
            drop(s);
            0
        }
        Some("run") => match run(&args) { Ok(()) => 0, Err(e) => { eprintln!("hover-measure: {e}"); 1 } },
        Some("summarize") => {
            let dirs: Vec<PathBuf> = args[2..].iter().take_while(|a| !a.starts_with("--")).map(PathBuf::from).collect();
            let md = summary::report(&dirs);
            match arg(&args, "--md") { Some(p) => { std::fs::write(&p, &md).unwrap(); println!("{p}"); } None => print!("{md}") }
            0
        }
        Some("regions") => {
            let pid: u32 = arg(&args, "--pid").and_then(|p| p.parse().ok()).expect("--pid");
            let top: usize = arg(&args, "--top").and_then(|p| p.parse().ok()).unwrap_or(30);
            match regions::run(pid, top) { Ok(s) => { print!("{s}"); 0 } Err(e) => { eprintln!("{e}"); 1 } }
        }
        Some("elements") => {
            let pid: u32 = arg(&args, "--pid").and_then(|p| p.parse().ok()).expect("--pid");
            match input::elements(pid) {
                Ok(v) => { for e in v { println!("{:>6} {:?} {}", e.role, e.rect, e.name); } 0 }
                Err(e) => { eprintln!("{e}"); 1 }
            }
        }
        _ => { eprintln!("hover-measure sample|run|summarize|elements (see the top of src/main.rs)"); 2 }
    };
    std::process::exit(code);
}

fn alive(pid: u32) -> bool { procs::list().iter().any(|p| p.pid == pid) }

/// The script, its repeats unrolled.
fn parse(text: &str, vars: &[(String, String)]) -> Result<Vec<Vec<String>>, String> {
    fn words(l: &str) -> Vec<String> {
        let mut out = vec![];
        let mut cur = String::new();
        let mut q = false;
        for c in l.chars() {
            match c {
                '"' => q = !q,
                c if c.is_whitespace() && !q => { if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } }
                c => cur.push(c),
            }
        }
        if !cur.is_empty() { out.push(cur); }
        out
    }
    let mut lines: Vec<Vec<String>> = vec![];
    for l in text.lines() {
        let mut l = l.trim().to_owned();
        if l.is_empty() || l.starts_with('#') { continue; }
        for (k, v) in vars { l = l.replace(&format!("${{{k}}}"), v); }
        lines.push(words(&l));
    }
    fn unroll(lines: &[Vec<String>], i: &mut usize, out: &mut Vec<Vec<String>>) -> Result<(), String> {
        while *i < lines.len() {
            let l = &lines[*i];
            *i += 1;
            match l[0].as_str() {
                "repeat" => {
                    let n: usize = l.get(1).and_then(|v| v.parse().ok()).ok_or("repeat N")?;
                    let start = *i;
                    let mut body = vec![];
                    unroll(lines, i, &mut body)?;
                    for k in 0..n { for b in &body { out.push(b.iter().map(|w| w.replace("${i}", &k.to_string())).collect()); } }
                    let _ = start;
                }
                "end" => return Ok(()),
                _ => out.push(l.clone()),
            }
        }
        Ok(())
    }
    let mut out = vec![];
    let mut i = 0;
    unroll(&lines, &mut i, &mut out)?;
    Ok(out)
}

struct Markers(std::io::BufWriter<std::fs::File>);
impl Markers {
    fn put(&mut self, kind: &str, text: &str) {
        let _ = writeln!(self.0, "{},{kind},{}", sampler::now_ms(), text.replace(['\n', '\r'], " ").replace(',', ";"));
        let _ = self.0.flush();
    }
}

struct App { child: Child, lines: Arc<Mutex<VecDeque<(Instant, String)>>>, pid: u32 }

fn run(args: &[String]) -> Result<(), String> {
    let exe = PathBuf::from(arg(args, "--exe").ok_or("--exe")?);
    let script = PathBuf::from(arg(args, "--script").ok_or("--script")?);
    let out = PathBuf::from(arg(args, "--out").ok_or("--out")?);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let every = Duration::from_millis(arg(args, "--interval-ms").and_then(|v| v.parse().ok()).unwrap_or(250));
    let data = arg(args, "--data").map(PathBuf::from);
    let mut vars: Vec<(String, String)> = arg_all(args, "--var").iter().filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
    vars.push(("OUT".into(), out.to_string_lossy().into_owned()));
    if let Some(d) = &data { vars.push(("DATA".into(), d.to_string_lossy().into_owned())); }
    let env: Vec<(String, String)> = arg_all(args, "--env").iter().filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned()))).collect();
    let path_first = arg(args, "--path-first");
    let steps = parse(&std::fs::read_to_string(&script).map_err(|e| format!("{}: {e}", script.display()))?, &vars)?;
    let mut mk = Markers(std::io::BufWriter::new(std::fs::File::create(out.join("markers.csv")).map_err(|e| e.to_string())?));
    let _ = writeln!(mk.0, "unix_ms,kind,text");
    mk.put("note", &format!("script {} exe {}", script.display(), exe.display()));
    let samp = sampler::Sampler::start(out.join("samples.csv"), every, 0);
    let mut app: Option<App> = None;
    let mut last_action = Instant::now();
    let result = (|| -> Result<(), String> {
        for (n, st) in steps.iter().enumerate() {
            let w = |i: usize| st.get(i).cloned().unwrap_or_default();
            let secs = |i: usize, d: f64| st.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
            let at = format!("step {} ({})", n + 1, st.join(" "));
            match st[0].as_str() {
                "launch" => {
                    let mut c = Command::new(&exe);
                    c.env("HOVER_BENCH", "1").stdin(Stdio::piped()).stdout(Stdio::piped())
                        .stderr(std::fs::File::create(out.join("hover-stderr.log")).map_err(|e| e.to_string())?);
                    if let Some(d) = &data { c.env("HOVER_DATA_DIR", d); }
                    for (k, v) in &env { c.env(k, v); }
                    if let Some(p) = &path_first {
                        let old = std::env::var_os("PATH").unwrap_or_default();
                        let mut all = vec![PathBuf::from(p)];
                        all.extend(std::env::split_paths(&old));
                        c.env("PATH", std::env::join_paths(all).map_err(|e| e.to_string())?);
                    }
                    last_action = Instant::now();
                    let mut child = c.spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
                    let pid = child.id();
                    samp.root.store(pid, std::sync::atomic::Ordering::SeqCst);
                    mk.put("launch", &pid.to_string());
                    let lines: Arc<Mutex<VecDeque<(Instant, String)>>> = Default::default();
                    let (l2, so) = (lines.clone(), child.stdout.take().unwrap());
                    let mut log = std::fs::File::create(out.join("hover-stdout.log")).map_err(|e| e.to_string())?;
                    std::thread::spawn(move || {
                        for line in std::io::BufReader::new(so).lines() {
                            let Ok(line) = line else { break };
                            let _ = writeln!(log, "{} {line}", sampler::now_ms());
                            l2.lock().unwrap().push_back((Instant::now(), line));
                        }
                    });
                    app = Some(App { child, lines, pid });
                }
                "send" => {
                    let a = app.as_mut().ok_or(format!("{at}: not launched"))?;
                    let line = st[1..].join(" ");
                    mk.put("send", &line);
                    last_action = Instant::now();
                    let stdin = a.child.stdin.as_mut().unwrap();
                    writeln!(stdin, "{line}").and_then(|_| stdin.flush()).map_err(|e| format!("{at}: {e}"))?;
                }
                "expect" => {
                    let a = app.as_ref().ok_or(format!("{at}: not launched"))?;
                    let prefix = w(1);
                    let t = Duration::from_secs_f64(secs(2, 10.0));
                    let label = st.iter().position(|x| x == "as").and_then(|i| st.get(i + 1)).cloned();
                    let (when, line) = wait_line(a, &prefix, t).ok_or(format!("{at}: no line starting with {prefix:?} within {t:?}"))?;
                    mk.put("got", &line);
                    if let Some(l) = label { mk.put("time", &format!("{l} {:.1}", (when - last_action).as_secs_f64() * 1000.0)); }
                }
                // The screen as the user sees it: `shot FILE.png [X Y W H]` (default: the
                // top centre, where the notch opens).
                "shot" => {
                    let file = w(1);
                    let pw = input::primary_width();
                    let n = |i: usize, d: i32| st.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
                    let (w, h) = (n(4, 1240), n(5, 560));
                    let (x, y) = (n(2, (pw - w) / 2), n(3, 0));
                    let px = input::capture(x, y, w, h)?;
                    let img = image::RgbaImage::from_raw(w as u32, h as u32, px).ok_or("the shot's size")?;
                    img.save(out.join(&file)).map_err(|e| e.to_string())?;
                    mk.put("note", &format!("shot {file}"));
                }
                "mark" => mk.put("mark", &w(1)),
                // Where the memory is right now (regions.rs), into OUT/FILE.
                "regions" => {
                    let a = app.as_ref().ok_or(format!("{at}: not launched"))?;
                    let text = regions::run(a.pid, 40)?;
                    std::fs::write(out.join(w(1)), text).map_err(|e| e.to_string())?;
                    mk.put("note", &format!("regions {}", w(1)));
                }
                "note" => mk.put("note", &st[1..].join(" ")),
                "sleep" => std::thread::sleep(Duration::from_secs_f64(secs(1, 1.0))),
                "move" | "click" | "wheel" => {
                    let x = if w(1) == "notch" { input::primary_width() / 2 } else { w(1).parse().map_err(|_| format!("{at}: X"))? };
                    let y: i32 = w(2).parse().map_err(|_| format!("{at}: Y"))?;
                    mk.put("input", &st.join(" "));
                    last_action = Instant::now();
                    match st[0].as_str() { "move" => input::move_to(x, y), "click" => input::click(x, y), _ => input::wheel(x, y, w(3).parse().unwrap_or(-1)) }
                }
                "press" => { mk.put("input", &st.join(" ")); last_action = Instant::now(); input::keys(&input::chord(&w(1)))?; }
                "type" => { mk.put("input", "type"); last_action = Instant::now(); input::type_text(&st[1..].join(" ")); }
                "click-name" | "expect-name" | "expect-no-name" => {
                    let a = app.as_ref().ok_or(format!("{at}: not launched"))?;
                    let name = w(1);
                    let t = Duration::from_secs_f64(secs(2, 10.0));
                    let want_gone = st[0] == "expect-no-name";
                    let start = Instant::now();
                    let found = loop {
                        let els = input::elements(a.pid)?;
                        let hit = els.into_iter().find(|e| if let Some(p) = name.strip_suffix('*') { e.name.starts_with(p) } else { e.name == name });
                        if want_gone { if hit.is_none() { break None; } } else if let Some(h) = hit { break Some(h); }
                        if start.elapsed() > t {
                            let names: Vec<String> = input::elements(a.pid).unwrap_or_default().into_iter().map(|e| e.name).collect();
                            std::fs::write(out.join(format!("elements-step{}.txt", n + 1)), names.join("\n")).ok();
                            return Err(format!("{at}: {} within {t:?}", if want_gone { "still there" } else { "not found" }));
                        }
                        std::thread::sleep(Duration::from_millis(150));
                    };
                    mk.put("got", &format!("{} {name}", st[0]));
                    if let (Some(f), "click-name") = (found, st[0].as_str()) {
                        let (x, y) = ((f.rect.0 + f.rect.2) / 2, (f.rect.1 + f.rect.3) / 2);
                        mk.put("input", &format!("click {x} {y} {}", f.name));
                        last_action = Instant::now();
                        input::click(x, y);
                    }
                }
                "quit" | "kill" => {
                    let mut a = app.take().ok_or(format!("{at}: not launched"))?;
                    let t0 = Instant::now();
                    if st[0] == "quit" {
                        mk.put("send", "quit");
                        let stdin = a.child.stdin.as_mut().unwrap();
                        let _ = writeln!(stdin, "quit").and_then(|_| stdin.flush());
                        let t = Duration::from_secs_f64(secs(1, 20.0));
                        loop {
                            if a.child.try_wait().ok().flatten().is_some() { break; }
                            if t0.elapsed() > t { kill_tree(a.pid); return Err(format!("{at}: still running after {t:?}")); }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                        mk.put("time", &format!("exit {:.1}", t0.elapsed().as_secs_f64() * 1000.0));
                    } else {
                        kill_tree(a.pid);
                        let _ = a.child.wait();
                        mk.put("note", "killed");
                    }
                    // Every process the app's tree ever held must be gone (tools die with it).
                    std::thread::sleep(Duration::from_millis(3000));
                    let now = procs::list();
                    let seen = samp.seen.lock().unwrap().clone();
                    // The start time too: Windows hands a freed id straight to a new process (the host's own conhost took one of ours), and that is not one of Hover's left behind.
                    let left: Vec<String> = seen.iter().filter(|(p, name, s)| now.iter().any(|q| q.pid == *p && &q.name == name && (q.started == *s || q.started == 0 || *s == 0))).map(|(p, n, _)| format!("{n}:{p}")).collect();
                    mk.put("orphans", &format!("{} {}", left.len(), left.join(" ")));
                    if !left.is_empty() && st[0] == "quit" { return Err(format!("{at}: left running after exit: {}", left.join(" "))); }
                    samp.seen.lock().unwrap().clear();
                }
                other => return Err(format!("{at}: unknown step {other}")),
            }
        }
        Ok(())
    })();
    if let Some(a) = app.take() { kill_tree(a.pid); }
    drop(samp);
    match &result { Ok(()) => mk.put("end", "ok"), Err(e) => mk.put("end", &format!("failed {e}")) }
    result
}

/// The line waited for, and when it came; lines before it are passed over.
fn wait_line(a: &App, prefix: &str, t: Duration) -> Option<(Instant, String)> {
    let start = Instant::now();
    loop {
        {
            let mut l = a.lines.lock().unwrap();
            if let Some(i) = l.iter().position(|(_, s)| s.starts_with(prefix)) {
                let got = l[i].clone();
                l.drain(..=i);
                return Some(got);
            }
        }
        if start.elapsed() > t { return None; }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn kill_tree(pid: u32) {
    let all = procs::list();
    for (_, p) in procs::tree(&all, pid).into_iter().rev() {
        #[cfg(windows)]
        let _ = Command::new("taskkill").args(["/F", "/PID", &p.pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        #[cfg(unix)]
        unsafe { libc::kill(p.pid as i32, libc::SIGKILL); }
    }
}
