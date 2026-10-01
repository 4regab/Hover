//! `regions --pid N`: where a process's memory is, by kind (a VMMap-style breakdown,
//! Windows only). Every committed region, grouped by the reservation it belongs to,
//! with how much of it is resident and resident-private (QueryWorkingSetEx per page).
//! Kinds: image (a DLL or the exe: its code and its private data), mapped (a section:
//! a file, or pagefile-backed shared memory), and private, which is split into
//! write-combined or uncached (memory the GPU driver maps for the CPU), heaps the
//! system knows (Heap32List), and the rest (Rust's allocator, stacks, the drivers' own).

#[cfg(windows)]
pub fn run(pid: u32, top: usize) -> Result<String, String> {
    use std::collections::HashMap;
    use std::fmt::Write;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Heap32ListFirst, Heap32ListNext, HEAPLIST32, TH32CS_SNAPHEAPLIST};
    use windows::Win32::System::Memory::*;
    use windows::Win32::System::ProcessStatus::{GetMappedFileNameW, QueryWorkingSetEx, PSAPI_WORKING_SET_EX_INFORMATION};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

    const PAGE: usize = 4096;
    let h = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }.map_err(|e| e.to_string())?;
    // The heaps the process's heap manager knows (their first segment's base).
    let mut heaps: Vec<usize> = vec![];
    unsafe {
        if let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPHEAPLIST, pid) {
            let mut l = HEAPLIST32 { dwSize: std::mem::size_of::<HEAPLIST32>(), ..Default::default() };
            let mut ok = Heap32ListFirst(snap, &mut l).is_ok();
            while ok { heaps.push(l.th32HeapID); ok = Heap32ListNext(snap, &mut l).is_ok(); }
            let _ = CloseHandle(snap);
        }
    }
    #[derive(Default, Clone)]
    struct Res { kind: String, name: String, reserved: usize, commit: usize, ws: usize, pws: usize }
    let mut by_base: HashMap<usize, Res> = HashMap::new();
    let mut addr = 0usize;
    loop {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let n = unsafe { VirtualQueryEx(h, Some(addr as *const _), &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if n == 0 { break; }
        let base = mbi.AllocationBase as usize;
        let size = mbi.RegionSize;
        let e = by_base.entry(base).or_default();
        e.reserved += if mbi.State != MEM_FREE { size } else { 0 };
        if mbi.State == MEM_COMMIT {
            e.commit += size;
            let prot = mbi.Protect.0;
            let wc = prot & (PAGE_WRITECOMBINE.0 | PAGE_NOCACHE.0) != 0;
            if e.kind.is_empty() || (wc && e.kind == "private") {
                e.kind = match mbi.Type {
                    MEM_IMAGE => "image".into(),
                    MEM_MAPPED => "mapped".into(),
                    _ if wc => "private write-combined".into(),
                    _ if heaps.contains(&base) => "private heap".into(),
                    _ => "private".into(),
                };
                if mbi.Type == MEM_IMAGE || mbi.Type == MEM_MAPPED {
                    let mut buf = [0u16; 520];
                    let len = unsafe { GetMappedFileNameW(h, mbi.BaseAddress, &mut buf) } as usize;
                    let full = String::from_utf16_lossy(&buf[..len]);
                    e.name = full.rsplit('\\').next().unwrap_or("").to_owned();
                }
            }
            // Resident and private-resident pages, 4096 at a time.
            let pages = size / PAGE;
            let mut done = 0;
            while done < pages {
                let k = (pages - done).min(4096);
                let mut info: Vec<PSAPI_WORKING_SET_EX_INFORMATION> = (0..k).map(|i| PSAPI_WORKING_SET_EX_INFORMATION { VirtualAddress: (mbi.BaseAddress as usize + (done + i) * PAGE) as *mut _, ..Default::default() }).collect();
                if unsafe { QueryWorkingSetEx(h, info.as_mut_ptr() as *mut _, (k * std::mem::size_of::<PSAPI_WORKING_SET_EX_INFORMATION>()) as u32) }.is_ok() {
                    for i in &info {
                        let f = unsafe { i.VirtualAttributes.Flags };
                        let valid = f & 1 != 0;
                        let shared = (f >> 15) & 1 != 0;
                        if valid { e.ws += PAGE; if !shared { e.pws += PAGE; } }
                    }
                }
                done += k;
            }
        }
        addr = mbi.BaseAddress as usize + size;
        if addr == 0 || addr >= 0x7FFF_FFFF_0000 { break; }
    }
    let _ = unsafe { CloseHandle(h) };
    let mib = |b: usize| b as f64 / 1048576.0;
    let mut kinds: HashMap<String, (usize, usize, usize, usize)> = HashMap::new();
    for r in by_base.values().filter(|r| r.commit > 0) {
        let k = kinds.entry(r.kind.clone()).or_default();
        k.0 += r.commit; k.1 += r.ws; k.2 += r.pws; k.3 += 1;
    }
    let mut o = String::new();
    let _ = writeln!(o, "pid {pid}: committed memory by kind (MiB): commit, resident, private resident, reservations");
    let mut kv: Vec<_> = kinds.into_iter().collect();
    kv.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    for (k, v) in &kv { let _ = writeln!(o, "{k:<24} {:>8.1} {:>8.1} {:>8.1} {:>6}", mib(v.0), mib(v.1), mib(v.2), v.3); }
    let _ = writeln!(o, "\nlargest reservations (MiB): base, kind, name, reserved, commit, resident, private resident");
    let mut rv: Vec<_> = by_base.into_iter().filter(|(_, r)| r.commit > 0).collect();
    rv.sort_by(|a, b| b.1.commit.cmp(&a.1.commit));
    for (b, r) in rv.iter().take(top) {
        let _ = writeln!(o, "{b:#014x} {:<24} {:<28} {:>8.1} {:>8.1} {:>8.1} {:>8.1}", r.kind, r.name, mib(r.reserved), mib(r.commit), mib(r.ws), mib(r.pws));
    }
    // Private commit by image: what each DLL's own writable data holds.
    Ok(o)
}

#[cfg(not(windows))]
pub fn run(pid: u32, _top: usize) -> Result<String, String> {
    // /proc/PID/smaps: each mapping's Rss, Pss and Private_*, by what it maps.
    let s = std::fs::read_to_string(format!("/proc/{pid}/smaps")).map_err(|e| e.to_string())?;
    let mut by: std::collections::BTreeMap<String, (u64, u64, u64)> = Default::default();
    let mut cur = String::new();
    for l in s.lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() >= 5 && f[0].contains('-') && !l.ends_with(" kB") {
            cur = f.get(5).map(|p| p.rsplit('/').next().unwrap_or(p).to_owned()).unwrap_or_else(|| "[anon]".into());
            continue;
        }
        let kb = || f.get(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
        let e = by.entry(cur.clone()).or_default();
        match f.first().copied() { Some("Rss:") => e.0 += kb(), Some("Pss:") => e.1 += kb(), Some("Private_Clean:") | Some("Private_Dirty:") => e.2 += kb(), _ => {} }
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1.2.cmp(&a.1.2));
    let mut o = format!("pid {pid}: by mapping (MiB): rss, pss, private\n");
    for (k, (r, p, pr)) in v.iter().take(40) { o.push_str(&format!("{k:<40} {:>8.1} {:>8.1} {:>8.1}\n", *r as f64 / 1048576.0, *p as f64 / 1048576.0, *pr as f64 / 1048576.0)); }
    Ok(o)
}
