//! One look at every process: who its parent is, and the memory counters the OS keeps
//! for it. The counters mean different things per OS, so each keeps its own name in
//! the CSV header (see docs/development/profiling.md):
//!
//! Windows: private = private commit (PrivateUsage), resident = working set,
//! private_resident = private working set, handles = kernel handles.
//! Linux: private = resident private pages (Private_Clean + Private_Dirty, the USS),
//! resident = RSS, pss = PSS, swap = swapped out, handles = open file descriptors.

#[derive(Clone, Debug, Default)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub threads: u32,
    pub private: u64,
    pub resident: u64,
    pub private_resident: u64,
    pub pss: u64,
    pub swap: u64,
    pub handles: u64,
    /// When it started (Windows: FILETIME ticks; Linux: clock ticks after boot), so a
    /// process whose parent id was reused isn't taken for a child.
    pub started: u64,
}

/// Every process, with only pid, parent and name filled in.
pub fn list() -> Vec<Proc> { imp::list() }

/// The counters for one process (none when it has gone or can't be read).
pub fn counters(p: &mut Proc) -> bool { imp::counters(p) }

/// The process and everything it started, depth first, the root at depth 0.
pub fn tree(all: &[Proc], root: u32) -> Vec<(usize, Proc)> {
    let mut out = vec![];
    let Some(r) = all.iter().find(|p| p.pid == root) else { return out };
    let mut stack = vec![(0usize, r.clone())];
    while let Some((d, p)) = stack.pop() {
        for c in all.iter().filter(|c| c.ppid == p.pid && c.pid != p.pid && (c.started == 0 || p.started == 0 || c.started >= p.started)) { stack.push((d + 1, c.clone())); }
        out.push((d, p));
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::Proc;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2};
    use windows::Win32::System::Threading::{GetProcessHandleCount, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    pub fn list() -> Vec<Proc> {
        let mut out = vec![];
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut e).is_ok();
            while ok {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                let mut p = Proc { pid: e.th32ProcessID, ppid: e.th32ParentProcessID, name: String::from_utf16_lossy(&e.szExeFile[..n]), threads: e.cntThreads, ..Default::default() };
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, p.pid) {
                    let (mut c, mut x, mut k, mut u) = Default::default();
                    if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok() { p.started = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64; }
                    let _ = CloseHandle(h);
                }
                out.push(p);
                ok = Process32NextW(snap, &mut e).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    pub fn counters(p: &mut Proc) -> bool {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, p.pid) else { return false };
            let mut c = PROCESS_MEMORY_COUNTERS_EX2 { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32, ..Default::default() };
            let ok = GetProcessMemoryInfo(h, &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb).is_ok();
            let mut handles = 0u32;
            let _ = GetProcessHandleCount(h, &mut handles);
            let _ = CloseHandle(h);
            if !ok { return false; }
            p.private = c.PrivateUsage as u64;
            p.resident = c.WorkingSetSize as u64;
            p.private_resident = c.PrivateWorkingSetSize as u64;
            p.handles = handles as u64;
            true
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::Proc;

    pub fn list() -> Vec<Proc> {
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir("/proc") else { return out };
        for e in rd.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // pid (comm) state ppid ... ; comm may hold spaces and parentheses.
            let (Some(a), Some(b)) = (stat.find('('), stat.rfind(')')) else { continue };
            let name = stat[a + 1..b].to_owned();
            let rest: Vec<&str> = stat[b + 1..].split_whitespace().collect();
            let ppid = rest.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            let threads = rest.get(17).and_then(|v| v.parse().ok()).unwrap_or(0);
            let started = rest.get(19).and_then(|v| v.parse().ok()).unwrap_or(0);
            out.push(Proc { pid, ppid, name, threads, started, ..Default::default() });
        }
        out
    }

    pub fn counters(p: &mut Proc) -> bool {
        let Ok(s) = std::fs::read_to_string(format!("/proc/{}/smaps_rollup", p.pid)) else { return false };
        let kb = |key: &str| s.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
        p.resident = kb("Rss:");
        p.pss = kb("Pss:");
        p.private = kb("Private_Clean:") + kb("Private_Dirty:");
        p.private_resident = p.private;
        p.swap = kb("Swap:");
        p.handles = std::fs::read_dir(format!("/proc/{}/fd", p.pid)).map(|d| d.count() as u64).unwrap_or(0);
        true
    }
}

/// The memory counters are read from /proc or the Windows APIs; a Mac has neither here, so the
/// tool compiles there (CI checks the whole workspace) and sees no processes.
#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::Proc;
    pub fn list() -> Vec<Proc> { vec![] }
    pub fn counters(_: &mut Proc) -> bool { false }
}
