use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::adapters::article::sanitize::is_safe_link;

/// Maps terminal cells to OSC 8 hyperlink targets for the current frame.
#[derive(Clone, Default)]
pub struct LinkRegistry {
    inner: Arc<Mutex<HashMap<(u16, u16), String>>>,
}

impl LinkRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.inner.lock() {
            map.clear();
        }
    }

    /// Register every cell covered by `text` at (`x`, `y`) as linking to `url`.
    pub fn insert_span(&self, x: u16, y: u16, text: &str, url: &str) {
        if !is_safe_link(url) {
            return;
        }
        let Ok(mut map) = self.inner.lock() else {
            return;
        };
        let mut cursor = x;
        for grapheme in text.graphemes(true) {
            let width = UnicodeWidthStr::width(grapheme).max(1) as u16;
            for _ in 0..width {
                map.insert((cursor, y), url.to_string());
                cursor = cursor.saturating_add(1);
            }
        }
    }

    pub fn get(&self, x: u16, y: u16) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|map| map.get(&(x, y)).cloned())
    }
}
