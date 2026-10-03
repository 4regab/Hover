//! hover-backend: see the library. The host (the Mac app's Swift) starts this with
//! HOVER_DATA_DIR set and speaks JSON lines on stdin and stdout.

use std::sync::Arc;

fn main() {
    let out = Arc::new(hover_backend::wire::Out::new(std::io::stdout()));
    hover_backend::run(std::io::stdin(), out);
}
