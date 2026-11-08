use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{AppError, Result};
use crate::model::{ChannelRef, Video};
use crate::util::text::{collapse_whitespace, sanitize_text};
use crate::util::time::parse_rfc3339;

#[derive(Default)]
struct EntryBuilder {
    id: String,
    title: String,
    link: String,
    published: String,
    author: String,
    channel_id: String,
    thumbnail: String,
    views: Option<u64>,
    description: String,
}

#[derive(Clone, Copy)]
enum Capture {
    Id,
    Title,
    Published,
    Author,
    ChannelId,
    Description,
}

/// Parse a YouTube channel Atom feed.
pub fn parse_channel_feed(xml: &str) -> Result<Vec<Video>> {
    let mut reader = Reader::from_str(xml);

    let mut entries: Vec<Video> = Vec::new();
    let mut current: Option<EntryBuilder> = None;
    let mut capture: Option<Capture> = None;
    let mut text = String::new();

    loop {
        match reader.read_event() {
            Err(err) => {
                return Err(AppError::Tool(format!("invalid YouTube RSS: {err}")));
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local = local_name(&e);
                if local == "entry" {
                    current = Some(EntryBuilder::default());
                    capture = None;
                } else if let Some(entry) = current.as_mut() {
                    start_element(&local, &e, entry, &mut capture, &mut text);
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(&e);
                if let Some(entry) = current.as_mut() {
                    empty_element(&local, &e, entry);
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(cap) = capture {
                    let raw: &str = &t;
                    let decoded = quick_xml::escape::unescape(raw)
                        .map(|c| c.into_owned())
                        .unwrap_or_else(|_| raw.to_string());
                    append_capture(cap, &decoded, current.as_mut(), &mut text);
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(cap) = capture {
                    let raw: &str = &t;
                    append_capture(cap, raw, current.as_mut(), &mut text);
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(cap) = capture {
                    let value = if let Ok(Some(ch)) = reference.resolve_char_ref() {
                        ch.to_string()
                    } else {
                        match &*reference {
                            "amp" => "&".to_string(),
                            "lt" => "<".to_string(),
                            "gt" => ">".to_string(),
                            "quot" => "\"".to_string(),
                            "apos" => "'".to_string(),
                            _ => String::new(),
                        }
                    };
                    if !value.is_empty() {
                        append_capture(cap, &value, current.as_mut(), &mut text);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name_end(&e);
                if local == "entry"
                    && let Some(entry) = current.take()
                    && let Some(video) = finalize(entry)
                {
                    entries.push(video);
                }
                capture = None;
            }
            _ => {}
        }
    }

    Ok(entries)
}

fn start_element(
    local: &str,
    e: &BytesStart<'_>,
    entry: &mut EntryBuilder,
    capture: &mut Option<Capture>,
    text: &mut String,
) {
    match local {
        "videoId" => set_capture(capture, text, Capture::Id),
        "title" if entry.title.is_empty() => set_capture(capture, text, Capture::Title),
        "published" => set_capture(capture, text, Capture::Published),
        "name" if entry.author.is_empty() => set_capture(capture, text, Capture::Author),
        "channelId" => set_capture(capture, text, Capture::ChannelId),
        "description" if entry.description.is_empty() => {
            set_capture(capture, text, Capture::Description)
        }
        _ => empty_element(local, e, entry),
    }
}

fn set_capture(capture: &mut Option<Capture>, text: &mut String, value: Capture) {
    *capture = Some(value);
    text.clear();
}

fn append_capture(
    cap: Capture,
    value: &str,
    current: Option<&mut EntryBuilder>,
    text: &mut String,
) {
    let Some(entry) = current else {
        return;
    };
    match cap {
        Capture::Id => {
            text.push_str(value);
            entry.id = text.clone();
        }
        Capture::Title => {
            text.push_str(value);
            entry.title = text.clone();
        }
        Capture::Published => {
            text.push_str(value);
            entry.published = text.clone();
        }
        Capture::Author => {
            text.push_str(value);
            entry.author = text.clone();
        }
        Capture::ChannelId => {
            text.push_str(value);
            entry.channel_id = text.clone();
        }
        Capture::Description => {
            text.push_str(value);
            entry.description = text.clone();
        }
    }
}

fn empty_element(local: &str, e: &BytesStart<'_>, entry: &mut EntryBuilder) {
    match local {
        "thumbnail" => {
            if entry.thumbnail.is_empty()
                && let Some(url) = attribute(e, "url")
            {
                entry.thumbnail = url;
            }
        }
        "statistics" => {
            if entry.views.is_none() {
                entry.views = attribute(e, "views").and_then(|v| v.parse().ok());
            }
        }
        "link" => {
            let rel = attribute(e, "rel");
            if rel.as_deref() != Some("self")
                && rel.as_deref() != Some("next")
                && let Some(href) = attribute(e, "href")
            {
                entry.link = href;
            }
        }
        _ => {}
    }
}

fn attribute(e: &BytesStart<'_>, key: &str) -> Option<String> {
    for attr in e.attributes().with_checks(false).flatten() {
        if attr.key.as_ref() == key {
            return attr
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()
                .map(|value| value.into_owned());
        }
    }
    None
}

fn local_name(e: &BytesStart<'_>) -> String {
    e.local_name().as_ref().to_string()
}

fn local_name_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    e.local_name().as_ref().to_string()
}

fn finalize(entry: EntryBuilder) -> Option<Video> {
    let id = entry.id.trim().to_string();
    if id.len() != 11 {
        return None;
    }
    let title = collapse_whitespace(&sanitize_text(&entry.title));
    let title = if title.is_empty() {
        "(untitled)".to_string()
    } else {
        title
    };
    let channel_name = collapse_whitespace(&sanitize_text(&entry.author));
    let web_url = if entry.link.is_empty() {
        format!("https://www.youtube.com/watch?v={id}")
    } else {
        entry.link
    };
    let thumbnail_url = if entry.thumbnail.is_empty() {
        format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg")
    } else {
        entry.thumbnail
    };
    let published_at = parse_rfc3339(entry.published.trim());
    Some(Video {
        id,
        title,
        channel: ChannelRef {
            id: if entry.channel_id.is_empty() {
                None
            } else {
                Some(entry.channel_id)
            },
            name: channel_name,
            handle: None,
        },
        // RSS does not expose duration.
        duration_secs: None,
        view_count: entry.views,
        published_at,
        thumbnail_url: Some(thumbnail_url),
        web_url,
        live: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015"
      xmlns:media="http://search.yahoo.com/mrss/"
      xmlns="http://www.w3.org/2005/Atom">
  <title>Test Channel</title>
  <entry>
    <id>yt:video:aaaaaaaaaaa</id>
    <yt:videoId>aaaaaaaaaaa</yt:videoId>
    <yt:channelId>UCtestchannel</yt:channelId>
    <title>First &amp; Best</title>
    <link rel="alternate" href="https://www.youtube.com/watch?v=aaaaaaaaaaa"/>
    <published>2025-03-04T10:00:00+00:00</published>
    <author><name>Test Channel</name></author>
    <media:group>
      <media:title>First &amp; Best</media:title>
      <media:thumbnail url="https://i.ytimg.com/vi/aaaaaaaaaaa/hqdefault.jpg"/>
      <media:community><media:statistics views="12345"/></media:community>
    </media:group>
  </entry>
  <entry>
    <yt:videoId>bbbbbbbbbbb</yt:videoId>
    <title>Second</title>
    <published>2025-03-01T09:30:00+00:00</published>
    <author><name>Test Channel</name></author>
  </entry>
</feed>"#;

    #[test]
    fn parses_entries() {
        let videos = parse_channel_feed(FEED).unwrap();
        assert_eq!(videos.len(), 2);
        assert_eq!(videos[0].id, "aaaaaaaaaaa");
        assert_eq!(videos[0].title, "First & Best");
        assert_eq!(videos[0].view_count, Some(12345));
        assert_eq!(videos[0].channel.id.as_deref(), Some("UCtestchannel"));
        assert!(videos[0].published_at.is_some());
        assert_eq!(videos[1].duration_secs, None);
    }
}
