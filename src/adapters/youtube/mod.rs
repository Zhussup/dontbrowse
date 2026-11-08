pub mod parse;
pub mod rss;
pub mod web;
pub mod ytdlp;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::config::{Config, HomeMode};
use crate::db::Subscription;
use crate::error::{AppError, Result};
use crate::model::{ChannelTab, FeedKey, FeedPage, FeedSource, Video};
use crate::util::text::sanitize_text;
use crate::util::time::now_unix;

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub fn short_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

pub fn cache_key(cfg: &Config, key: &FeedKey) -> String {
    let cookie_fp = cfg
        .cookies_from_browser
        .as_deref()
        .map(short_hash)
        .unwrap_or_else(|| "anon".to_string());
    match key {
        FeedKey::Trending => "feed:v1:trending".to_string(),
        FeedKey::Recommended => format!("feed:v1:recommended:{cookie_fp}"),
        FeedKey::Subscriptions => "feed:v1:subscriptions".to_string(),
        FeedKey::Search { query, limit } => {
            format!("feed:v1:search:{}:{limit}", short_hash(query))
        }
        FeedKey::Channel { id, tab, limit } => {
            format!("feed:v1:channel:{}:{}:{limit}", short_hash(id), tab.slug())
        }
        FeedKey::Playlist { id, limit } => {
            format!("feed:v1:playlist:{}:{limit}", short_hash(id))
        }
    }
}

pub async fn load_feed(cfg: &Config, key: &FeedKey) -> Result<FeedPage> {
    match key {
        FeedKey::Subscriptions => Err(AppError::Tool(
            "subscriptions need the channel list from the database".to_string(),
        )),
        FeedKey::Search { query, limit } => {
            let client = web::client();
            if let Ok(items) = web::search(&client, query, *limit).await
                && !items.is_empty()
            {
                return Ok(page(
                    key.clone(),
                    format!("Search: {query}"),
                    items,
                    FeedSource::Web,
                ));
            }
            let target = format!("ytsearch{limit}:{query}");
            let json = ytdlp::run_json(cfg, &target, ytdlp::SEARCH_TIMEOUT).await?;
            let items = parse::parse_videos(&json).map_err(|e| {
                AppError::Tool(format!("could not parse yt-dlp search output: {e}"))
            })?;
            Ok(page(
                key.clone(),
                format!("Search: {query}"),
                items,
                FeedSource::YtDlp,
            ))
        }
        FeedKey::Trending => discover(cfg).await,
        FeedKey::Recommended => {
            load_url(
                cfg,
                key,
                "https://www.youtube.com/feed/recommended",
                "Recommended",
            )
            .await
        }
        FeedKey::Channel { id, tab, limit } => {
            let url = channel_url(id, *tab);
            let client = web::client();
            if let Ok((items, title, _)) = web::channel(&client, &url, *limit).await
                && !items.is_empty()
            {
                return Ok(page(key.clone(), title, items, FeedSource::Web));
            }
            let title = if id.starts_with('@') {
                format!("Channel {id}")
            } else {
                "Channel".to_string()
            };
            load_url(cfg, key, &url, &title).await
        }
        FeedKey::Playlist { id, .. } => {
            let url = format!("https://www.youtube.com/playlist?list={id}");
            load_url(cfg, key, &url, "Playlist").await
        }
    }
}

pub async fn load_home(cfg: &Config) -> Result<FeedPage> {
    let mode = match cfg.youtube.home {
        HomeMode::Trending => HomeMode::Trending,
        HomeMode::Recommended => HomeMode::Recommended,
        HomeMode::Auto => {
            if cfg.cookies_active() {
                HomeMode::Recommended
            } else {
                HomeMode::Trending
            }
        }
    };

    match mode {
        HomeMode::Recommended => match load_feed(cfg, &FeedKey::Recommended).await {
            Ok(mut page) => {
                page.title = "Home".to_string();
                Ok(page)
            }
            Err(err) => {
                tracing::warn!("recommended feed unavailable: {err}");
                let mut page = discover(cfg).await?;
                page.source = FeedSource::TrendingFallback;
                page.warnings
                    .push(format!("recommended feed unavailable: {err}"));
                Ok(page)
            }
        },
        _ => discover(cfg).await,
    }
}

async fn discover(cfg: &Config) -> Result<FeedPage> {
    let client = web::client();
    let items = web::discover(&client, &cfg.youtube.discover_query, cfg.youtube.limit).await?;
    if items.is_empty() {
        return Err(AppError::Tool(
            "YouTube returned no discovery results".to_string(),
        ));
    }
    Ok(page(
        FeedKey::Trending,
        "Discover".to_string(),
        items,
        FeedSource::Web,
    ))
}

async fn load_url(cfg: &Config, key: &FeedKey, url: &str, title: &str) -> Result<FeedPage> {
    let json = ytdlp::run_json(cfg, url, ytdlp::FEED_TIMEOUT).await?;
    let items = parse::parse_videos(&json)
        .map_err(|e| AppError::Tool(format!("could not parse yt-dlp output: {e}")))?;
    let title = parse::parse_title(&json)
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| title.to_string());
    Ok(page(key.clone(), title, items, FeedSource::YtDlp))
}

