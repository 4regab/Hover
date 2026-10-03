//! The host's pipe and Program.cs's EventLoop: JSON lines out (one writer, one line at a
//! time), and one thread that runs every command and every callback marshalled onto it.

use hover_core::json::Json;
use std::io::Write;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

/// Backend.Send: a message as one compact line, flushed. Any thread may write.
pub struct Out(Mutex<Box<dyn Write + Send>>);

impl Out {
    pub fn new(w: impl Write + Send + 'static) -> Out { Out(Mutex::new(Box::new(w))) }

    pub fn send(&self, m: &Json) {
        let mut w = self.0.lock().unwrap();
        // The host going away is seen as the end of its input; a failed write says nothing more.
        let _ = writeln!(w, "{}", m.compact());
        let _ = w.flush();
    }

    pub fn toast(&self, text: &str) { self.send(&Json::obj(vec![("type", Json::str("toast")), ("text", Json::str(text))])); }
}

/// What a job on the loop is given: the backend once `initialize` has made it, and
/// whether the loop is to end.
pub struct Host {
    pub backend: Option<crate::backend::Backend>,
    pub done: bool,
}

type Job = Box<dyn FnOnce(&mut Host) + Send>;

/// EventLoop: a queue a single thread runs, so sessions' callbacks and the host's
/// commands never meet.
#[derive(Clone)]
pub struct Loop(Sender<Job>);

impl Loop {
    pub fn new() -> (Loop, Receiver<Job>) {
        let (tx, rx) = channel();
        (Loop(tx), rx)
    }

    /// Post: queued; dropped once the loop has ended.
    pub fn post(&self, f: impl FnOnce(&mut Host) + Send + 'static) { let _ = self.0.send(Box::new(f)); }
}

/// EventLoop.Run: until a job says it is done.
pub fn run_loop(rx: Receiver<Job>) {
    let mut host = Host { backend: None, done: false };
    for job in rx {
        job(&mut host);
        if host.done { break; }
    }
}

/// Backend.Str: a string property of an object message.
pub fn str_of<'a>(m: &'a Json, key: &str) -> Option<&'a str> { m.get(key).and_then(Json::as_str) }

/// An integer property (JsonElement.TryGetInt32).
pub fn int_of(m: &Json, key: &str) -> Option<i32> {
    match m.get(key) { Some(Json::Num(n)) => n.parse::<i32>().ok(), _ => None }
}

/// A boolean property, when it is one.
pub fn bool_of(m: &Json, key: &str) -> Option<bool> {
    match m.get(key) { Some(Json::Bool(b)) => Some(*b), _ => None }
}
