//! The samples cut at the markers: each scenario's median, 95th percentile and peak
//! per counter, for Hover alone and for the processes under it, and the timings the
//! runner recorded. Several runs of one script summarise together (their medians'
//! median, the highest peak).

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

const MIB: f64 = 1048576.0;

#[derive(Default, Clone)]
struct Row { t: u128, depth: u32, name: String, v: [f64; 9] }
// v: private, resident, private_resident, pss, swap, handles, threads, gpu_dedicated, gpu_shared
const COLS: [&str; 9] = ["private", "resident", "private_resident", "pss", "swap", "handles", "threads", "gpu_dedicated", "gpu_shared"];

fn stats(mut v: Vec<f64>) -> Option<(f64, f64, f64)> {
    if v.is_empty() { return None; }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    // The median (the middle two's mean for an even count), the 95th percentile by
    // nearest rank, and the peak.
    let med = if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 };
    let p95 = v[((n as f64 * 0.95).ceil() as usize).clamp(1, n) - 1];
    Some((med, p95, v[n - 1]))
}

pub struct Span { pub name: String, pub root: BTreeMap<&'static str, (f64, f64, f64)>, pub kids_private: Option<(f64, f64, f64)>, pub tree_private: Option<(f64, f64, f64)>, pub kids: Vec<String>, pub n: usize }

pub struct Trial { pub dir: PathBuf, pub spans: Vec<Span>, pub times: Vec<(String, f64)>, pub lines: Vec<String>, pub end: String }

fn read_trial(dir: &Path) -> Option<Trial> {
    let samples = std::fs::read_to_string(dir.join("samples.csv")).ok()?;
    let markers = std::fs::read_to_string(dir.join("markers.csv")).ok()?;
    let rows: Vec<Row> = samples.lines().skip(1).filter_map(|l| {
        let f: Vec<&str> = l.split(',').collect();
        if f.len() < 14 { return None; }
        let num = |i: usize| f[i].parse::<f64>().unwrap_or(f64::NAN);
        Some(Row { t: f[0].parse().ok()?, depth: f[3].parse().ok()?, name: f[4].to_owned(), v: std::array::from_fn(|k| num(5 + k)) })
    }).collect();
    let mut marks: Vec<(u128, String)> = vec![];
    let (mut times, mut lines, mut end) = (vec![], vec![], String::new());
    for l in markers.lines().skip(1) {
        let mut f = l.splitn(3, ',');
        let (Some(t), Some(kind), Some(text)) = (f.next().and_then(|t| t.parse::<u128>().ok()), f.next(), f.next()) else { continue };
        match kind {
            "mark" => marks.push((t, text.to_owned())),
            "time" => { if let Some((k, v)) = text.rsplit_once(' ') { if let Ok(v) = v.parse() { times.push((k.to_owned(), v)); } } }
            "got" if text.starts_with("bench frames") || text.starts_with("bench heap") => lines.push(text.to_owned()),
            "orphans" => lines.push(format!("orphans {text}")),
            "end" => { end = text.to_owned(); marks.push((t, "_end".into())); }
            "launch" | "kill" => marks.push((t, "_gap".into())),
            _ => {}
        }
    }
    marks.sort_by_key(|m| m.0);
    let last_t = rows.last().map_or(0, |r| r.t);
    let mut spans = vec![];
    for (i, (t0, name)) in marks.iter().enumerate() {
        if name.starts_with('_') || name == "-" { continue; }
        let t1 = marks.get(i + 1).map_or(last_t + 1, |m| m.0);
        let r: Vec<&Row> = rows.iter().filter(|r| r.t >= *t0 && r.t < t1).collect();
        let root: Vec<&&Row> = r.iter().filter(|r| r.depth == 0).collect();
        let mut rs = BTreeMap::new();
        for (k, c) in COLS.iter().enumerate() {
            let v: Vec<f64> = root.iter().map(|r| r.v[k]).filter(|x| x.is_finite()).collect();
            if let Some(s) = stats(v) { rs.insert(*c, s); }
        }
        // Per sample time: the processes under Hover, summed (private memory doesn't overlap).
        let mut by_t: BTreeMap<u128, (f64, f64)> = BTreeMap::new();
        let mut kids: Vec<String> = vec![];
        for x in &r {
            let e = by_t.entry(x.t).or_default();
            e.1 += x.v[0];
            if x.depth > 0 {
                e.0 += x.v[0];
                if !kids.contains(&x.name) { kids.push(x.name.clone()); }
            }
        }
        spans.push(Span { name: name.clone(), root: rs, kids_private: stats(by_t.values().map(|v| v.0).collect()), tree_private: stats(by_t.values().map(|v| v.1).collect()), kids, n: root.len() });
    }
    Some(Trial { dir: dir.to_owned(), spans, times, lines, end })
}

fn mib(v: Option<&(f64, f64, f64)>) -> String { v.map_or("–".into(), |s| format!("{:.1} / {:.1} / {:.1}", s.0 / MIB, s.1 / MIB, s.2 / MIB)) }

pub fn report(dirs: &[PathBuf]) -> String {
    let trials: Vec<Trial> = dirs.iter().filter_map(|d| read_trial(d)).collect();
    let mut o = String::new();
    let _ = writeln!(o, "Memory in MiB as median / p95 / peak over each scenario's samples. Hover alone: `private` is private commit on Windows and private resident (USS) on Linux; `priv res` is the private working set (Windows) or the same USS (Linux). Tools = every process under Hover, summed.\n");
    for t in &trials {
        let _ = writeln!(o, "## {} ({})\n", t.dir.display(), if t.end.is_empty() { "no end marker" } else { &t.end });
        let _ = writeln!(o, "| scenario | n | private | priv res | resident | GPU dedicated | GPU shared | handles | threads | tools private | tree private | tools |");
        let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|---|---|---|");
        for s in &t.spans {
            let cnt = |k: &str| s.root.get(k).map_or("–".into(), |v| format!("{:.0} / {:.0}", v.0, v.2));
            let _ = writeln!(o, "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", s.name, s.n, mib(s.root.get("private")), mib(s.root.get("private_resident")), mib(s.root.get("resident")),
                mib(s.root.get("gpu_dedicated")), mib(s.root.get("gpu_shared")), cnt("handles"), cnt("threads"), mib(s.kids_private.as_ref()), mib(s.tree_private.as_ref()), s.kids.join(" "));
        }
        let _ = writeln!(o);
        for l in &t.lines { let _ = writeln!(o, "- {l}"); }
        if !t.lines.is_empty() { let _ = writeln!(o); }
    }
    if trials.len() > 1 || trials.iter().any(|t| !t.times.is_empty()) {
        let _ = writeln!(o, "## Across {} run(s)\n", trials.len());
        let _ = writeln!(o, "| scenario | runs | private median (median of runs) | private peak (max) | priv res median | GPU dedicated median | GPU shared median | tools private median | tree private peak |");
        let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|");
        let mut names: Vec<String> = vec![];
        for t in &trials { for s in &t.spans { if !names.contains(&s.name) { names.push(s.name.clone()); } } }
        for n in &names {
            let all: Vec<&Span> = trials.iter().flat_map(|t| t.spans.iter().filter(|s| &s.name == n)).collect();
            let med = |f: &dyn Fn(&Span) -> Option<f64>| stats(all.iter().filter_map(|s| f(s)).collect()).map_or("–".into(), |s| format!("{:.1}", s.0 / MIB));
            let peak = |f: &dyn Fn(&Span) -> Option<f64>| stats(all.iter().filter_map(|s| f(s)).collect()).map_or("–".into(), |s| format!("{:.1}", s.2 / MIB));
            let _ = writeln!(o, "| {n} | {} | {} | {} | {} | {} | {} | {} | {} |", all.len(),
                med(&|s| s.root.get("private").map(|v| v.0)), peak(&|s| s.root.get("private").map(|v| v.2)), med(&|s| s.root.get("private_resident").map(|v| v.0)),
                med(&|s| s.root.get("gpu_dedicated").map(|v| v.0)), med(&|s| s.root.get("gpu_shared").map(|v| v.0)), med(&|s| s.kids_private.map(|v| v.0)), peak(&|s| s.tree_private.map(|v| v.2)));
        }
        let _ = writeln!(o);
        let mut labels: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for t in &trials { for (k, v) in &t.times { labels.entry(k.clone()).or_default().push(*v); } }
        if !labels.is_empty() {
            let _ = writeln!(o, "| timing (ms) | n | median | p95 | max |\n|---|---|---|---|---|");
            for (k, v) in labels { if let Some(s) = stats(v.clone()) { let _ = writeln!(o, "| {k} | {} | {:.1} | {:.1} | {:.1} |", v.len(), s.0, s.1, s.2); } }
            let _ = writeln!(o);
        }
    }
    o
}
