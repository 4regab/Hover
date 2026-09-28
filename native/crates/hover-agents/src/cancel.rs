//! A CancellationToken: set once, with callbacks that run when it is (at once when
//! it already was), each removable while it hasn't run.

use std::sync::{Arc, Mutex};

type Callback = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Inner { cancelled: bool, next: u64, callbacks: Vec<(u64, Callback)> }

#[derive(Clone, Default)]
pub struct Cancel(Arc<Mutex<Inner>>);

/// Removes its callback when dropped (CancellationTokenRegistration.Dispose).
pub struct Registration { token: Cancel, id: u64 }

impl Drop for Registration {
    fn drop(&mut self) { self.token.0.lock().unwrap().callbacks.retain(|(i, _)| *i != self.id); }
}

impl Cancel {
    pub fn new() -> Cancel { Cancel::default() }

    pub fn is_cancelled(&self) -> bool { self.0.lock().unwrap().cancelled }

    pub fn cancel(&self) {
        let callbacks = {
            let mut g = self.0.lock().unwrap();
            if g.cancelled { return; }
            g.cancelled = true;
            std::mem::take(&mut g.callbacks)
        };
        for (_, f) in callbacks { f(); }
    }

    pub fn on_cancel(&self, f: impl FnOnce() + Send + 'static) -> Registration {
        let mut g = self.0.lock().unwrap();
        g.next += 1;
        let id = g.next;
        if g.cancelled {
            drop(g);
            f();
        } else {
            g.callbacks.push((id, Box::new(f)));
        }
        Registration { token: self.clone(), id }
    }
}
