//! hover-data: a data folder written and read by the port, for the C#↔Rust round trips
//! (port/phase3/RUN-ON-WINDOWS.md) and the benchmark's history fixture.
//!   hover-data write <data> <project> <turns> <answer.md>   settings, key, one sealed session; prints its key
//!   hover-data dump <data>                                   what the folder holds, read by the port
//!   hover-data where                                         the data folder Hover would use

use hover_core::crypto::Crypto;
use hover_core::history::{AgentHistory, SavedSession, SavedTurn};
use hover_core::model::{AgentTool, KiroState, KiroStep};
use hover_core::time::Stamp;
use std::path::PathBuf;
use std::sync::Arc;

fn key(data: &std::path::Path) -> Option<Arc<Crypto>> {
    Crypto::load_or_create(&data.join("note.key"), &hover_core::platform::SystemKeyGuard::default()).map(Arc::new)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("write") if a.len() >= 6 => {
            let (data, project) = (PathBuf::from(&a[2]), PathBuf::from(&a[3]));
            let turns: usize = a[4].parse().expect("a number of turns");
            let answer = std::fs::read_to_string(&a[5]).expect("the answer file");
            std::fs::create_dir_all(&data).unwrap();
            std::fs::create_dir_all(&project).unwrap();
            let settings = hover_core::settings::Settings::load(data.join("settings.json"));
            settings.set_kiro_folder(Some(&project.to_string_lossy()));
            settings.flush();
            let crypto = key(&data).expect("a key for the history");
            let h = AgentHistory::new(data.join("agents"), crypto);
            let key = hover_core::guid_n();
            let now = Stamp::now();
            let t = |i: usize| Stamp { ticks: now.ticks - ((turns - i) as i64) * 600_000_000, ..now };
            let s = SavedSession {
                key: key.clone(), tool: AgentTool::Kiro, folder: project.to_string_lossy().into_owned(), title: "A long rich conversation".into(), acp_id: None, context: Some(42.0),
                turns: (0..turns).map(|i| SavedTurn {
                    prompt: format!("Question {} about the rich fixture", i + 1), images: vec![],
                    steps: vec![KiroStep::new(&format!("t{i}"), "read", &format!("Read src/file{i}.rs"), Some(format!("src/file{i}.rs")), "completed")],
                    state: Some(KiroState::Completed), text: Some(answer.clone()), started_at: t(i), woke_at: Some(t(i)), ended_at: Some(t(i).add_secs(30.0)),
                }).collect(),
                updated: now,
                access: None,
            };
            h.save(&s);
            h.flush();
            println!("{key}");
        }
        Some("dump") if a.len() >= 3 => {
            let data = PathBuf::from(&a[2]);
            let Some(crypto) = key(&data) else { println!("no key: history unreadable this run"); return };
            let h = AgentHistory::new(data.join("agents"), crypto);
            let e = h.entries();
            println!("history: {} sessions", e.len());
            for x in e {
                let s = h.load(&x.key);
                let (n, bytes) = s.map_or((0, 0), |s| (s.turns.len(), s.turns.iter().map(|t| t.text.as_ref().map_or(0, String::len)).sum::<usize>()));
                println!("  {}… {} {} {:?} {n} turns, {bytes} answer bytes, updated {}", &x.key[..8.min(x.key.len())], x.tool.name(), x.state.name(), x.title, x.updated.iso());
            }
        }
        Some("where") => println!("{}", hover_core::paths::support().display()),
        _ => { eprintln!("hover-data write <data> <project> <turns> <answer.md> | dump <data> | where"); std::process::exit(2); }
    }
}
