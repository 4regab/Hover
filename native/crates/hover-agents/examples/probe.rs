//! probe: what Hover finds of each tool on this machine, the tool's own status check,
//! and one turn in a folder: `cargo run --release -p hover-agents --example probe -- <folder> [kiro|codex|cursor]`.
use hover_agents::acp::AcpHost;
use hover_agents::agents;
use hover_agents::cancel::Cancel;
use hover_core::model::{AgentOptions, AgentTool};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let folder = a.first().cloned().unwrap_or_else(|| std::env::current_dir().unwrap().to_string_lossy().into_owned());
    for t in AgentTool::ALL {
        println!("{}: exe {:?}, check {:?}", t.name(), agents::exe(t), agents::check(t, true));
    }
    if let Some(t) = AgentTool::parse(a.get(1).map(String::as_str)) {
        let host = AcpHost::new(t, AgentOptions::default);
        host.on_options_seen(|_, o| println!("offers: {}", o.iter().map(|x| format!("{} ({} choices)", x.id, x.choices.len())).collect::<Vec<_>>().join(", ")));
        let t0 = std::time::Instant::now();
        let r = host.run(&folder, "Say hello in one word.", Some(Box::new(|p| println!("phase {p:?}"))), &Cancel::new(), None, None);
        println!("{:?} after {:.1}s: {}", r.state, t0.elapsed().as_secs_f64(), r.text);
        host.shutdown("probe done");
    }
}
