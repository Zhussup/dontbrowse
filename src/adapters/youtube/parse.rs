use serde_json::Value;

use crate::model::{ChannelRef, Video};
use crate::util::text::{collapse_whitespace, sanitize_text};

/// Parse the JSON emitted by `yt-dlp --dump-single-json --flat-playlist`.
///
/// The top-level object may be a single video or a playlist-like wrapper with
/// `entries`. Every field is optional so extractor changes degrade instead of
/// failing the whole feed.
pub fn parse_videos(json: &str) -> serde_json::Result<Vec<Video>> {
    let root: Value = serde_json::from_str(json)?;
    let entries: Vec<&Value> = match root.get("entries").and_then(Value::as_array) {
        Some(items) => items.iter().collect(),
        None => vec![&root],
    };
    Ok(entries.into_iter().filter_map(map_video).collect())
}

pub fn parse_title(json: &str) -> Option<String> {
    let root: Value = serde_json::from_str(json).ok()?;
    root.get("title")
        .and_then(Value::as_str)
        .map(|s| collapse_whitespace(&sanitize_text(s)))
        .filter(|s| !s.is_empty())
}

fn map_video(v: &Value) -> Option<Video> {
    let id = get_str(v, "id")?;
    // Channel and playlist results have longer ids; video ids are 11 chars.
    if id.len() != 11 || id.contains('/') {
        return None;
    }

    let title = get_str(v, "title")
        .map(|s| collapse_whitespace(&sanitize_text(&s)))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(untitled)".to_string());

    let channel_name = get_str(v, "channel")
        .or_else(|| get_str(v, "uploader"))
        .map(|s| collapse_whitespace(&sanitize_text(&s)))
        .unwrap_or_default();

    let channel_id = get_str(v, "channel_id")
        .or_else(|| get_str(v, "channel_url").and_then(|url| extract_channel_id(&url)));

    let handle = get_str(v, "uploader_id")
        .filter(|s| s.starts_with('@'))
        .or_else(|| {
            get_str(v, "channel_url")
                .or_else(|| get_str(v, "uploader_url"))
                .and_then(|url| extract_handle(&url))
        });

    let duration_secs = get_f64(v, "duration").map(|d| d.max(0.0).round() as u32);

    let view_count = get_u64(v, "view_count");

    let published_at = get_i64(v, "timestamp")
        .or_else(|| get_i64(v, "release_timestamp"))
        .or_else(|| get_str(v, "upload_date").and_then(|d| parse_upload_date(&d)));

    let thumbnail_url =
        best_thumbnail(v).or_else(|| Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")));

    let web_url = get_str(v, "webpage_url")
        .or_else(|| get_str(v, "url").filter(|u| u.starts_with("http")))
        .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={id}"));

    let live = get_str(v, "live_status").as_deref() == Some("is_live")
        || v.get("is_live").and_then(Value::as_bool).unwrap_or(false);

    Some(Video {
        id,
        title,
        channel: ChannelRef {
            id: channel_id,
            name: channel_name,
            handle,
        },
        duration_secs,
        view_count,
        published_at,
        thumbnail_url,
        web_url,
        live,
    })
}

fn best_thumbnail(v: &Value) -> Option<String> {
    let thumbs = v.get("thumbnails")?.as_array()?;
    let mut best: Option<(u32, String)> = None;
    for t in thumbs {
        let Some(url) = t.get("url").and_then(Value::as_str) else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let w = t.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
        if w > 1280 {
            continue;
        }
        if best.as_ref().is_none_or(|(bw, _)| w >= *bw) {
            best = Some((w, url.to_string()));
        }
    }
    if best.is_none() {
        // All candidates were larger than 1280: take the smallest one.
        let mut smallest: Option<(u32, String)> = None;
        for t in thumbs {
            let Some(url) = t.get("url").and_then(Value::as_str) else {
                continue;
            };
            let w = t
                .get("width")
                .and_then(Value::as_u64)
                .unwrap_or(u32::MAX as u64) as u32;
            if smallest.as_ref().is_none_or(|(sw, _)| w < *sw) {
                smallest = Some((w, url.to_string()));
            }
        }
        best = smallest;
    }
    best.map(|(_, url)| url)
}

fn extract_channel_id(url: &str) -> Option<String> {
    let rest = url.split("/channel/").nth(1)?;
    let id = rest.split(['/', '?', '#']).next()?;
    if id.starts_with("UC") {
        Some(id.to_string())
    } else {
        None
    }
}

fn extract_handle(url: &str) -> Option<String> {
    let rest = url.split("youtube.com/").nth(1)?;
    let seg = rest.split(['/', '?', '#']).next()?;
    if seg.starts_with('@') {
        Some(seg.to_string())
    } else {
        None
    }
}

fn parse_upload_date(s: &str) -> Option<i64> {
    if s.len() != 8 || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year = s[0..4].parse::<i32>().ok()?;
    let month = s[4..6].parse::<u8>().ok()?;
    let day = s[6..8].parse::<u8>().ok()?;
    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    Some(date.midnight().assume_utc().unix_timestamp())
}

fn get_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn get_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key)
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f.max(0.0) as u64)))
}

fn get_f64(v: &Value, key: &str) -> Option<f64> {
    v.get(key)
        .and_then(|x| x.as_f64().or_else(|| x.as_i64().map(|i| i as f64)))
}

fn get_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_search() {
        let json = r#"{
          "id": "abc",
          "title": "search",
          "entries": [
            {"id": "dQw4w9WgXcQ", "title": "A video", "channel": "Chan",
             "channel_id": "UC123", "duration": 212.4, "view_count": 1200000,
             "timestamp": 1700000000,
             "thumbnails": [{"url":"https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg","width":320},
                            {"url":"https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg","width":1280}],
             "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"},
            {"id": "UC123", "title": "A channel", "url": "https://www.youtube.com/channel/UC123"}
          ]
        }"#;
        let vids = parse_videos(json).unwrap();
        assert_eq!(vids.len(), 1);
        assert_eq!(vids[0].id, "dQw4w9WgXcQ");
        assert_eq!(vids[0].duration_secs, Some(212));
        assert_eq!(vids[0].view_count, Some(1_200_000));
        assert_eq!(vids[0].channel.id.as_deref(), Some("UC123"));
        assert!(vids[0].thumbnail_url.as_deref().unwrap().contains("maxres"));
    }

    #[test]
    fn missing_fields_are_tolerated() {
        let json = r#"{"id":"dQw4w9WgXcQ","title":"x"}"#;
        let vids = parse_videos(json).unwrap();
        assert_eq!(vids.len(), 1);
        assert_eq!(vids[0].duration_secs, None);
        assert!(vids[0].thumbnail_url.is_some());
    }
}
