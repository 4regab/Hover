//! The sampler: every interval, the root process and everything under it, one CSV row
//! each, with a wall-clock time the runner's markers share. Nothing is kept in memory
//! but the open file.

use crate::{gpu, procs};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const HEADER: &str = "unix_ms,pid,ppid,depth,name,private,resident,private_resident,pss,swap,handles,threads,gpu_dedicated,gpu_shared";

pub fn now_ms() -> u128 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() }

pub struct Sampler { stop: Arc<AtomicBool>, thread: Option<std::thread::JoinHandle<()>>, pub root: Arc<AtomicU32>, pub seen: Arc<std::sync::Mutex<Vec<(u32, String)>>> }

impl Sampler {
    /// Samples `root` (0 waits for one to be set) into `out` every `every`.
    pub fn start(out: std::path::PathBuf, every: Duration, root: u32) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let root = Arc::new(AtomicU32::new(root));
        let seen: Arc<std::sync::Mutex<Vec<(u32, String)>>> = Default::default();
        let (s, r, sn) = (stop.clone(), root.clone(), seen.clone());
        let thread = std::thread::Builder::new().name("sampler".into()).spawn(move || {
            let mut f = std::io::BufWriter::new(std::fs::File::create(&out).expect("the samples file"));
            let _ = writeln!(f, "{HEADER}");
            let mut g = gpu::Counters::new();
            let mut next = Instant::now();
            while !s.load(Ordering::SeqCst) {
                let pid = r.load(Ordering::SeqCst);
                if pid != 0 {
                    let t = now_ms();
                    let all = procs::list();
                    let tree = procs::tree(&all, pid);
                    let gm = g.as_mut().map(|g| g.read()).unwrap_or_default();
                    for (d, mut p) in tree {
                        if !procs::counters(&mut p) { continue; }
                        {
                            let mut sn = sn.lock().unwrap();
                            if !sn.iter().any(|(q, _)| *q == p.pid) { sn.push((p.pid, p.name.clone())); }
                        }
                        let gp = gm.get(&p.pid).copied();
                        let (gd, gs) = gp.map_or((String::new(), String::new()), |x| (x.dedicated.to_string(), x.shared.to_string()));
                        let _ = writeln!(f, "{t},{},{},{d},{},{},{},{},{},{},{},{},{gd},{gs}", p.pid, p.ppid, p.name.replace(',', "_"), p.private, p.resident, p.private_resident, p.pss, p.swap, p.handles, p.threads);
                    }
                    let _ = f.flush();
                }
                next += every;
                let now = Instant::now();
                if next > now { std::thread::sleep(next - now); } else { next = now; }
            }
        }).unwrap();
        Sampler { stop, thread: Some(thread), root, seen }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}
