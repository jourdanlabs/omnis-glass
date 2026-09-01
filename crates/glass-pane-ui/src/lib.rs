//! Embedded Glass Pane static UI.

use include_dir::{include_dir, Dir};

pub static UI: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ui");

pub fn index_html() -> Option<&'static [u8]> {
    UI.get_file("index.html").map(|f| f.contents())
}
