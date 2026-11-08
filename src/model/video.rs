use serde::{Deserialize, Serialize};

use crate::util::time::{format_count, relative_time};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRef {
    pub id: Option<String>,
    pub name: String,
    pub handle: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub channel: ChannelRef,
    pub duration_secs: Option<u32>,
    pub view_count: Option<u64>,
    /// Unix timestamp in seconds.
    pub published_at: Option<i64>,
    pub thumbnail_url: Option<String>,
    pub web_url: String,
    pub live: bool,
}

impl Video {
    pub fn watch_url(&self) -> String {
        if self.web_url.is_empty() {
            format!("https://www.youtube.com/watch?v={}", self.id)
        } else {
            self.web_url.clone()
        }
    }

    pub fn date_label(&self) -> Option<String> {
        self.published_at
            .map(|ts| relative_time(ts, crate::util::time::now_unix()))
    }

    /// "Channel · 3d ago · 1.2M views" with missing parts omitted.
    pub fn meta_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.channel.name.is_empty() {
            parts.push(self.channel.name.clone());
        }
        if let Some(date) = self.date_label() {
            parts.push(date);
        }
        if let Some(views) = self.view_count {
            parts.push(format_count(views));
        }
        parts.join("  ·  ")
    }

    pub fn duration_label(&self) -> Option<String> {
        self.duration_secs.map(crate::util::time::format_duration)
    }
}
