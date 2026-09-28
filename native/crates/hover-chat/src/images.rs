//! Images for the thread, shared by the layout (which needs their sizes) and the painter
//! (which needs their pixels). Bytes come from a loader that may answer "not yet": a web
//! image is fetched in the background, and the thread lays its section out again when
//! it arrives (`Thread::image_changed`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use resvg::tiny_skia::{self, Pixmap};

/// What a loader has for a URL.
pub enum Fetch {
    Bytes(Vec<u8>),
    /// Asked for, not here yet.
    Pending,
    Failed,
}

pub type Loader = Box<dyn Fn(&str) -> Fetch>;

/// An image as the layout sees it. A pending and a failed image look the same in the
/// page (Chromium shows the broken-image icon and the alt text for both).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImageState {
    Ready(f32, f32),
    Pending,
    Broken,
}

enum Slot {
    Ready(Pixmap),
    Pending,
    Failed,
}

pub struct Images {
    loader: Loader,
    map: HashMap<String, Slot>,
}

/// The cache, shared between a thread and its painter.
pub type Shared = Rc<RefCell<Images>>;

/// A decoded image's longest side at most (a larger one is scaled down once, on load):
/// the chat never draws one bigger than the drawer, even at 400 %.
const MAX_SIDE: u32 = 4096;

impl Images {
    pub fn new(loader: Loader) -> Shared {
        Rc::new(RefCell::new(Images { loader, map: HashMap::new() }))
    }

    /// Nothing loads (tests, and the headless screenshots unless told otherwise).
    pub fn none() -> Shared {
        Self::new(Box::new(|_| Fetch::Failed))
    }

    fn slot(&mut self, src: &str) -> &Slot {
        // A pending image is asked for again until the loader has it.
        if !matches!(self.map.get(src), Some(Slot::Ready(_) | Slot::Failed)) {
            let s = match (self.loader)(src) {
                Fetch::Bytes(b) => decode(&b).map_or(Slot::Failed, Slot::Ready),
                Fetch::Pending => Slot::Pending,
                Fetch::Failed => Slot::Failed,
            };
            self.map.insert(src.to_string(), s);
        }
        &self.map[src]
    }

    pub fn state(&mut self, src: &str) -> ImageState {
        match self.slot(src) {
            Slot::Ready(p) => ImageState::Ready(p.width() as f32, p.height() as f32),
            Slot::Pending => ImageState::Pending,
            Slot::Failed => ImageState::Broken,
        }
    }

    pub fn pixmap(&mut self, src: &str) -> Option<&Pixmap> {
        match self.slot(src) { Slot::Ready(p) => Some(p), _ => None }
    }

    /// Forgets an image, so it is asked for again (a retry, or a file that changed).
    pub fn forget(&mut self, src: &str) {
        self.map.remove(src);
    }
}

fn decode(bytes: &[u8]) -> Option<Pixmap> {
    let mut img = image::load_from_memory(bytes).ok()?;
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let rgba = img.to_rgba8();
    let mut p = Pixmap::new(rgba.width(), rgba.height())?;
    for (d, s) in p.pixels_mut().iter_mut().zip(rgba.pixels()) {
        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
    }
    Some(p)
}
