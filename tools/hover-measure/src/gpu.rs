//! GPU memory per process. Windows: the "GPU Process Memory" performance counters
//! (what Task Manager shows), dedicated (video memory) and shared (system memory the
//! GPU maps), summed over the adapters a process uses. Linux: not read (no portable
//! counter; the DRM fdinfo keys differ per driver), so the columns stay empty.

use std::collections::HashMap;

#[derive(Clone, Copy, Default, Debug)]
pub struct Gpu { pub dedicated: u64, pub shared: u64 }

pub struct Counters { imp: imp::Q }

impl Counters {
    pub fn new() -> Option<Counters> { imp::Q::new().map(|imp| Counters { imp }) }
    /// Every process's GPU memory, by pid.
    pub fn read(&mut self) -> HashMap<u32, Gpu> { self.imp.read() }
}

#[cfg(windows)]
mod imp {
    use super::Gpu;
    use std::collections::HashMap;
    use windows::core::PCWSTR;
    use windows::Win32::System::Performance::*;

    pub struct Q { q: PDH_HQUERY, ded: PDH_HCOUNTER, sh: PDH_HCOUNTER }

    fn w(s: &str) -> Vec<u16> { s.encode_utf16().chain([0]).collect() }

    impl Q {
        pub fn new() -> Option<Q> {
            unsafe {
                let mut q = PDH_HQUERY::default();
                if PdhOpenQueryW(PCWSTR::null(), 0, &mut q) != 0 { return None; }
                let (mut ded, mut sh) = (PDH_HCOUNTER::default(), PDH_HCOUNTER::default());
                let a = w("\\GPU Process Memory(*)\\Dedicated Usage");
                let b = w("\\GPU Process Memory(*)\\Shared Usage");
                if PdhAddEnglishCounterW(q, PCWSTR(a.as_ptr()), 0, &mut ded) != 0 { return None; }
                if PdhAddEnglishCounterW(q, PCWSTR(b.as_ptr()), 0, &mut sh) != 0 { return None; }
                Some(Q { q, ded, sh })
            }
        }

        pub fn read(&mut self) -> HashMap<u32, Gpu> {
            let mut out: HashMap<u32, Gpu> = HashMap::new();
            unsafe {
                if PdhCollectQueryData(self.q) != 0 { return out; }
                for (c, dedicated) in [(self.ded, true), (self.sh, false)] {
                    let (mut size, mut n) = (0u32, 0u32);
                    let _ = PdhGetFormattedCounterArrayW(c, PDH_FMT_LARGE, &mut size, &mut n, None);
                    if size == 0 { continue; }
                    let mut buf = vec![0u8; size as usize];
                    let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
                    if PdhGetFormattedCounterArrayW(c, PDH_FMT_LARGE, &mut size, &mut n, Some(items)) != 0 { continue; }
                    for i in 0..n as usize {
                        let it = &*items.add(i);
                        let name = it.szName.to_string().unwrap_or_default();
                        // pid_1234_luid_0x..._phys_0
                        let Some(pid) = name.strip_prefix("pid_").and_then(|r| r.split('_').next()).and_then(|p| p.parse::<u32>().ok()) else { continue };
                        let v = it.FmtValue.Anonymous.largeValue.max(0) as u64;
                        let e = out.entry(pid).or_default();
                        if dedicated { e.dedicated += v } else { e.shared += v }
                    }
                }
            }
            out
        }
    }

    impl Drop for Q {
        fn drop(&mut self) { unsafe { let _ = PdhCloseQuery(self.q); } }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Gpu;
    use std::collections::HashMap;
    pub struct Q;
    impl Q {
        pub fn new() -> Option<Q> { None }
        pub fn read(&mut self) -> HashMap<u32, Gpu> { HashMap::new() }
    }
}
