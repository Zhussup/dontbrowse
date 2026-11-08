use std::collections::HashSet;
use std::time::Duration;

use serde_json::Value;
use url::Url;

use crate::error::{AppError, Result};
use crate::model::{ChannelRef, Video};
use crate::util::text::{collapse_whitespace, sanitize_text};
use crate::util::time::now_unix;

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const MAX_PAGE: usize = 8 * 1024 * 1024;

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

pub async fn search(client: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Video>> {
    let mut url = Url::parse("https://www.youtube.com/results")
        .map_err(|e| AppError::Other(e.to_string()))?;
    url.query_pairs_mut().append_pair("search_query", query);
    let html = fetch(client, url.as_str()).await?;
    Ok(parse_page(&html, limit, None, None))
}

pub async fn discover(client: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Video>> {
    search(client, query, limit).await
}

pub async fn channel(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<(Vec<Video>, String, Option<String>)> {
    let html = fetch(client, url).await?;
    let data = extract_initial_data(&html)
        .ok_or_else(|| AppError::Extraction("YouTube page data not found".to_string()))?;
    let (title, channel_id) = channel_meta(&data);
    let videos = parse_page(&html, limit, title.as_deref(), channel_id.as_deref());
    Ok((
        videos,
        title.unwrap_or_else(|| "Channel".to_string()),
        channel_id,
    ))
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<String> {
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT_LANGUAGE, "en-US,en;q=0.9")
        .header(
            reqwest::header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "YouTube returned HTTP {}",
            response.status().as_u16()
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;
    if bytes.len() > MAX_PAGE {
        return Err(AppError::Tool("YouTube page is too large".to_string()));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn parse_page(
    html: &str,
    limit: usize,
    default_channel: Option<&str>,
    default_channel_id: Option<&str>,
) -> Vec<Video> {
    let Some(data) = extract_initial_data(html) else {
        return Vec::new();
    };
    let mut renderers = Vec::new();
    collect_renderers(&data, &mut renderers);

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for renderer in renderers {
        let video = if renderer.get("videoId").is_some() {
            from_video_renderer(renderer)
        } else if renderer.get("contentId").is_some() {
            from_lockup(renderer, default_channel, default_channel_id)
        } else {
            None
        };
        if let Some(video) = video
            && seen.insert(video.id.clone())
        {
            out.push(video);
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn collect_renderers<'a>(value: &'a Value, out: &mut Vec<&'a Value>) {
    match value {
        Value::Object(map) => {
            if let Some(renderer) = map.get("videoRenderer") {
                out.push(renderer);
            }
            if let Some(renderer) = map.get("gridVideoRenderer") {
                out.push(renderer);
            }
            if let Some(renderer) = map.get("lockupViewModel") {
                out.push(renderer);
            }
            for child in map.values() {
                collect_renderers(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_renderers(child, out);
            }
        }
        _ => {}
    }
}

fn from_video_renderer(value: &Value) -> Option<Video> {
    let id = value.get("videoId")?.as_str()?.to_string();
    if id.len() != 11 {
        return None;
    }
    let title = text_of(value.get("title"))
        .map(|s| collapse_whitespace(&sanitize_text(&s)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(untitled)".to_string());

    let channel_name = text_of(value.get("ownerText"))
        .or_else(|| text_of(value.get("longBylineText")))
        .or_else(|| text_of(value.get("shortBylineText")))
        .map(|s| collapse_whitespace(&sanitize_text(&s)))
        .unwrap_or_default();

    let channel_id = value
        .get("ownerText")
        .or_else(|| value.get("longBylineText"))
        .and_then(|owner| owner.get("runs"))
        .and_then(Value::as_array)
        .and_then(|runs| runs.first())
        .and_then(|run| run.pointer("/navigationEndpoint/browseEndpoint/browseId"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let published_at =
        text_of(value.get("publishedTimeText")).and_then(|text| parse_relative(&text));
    let duration_secs = text_of(value.get("lengthText")).and_then(|text| parse_duration(&text));
    let view_count = text_of(value.get("viewCountText")).and_then(|text| parse_views(&text));
    let thumbnail_url = value
        .pointer("/thumbnail/thumbnails")
        .and_then(best_thumbnail);
    let live = has_live_badge(value)
        || text_of(value.get("lengthText"))
            .map(|text| text.trim().eq_ignore_ascii_case("live"))
            .unwrap_or(false);

    Some(Video {
        web_url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        title,
        channel: ChannelRef {
            id: channel_id,
            name: channel_name,
            handle: None,
        },
        duration_secs,
        view_count,
        published_at,
        thumbnail_url,
        live,
    })
}

fn from_lockup(
    value: &Value,
    default_channel: Option<&str>,
    default_channel_id: Option<&str>,
) -> Option<Video> {
    if let Some(content_type) = value.get("contentType").and_then(Value::as_str)
        && content_type != "LOCKUP_CONTENT_TYPE_VIDEO"
    {
        return None;
    }
    let id = value.get("contentId")?.as_str()?.to_string();
    if id.len() != 11 {
        return None;
    }
    let metadata = value
        .pointer("/metadata/lockupMetadataViewModel")
        .cloned()
        .unwrap_or(Value::Null);

    let title = metadata
        .pointer("/title/content")
        .and_then(Value::as_str)
        .map(|s| collapse_whitespace(&sanitize_text(s)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(untitled)".to_string());

    let mut published_at = None;
    let mut view_count = None;
    if let Some(rows) = metadata
        .pointer("/metadata/contentMetadataViewModel/metadataRows")
        .and_then(Value::as_array)
    {
        for row in rows {
            let Some(parts) = row.get("metadataParts").and_then(Value::as_array) else {
                continue;
            };
            for part in parts {
                let text = part
                    .pointer("/text/content")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let accessibility = part
                    .get("accessibilityLabel")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if accessibility.to_ascii_lowercase().contains("view")
                    || text.to_ascii_lowercase().contains("view")
                {
                    if let Some(views) = parse_views(if accessibility.is_empty() {
                        text
                    } else {
                        accessibility
                    }) {
                        view_count = Some(views);
                    }
                } else if text.contains("ago") {
                    published_at = parse_relative(text);
                }
            }
        }
    }

    let mut duration_secs = None;
    let mut live = false;
    if let Some(overlays) = value
        .pointer("/contentImage/thumbnailViewModel/overlays")
        .and_then(Value::as_array)
    {
        for overlay in overlays {
            if let Some(badges) = overlay
                .pointer("/thumbnailBottomOverlayViewModel/badges")
                .and_then(Value::as_array)
            {
                for badge in badges {
                    if let Some(text) = badge
                        .pointer("/thumbnailBadgeViewModel/text")
                        .and_then(Value::as_str)
                    {
                        if text.trim().eq_ignore_ascii_case("live") {
                            live = true;
                        } else if let Some(seconds) = parse_duration(text) {
                            duration_secs = Some(seconds);
                        }
                    }
                }
            }
        }
    }

    let thumbnail_url = value
        .pointer("/contentImage/thumbnailViewModel/image/sources")
        .and_then(best_thumbnail);

    let channel = default_channel.unwrap_or("").to_string();
    Some(Video {
        web_url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        title,
        channel: ChannelRef {
            id: default_channel_id.map(str::to_string),
            name: collapse_whitespace(&sanitize_text(&channel)),
            handle: None,
        },
        duration_secs,
        view_count,
        published_at,
        thumbnail_url,
        live,
    })
}

fn channel_meta(data: &Value) -> (Option<String>, Option<String>) {
    let renderer = data.pointer("/metadata/channelMetadataRenderer");
    let title = renderer
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .map(|s| collapse_whitespace(&sanitize_text(s)))
        .filter(|s| !s.is_empty());
    let id = renderer
        .and_then(|value| value.get("externalId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (title, id)
}

fn text_of(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("simpleText").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("content").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(runs) = value.get("runs").and_then(Value::as_array) {
        let joined: String = runs
            .iter()
            .filter_map(|run| run.get("text").and_then(Value::as_str))
            .collect();
        if !joined.is_empty() {
            return Some(joined);
        }
    }
    None
}

fn best_thumbnail(thumbnails: &Value) -> Option<String> {
    let array = thumbnails.as_array()?;
    let mut best: Option<(u32, String)> = None;
    for thumbnail in array {
        let Some(url) = thumbnail.get("url").and_then(Value::as_str) else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let width = thumbnail.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
        if width > 1280 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_width, _)| width >= *best_width)
        {
            best = Some((width, url.to_string()));
        }
    }
    if best.is_none() {
        best = array.iter().find_map(|thumbnail| {
            thumbnail
                .get("url")
                .and_then(Value::as_str)
                .map(|url| (0, url.to_string()))
        });
    }
    best.map(|(_, url)| url)
}

fn has_live_badge(value: &Value) -> bool {
    let Some(badges) = value.get("badges").and_then(Value::as_array) else {
        return false;
    };
    badges.iter().any(|badge| {
        badge
            .pointer("/metadataBadgeRenderer/label")
            .and_then(Value::as_str)
            .map(|label| label.eq_ignore_ascii_case("live"))
            .unwrap_or(false)
            || badge
                .pointer("/metadataBadgeRenderer/style")
                .and_then(Value::as_str)
                .map(|style| style.contains("LIVE"))
                .unwrap_or(false)
    })
}

pub fn parse_relative(text: &str) -> Option<i64> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("just now") || lower.trim() == "now" {
        return Some(now_unix());
    }
    let number = first_number(&lower)?;
    let unit_seconds: i64 = if lower.contains("year") || lower.contains("y ago") {
        365 * 86_400
    } else if lower.contains("month") || lower.contains("mo ago") {
        30 * 86_400
    } else if lower.contains("week") || lower.contains("w ago") {
        7 * 86_400
    } else if lower.contains("day") || lower.contains("d ago") {
        86_400
    } else if lower.contains("hour") || lower.contains("h ago") {
        3_600
    } else if lower.contains("minute") || lower.contains("m ago") {
        60
    } else if lower.contains("second") || lower.contains("s ago") {
        1
    } else {
        return None;
    };
    Some(now_unix() - (number * unit_seconds as f64) as i64)
}

fn first_number(text: &str) -> Option<f64> {
    let mut number = String::new();
    let mut started = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            started = true;
        } else if c == '.' && started {
            number.push(c);
        } else if started {
            break;
        }
    }
    number.parse().ok()
}

pub fn parse_duration(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("live") {
        return None;
    }
    let mut total: u32 = 0;
    let mut parts = 0;
    for part in text.split(':') {
        let value: u32 = part.trim().parse().ok()?;
        total = total.checked_mul(60)?.checked_add(value)?;
        parts += 1;
    }
    if parts >= 2 { Some(total) } else { None }
}

pub fn parse_views(text: &str) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("no views") {
        return Some(0);
    }
    let cleaned = text.replace(',', "");
    let mut number = String::new();
    let mut start = None;
    let mut end = cleaned.len();
    for (index, c) in cleaned.char_indices() {
        if c.is_ascii_digit() || (c == '.' && start.is_some()) {
            if start.is_none() {
                start = Some(index);
            }
            number.push(c);
        } else if start.is_some() {
            end = index;
            break;
        }
    }
    let value: f64 = number.parse().ok()?;
    let rest = cleaned[end..].trim_start().to_ascii_lowercase();
    let multiplier = if rest.starts_with('k') || rest.starts_with("thousand") {
        1_000.0
    } else if rest.starts_with('m') || rest.starts_with("million") {
        1_000_000.0
    } else if rest.starts_with('b') || rest.starts_with("billion") {
        1_000_000_000.0
    } else {
        1.0
    };
    Some((value * multiplier).round() as u64)
}

pub fn extract_initial_data(html: &str) -> Option<Value> {
    for marker in [
        "var ytInitialData = ",
        "ytInitialData = ",
        "\"ytInitialData\":",
    ] {
        if let Some(index) = html.find(marker) {
            let after = index + marker.len();
            let start = html[after..].find('{')? + after;
            let end = balanced_json_end(html, start)?;
            if let Ok(value) = serde_json::from_str::<Value>(&html[start..end]) {
                return Some(value);
            }
        }
    }
    None
}

fn balanced_json_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        let index = start + offset;
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_labels() {
        let now = now_unix();
        assert!((parse_relative("3 days ago").unwrap() - (now - 3 * 86_400)).abs() < 5);
        assert!((parse_relative("3w ago").unwrap() - (now - 21 * 86_400)).abs() < 5);
        assert!((parse_relative("5y ago").unwrap() - (now - 5 * 365 * 86_400)).abs() < 5);
    }

    #[test]
    fn parses_counts() {
        assert_eq!(parse_views("2,654,090 views"), Some(2_654_090));
        assert_eq!(parse_views("2.7M views"), Some(2_700_000));
        assert_eq!(parse_views("1.2K views"), Some(1_200));
        assert_eq!(parse_views("No views"), Some(0));
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("2:29"), Some(149));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("LIVE"), None);
    }

    #[test]
    fn parses_search_page() {
        let html = r#"<script>var ytInitialData = {"contents":{"x":[{"videoRenderer":{"videoId":"dQw4w9WgXcQ","title":{"runs":[{"text":"A video"}]},"ownerText":{"runs":[{"text":"Chan","navigationEndpoint":{"browseEndpoint":{"browseId":"UC123"}}}]},"publishedTimeText":{"simpleText":"3 days ago"},"lengthText":{"simpleText":"3:32"},"viewCountText":{"simpleText":"1.2M views"},"thumbnail":{"thumbnails":[{"url":"https://i.ytimg.com/vi/dQw4w9WgXcQ/hq720.jpg","width":720}]}}}]}};</script>"#;
        let videos = parse_page(html, 10, None, None);
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].id, "dQw4w9WgXcQ");
        assert_eq!(videos[0].duration_secs, Some(212));
        assert_eq!(videos[0].view_count, Some(1_200_000));
        assert!(videos[0].published_at.is_some());
        assert_eq!(videos[0].channel.id.as_deref(), Some("UC123"));
    }

    #[test]
    fn parses_lockup_page() {
        let html = r#"<script>var ytInitialData = {"metadata":{"channelMetadataRenderer":{"title":"Chan","externalId":"UC123"}},"contents":[{"lockupViewModel":{"contentId":"dQw4w9WgXcQ","contentType":"LOCKUP_CONTENT_TYPE_VIDEO","contentImage":{"thumbnailViewModel":{"image":{"sources":[{"url":"https://i.ytimg.com/vi/dQw4w9WgXcQ/hq720.jpg","width":720}]},"overlays":[{"thumbnailBottomOverlayViewModel":{"badges":[{"thumbnailBadgeViewModel":{"text":"3:32"}}]}}]}},"metadata":{"lockupMetadataViewModel":{"title":{"content":"A video"},"metadata":{"contentMetadataViewModel":{"metadataRows":[{"metadataParts":[{"text":{"content":"2.7M"},"accessibilityLabel":"2.7 million views"},{"text":{"content":"3w ago"}}]}]}}}}}}]};</script>"#;
        let videos = parse_page(html, 10, Some("Chan"), Some("UC123"));
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].duration_secs, Some(212));
        assert_eq!(videos[0].view_count, Some(2_700_000));
        assert!(videos[0].published_at.is_some());
        assert_eq!(videos[0].channel.name, "Chan");
    }
}