pub async fn load_subscriptions(_cfg: &Config, subs: &[Subscription]) -> Result<FeedPage> {
    if subs.is_empty() {
        return Ok(FeedPage::empty(FeedKey::Subscriptions, "Subscriptions"));
    }

    let feedable: Vec<&Subscription> = subs.iter().filter(|s| s.id.starts_with("UC")).collect();

    if feedable.is_empty() {
        let mut page = FeedPage::empty(FeedKey::Subscriptions, "Subscriptions");
        page.warnings.push(
            "no subscribed channel has a channel id yet; open a channel and press s to subscribe"
                .to_string(),
        );
        return Ok(page);
    }

    let client = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

    let sem = Arc::new(Semaphore::new(6));
    let mut tasks = JoinSet::new();
    let total = feedable.len();

    for sub in &feedable {
        let client = client.clone();
        let sem = sem.clone();
        let id = sub.id.clone();
        tasks.spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}");
            let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            let body = resp.text().await.map_err(|e| e.to_string())?;
            rss::parse_channel_feed(&body).map_err(|e| e.to_string())
        });
    }

    let mut items: Vec<Video> = Vec::new();
    let mut failed = 0usize;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(mut videos)) => items.append(&mut videos),
            Ok(Err(err)) => {
                tracing::warn!("rss channel failed: {err}");
                failed += 1;
            }
            Err(err) => {
                tracing::warn!("rss task failed: {err}");
                failed += 1;
            }
        }
    }

    let mut seen = HashSet::new();
    items.retain(|v| seen.insert(v.id.clone()));
    items.sort_by(|a, b| {
        b.published_at
            .unwrap_or(0)
            .cmp(&a.published_at.unwrap_or(0))
    });
    items.truncate(100);

    let mut page = page(
        FeedKey::Subscriptions,
        "Subscriptions".to_string(),
        items,
        FeedSource::Rss,
    );
    if failed > 0 {
        page.warnings.push(format!(
            "{failed} of {total} subscriptions failed to refresh"
        ));
    }
    if feedable.len() < subs.len() {
        page.warnings.push(format!(
            "{} subscriptions have no channel id and were skipped",
            subs.len() - feedable.len()
        ));
    }
    Ok(page)
}

fn page(key: FeedKey, title: String, items: Vec<Video>, source: FeedSource) -> FeedPage {
    FeedPage {
        key,
        title,
        items,
        fetched_at: now_unix(),
        source,
        stale: false,
        warnings: Vec::new(),
    }
}

pub fn channel_url(id: &str, tab: ChannelTab) -> String {
    let base = if id.starts_with("http") {
        id.trim_end_matches('/').to_string()
    } else if id.starts_with("UC") {
        format!("https://www.youtube.com/channel/{id}")
    } else if id.starts_with('@') {
        format!("https://www.youtube.com/{id}")
    } else {
        format!("https://www.youtube.com/@{id}")
    };
    let base = if base.ends_with(&format!("/{}", tab.slug())) {
        base
    } else {
        format!("{base}/{}", tab.slug())
    };
    let _ = sanitize_text(&base);
    base
}

/// Minimal video record for URLs typed directly into the omnibox.
pub fn video_stub(url: &str, id: &str) -> Video {
    Video {
        id: id.to_string(),
        title: if id.is_empty() {
            url.to_string()
        } else {
            format!("YouTube video {id}")
        },
        channel: Default::default(),
        duration_secs: None,
        view_count: None,
        published_at: None,
        thumbnail_url: if id.is_empty() {
            None
        } else {
            Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"))
        },
        web_url: url.to_string(),
        live: false,
    }
}

/// Extract a YouTube video id from common URL shapes.
pub fn extract_video_id(url: &url::Url) -> Option<String> {
    match url.host_str()? {
        "youtu.be" => {
            let id = url.path().trim_start_matches('/');
            if id.len() == 11 {
                Some(id.to_string())
            } else {
                None
            }
        }
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
            if url.path() == "/watch" {
                url.query_pairs()
                    .find(|(k, _)| k == "v")
                    .map(|(_, v)| v.to_string())
                    .filter(|v| v.len() == 11)
            } else {
                let parts: Vec<&str> = url.path_segments()?.collect();
                if parts.len() >= 2 && matches!(parts[0], "shorts" | "live" | "embed") {
                    let id = parts[1];
                    if id.len() == 11 {
                        return Some(id.to_string());
                    }
                }
                None
            }
        }
        _ => None,
    }
}

/// Extract a channel id/handle from a channel URL.
pub fn extract_channel_ref(url: &url::Url) -> Option<String> {
    let segments: Vec<&str> = url.path_segments()?.collect();
    if segments.len() >= 2 && segments[0] == "channel" && segments[1].starts_with("UC") {
        return Some(segments[1].to_string());
    }
    segments
        .iter()
        .find(|s| s.starts_with('@'))
        .map(|s| s.to_string())
}

pub fn extract_playlist_id(url: &url::Url) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == "list")
        .map(|(_, v)| v.to_string())
}

pub fn is_playlist_url(url: &url::Url) -> bool {
    extract_playlist_id(url).is_some()
}
