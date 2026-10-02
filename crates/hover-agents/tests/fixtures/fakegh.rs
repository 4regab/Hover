//! A stand-in for gh (and winget) for the tests, compiled by them with rustc into a
//! folder of its own and told what to say by scripts beside it: one `.exe`/binary on
//! every system, so Windows needs no .cmd. It never reaches a network.
//!
//! A call is logged to `calls.log` (its arguments joined with U+001F, one call a line)
//! and answered by the script `script/<key>.txt`, where the key is the words before the
//! first option joined with "_" ("auth_status", "pr_create"; "--version" is "version").
//! With a third plain word, `script/<key>_<word, letters and digits only>.txt` is tried
//! first (gh pr view <url>). A script is lines:
//!   out=TEXT / err=TEXT   print a line on stdout / stderr
//!   exit=N                the exit code (default 0)
//!   read                  wait for a line on stdin
//!   readall               read stdin to its end into `stdin.<key>.txt`
//!   sleep=MS              wait
//!   hold                  wait until killed, writing the time to `alive.txt` as it does
//!   flood=N               print N long lines on stdout
//!   write=FILE|TEXT       write TEXT (\n for a newline) to FILE in this folder
//!   copy=FROM|TO          copy a file of this folder to another

use std::io::{BufRead, Read, Write};

fn main() {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().to_path_buf();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("calls.log")).unwrap();
    writeln!(log, "{}", args.join("\u{1f}")).unwrap();
    drop(log);

    let key = if args.first().map(String::as_str) == Some("--version") {
        "version".to_owned()
    } else {
        args.iter().take_while(|a| !a.starts_with('-')).take(2).cloned().collect::<Vec<_>>().join("_")
    };
    let third = args.iter().take_while(|a| !a.starts_with('-')).nth(2).map(|a| a.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>());
    let mut candidates = vec![];
    if let Some(t) = third { candidates.push(format!("{key}_{t}")); }
    candidates.push(key.clone());
    let Some(script) = candidates.iter().find_map(|c| std::fs::read_to_string(dir.join("script").join(format!("{c}.txt"))).ok()) else {
        eprintln!("fake gh: no script for {key}");
        std::process::exit(1);
    };

    let stdin = std::io::stdin();
    let mut code = 0;
    for line in script.lines() {
        let (word, rest) = line.split_once('=').unwrap_or((line, ""));
        match word.trim() {
            "out" => { println!("{rest}"); std::io::stdout().flush().unwrap(); }
            "err" => { eprintln!("{rest}"); }
            "exit" => code = rest.trim().parse().unwrap(),
            "read" => { let mut l = String::new(); let _ = stdin.lock().read_line(&mut l); }
            "readall" => {
                let mut all = vec![];
                let _ = stdin.lock().read_to_end(&mut all);
                std::fs::write(dir.join(format!("stdin.{key}.txt")), all).unwrap();
            }
            "sleep" => std::thread::sleep(std::time::Duration::from_millis(rest.trim().parse().unwrap())),
            "hold" => loop {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis();
                let _ = std::fs::write(dir.join("alive.txt"), now.to_string());
                std::thread::sleep(std::time::Duration::from_millis(50));
            },
            "flood" => {
                let out = std::io::stdout();
                let mut out = out.lock();
                for i in 0..rest.trim().parse::<u32>().unwrap() {
                    if writeln!(out, "{i:08} {}", "x".repeat(100)).is_err() { std::process::exit(3); }
                }
            }
            "write" => { let (f, t) = rest.split_once('|').unwrap(); std::fs::write(dir.join(f), t.replace("\\n", "\n")).unwrap(); }
            "copy" => { let (f, t) = rest.split_once('|').unwrap(); std::fs::copy(dir.join(f), dir.join(t)).unwrap(); }
            "" => {}
            other => panic!("fake gh: unknown directive {other}"),
        }
    }
    std::process::exit(code);
}
