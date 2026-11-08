use serde::{Deserialize, Serialize};

use super::video::Video;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelTab {
    Videos,
    Shorts,
    Streams,
}

impl ChannelTab {
    pub fn slug(self) -> &'static str {
        match self {
            ChannelTab::Videos => "videos",
            ChannelTab::Shorts => "shorts",
            ChannelTab::Streams => "streams",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeedKey {
    Trending,
    Recommended,
    Subscriptions,
    Search {
        query: String,
        limit: usize,
    },
    Channel {
        id: String,
        tab: ChannelTab,
        limit: usize,
    },
    Playlist {
        id: String,
        limit: usize,
    },
}

impl FeedKey {
    pub fn kind(&self) -> &'static str {
        match self {
            FeedKey::Trending => "trending",
            FeedKey::Recommended => "recommended",
            FeedKey::Subscriptions => "subscriptions",
            FeedKey::Search { .. } => "search",
            FeedKey::Channel { .. } => "channel",
            FeedKey::Playlist { .. } => "playlist",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeedSource {
    YtDlp,
    Web,
    Rss,
    Cache,
    TrendingFallback,
    Empty,
}

impl FeedSource {
    pub fn label(self) -> &'static str {
        match self {
            FeedSource::YtDlp => "yt-dlp",
            FeedSource::Web => "web",
            FeedSource::Rss => "rss",
            FeedSource::Cache => "cache",
            FeedSource::TrendingFallback => "trending",
            FeedSource::Empty => "empty",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeedPage {
    pub key: FeedKey,
    pub title: String,
    pub items: Vec<Video>,
    pub fetched_at: i64,
    pub source: FeedSource,
    pub stale: bool,
    pub warnings: Vec<String>,
}

impl FeedPage {
    pub fn empty(key: FeedKey, title: impl Into<String>) -> Self {
        Self {
            key,
            title: title.into(),
            items: Vec::new(),
            fetched_at: crate::util::time::now_unix(),
            source: FeedSource::Empty,
            stale: false,
            warnings: Vec::new(),
        }
    }
}

/// Which UI surface asked for a feed; worker results are routed by this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedTarget {
    Home,
    Search,
    Subscriptions,
    Library,
}
