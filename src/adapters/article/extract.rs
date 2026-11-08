use std::collections::HashMap;

use dom_smoothie::{Config as ReadConfig, Readability};
use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use super::blocks::{self, MapResult};
use crate::error::{AppError, Result};
use crate::model::{Article, Block, ExtractSource, Inline, TocEntry};
use crate::util::text::{collapse_whitespace, sanitize_text};
use crate::util::time::{parse_date_loose, parse_rfc3339};

/// Extract a readable article from raw HTML, falling back through several
/// strategies until one produces meaningful content.
pub fn extract(html: &str, requested_url: &str, final_url: &str) -> Result<Article> {
    if let Some(article) = via_readability(html, requested_url, final_url)
        && (article.word_count >= 60 || article.blocks.len() >= 3)
    {
        return Ok(article);
    }
    if let Some(article) = via_heuristic(html, requested_url, final_url)
        && article.word_count >= 30
    {
        return Ok(article);
    }
    if let Some(article) = via_json_ld(html, requested_url, final_url) {
        return Ok(article);
    }
    if let Some(article) = via_plain(html, requested_url, final_url) {
        return Ok(article);
    }
    Err(AppError::Extraction(
        "no readable content found; this page may require JavaScript".to_string(),
    ))
}

fn via_readability(html: &str, requested_url: &str, final_url: &str) -> Option<Article> {
    let config = ReadConfig {
        keep_classes: true,
        max_elements_to_parse: 150_000,
        ..Default::default()
    };
    let mut readability = Readability::new(html, Some(final_url), Some(config)).ok()?;
    let parsed = readability.parse().ok()?;
    let mapped = blocks::map_fragment(parsed.content.as_ref(), final_url);
    if mapped.blocks.is_empty() {
        return None;
    }

    let meta = extract_meta(html);
    let title = clean_opt(&parsed.title)
        .or(meta.title)
        .unwrap_or_else(|| host_title(final_url));
    let byline = parsed.byline.as_deref().and_then(clean_opt).or(meta.byline);
    let site_name = parsed
        .site_name
        .as_deref()
        .and_then(clean_opt)
        .or(meta.site_name);
    let published_at = parsed
        .published_time
        .as_deref()
        .and_then(parse_any_date)
        .or(meta.published_at);
    let lang = parsed.lang.as_deref().and_then(clean_opt).or(meta.lang);

    Some(build_article(
        requested_url,
        final_url,
        title,
        byline,
        site_name,
        published_at,
        lang,
        mapped,
        ExtractSource::Readability,
        Vec::new(),
    ))
}

fn via_heuristic(html: &str, requested_url: &str, final_url: &str) -> Option<Article> {
    let doc = Html::parse_document(html);
    let p_selector = Selector::parse("p").ok()?;
    let a_selector = Selector::parse("a").ok()?;

    let candidates = [
        "article",
        "main",
        "[role=main]",
        ".article-body",
        ".post-content",
        ".entry-content",
        ".article-content",
        "#content",
        "div",
    ];

    let mut best: Option<(f64, ElementRef<'_>)> = None;
    for css in candidates {
        let Ok(selector) = Selector::parse(css) else {
            continue;
        };
        let limit = if css == "div" { 1500 } else { 60 };
        for element in doc.select(&selector).take(limit) {
            let text = element.text().collect::<String>();
            let text_len = text.chars().count();
            if text_len < 200 {
                continue;
            }
            let paragraphs = element.select(&p_selector).count();
            let link_len: usize = element
                .select(&a_selector)
                .map(|a| a.text().collect::<String>().chars().count())
                .sum();
            let link_ratio = link_len as f64 / text_len as f64;
            let score = text_len as f64 + 50.0 * paragraphs as f64 - 20.0 * link_ratio * 100.0;
            if best
                .as_ref()
                .is_none_or(|(best_score, _)| score > *best_score)
            {
                best = Some((score, element));
            }
        }
    }

    let (_, element) = best?;
    let mapped = blocks::map_element(element, final_url);
    if mapped.blocks.is_empty() {
        return None;
    }

    let meta = extract_meta(html);
    let title = meta.title.unwrap_or_else(|| host_title(final_url));
    Some(build_article(
        requested_url,
        final_url,
        title,
        meta.byline,
        meta.site_name,
        meta.published_at,
        meta.lang,
        mapped,
        ExtractSource::Heuristic,
        Vec::new(),
    ))
}

fn via_json_ld(html: &str, requested_url: &str, final_url: &str) -> Option<Article> {
    let doc = Html::parse_document(html);
    let selector = Selector::parse("script[type=\"application/ld+json\"]").ok()?;
    let meta = extract_meta(html);

    for script in doc.select(&selector) {
        let raw = script.text().collect::<String>();
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(found) = find_ld_article(&value) else {
            continue;
        };
        let body = found.body;
        if body.chars().count() < 150 {
            continue;
        }

        let mut blocks = Vec::new();
        let paragraphs: Vec<&str> = if body.contains("\n\n") {
            body.split("\n\n").collect()
        } else {
            body.lines().collect()
        };
        for paragraph in paragraphs {
            let text = collapse_whitespace(&sanitize_text(paragraph));
            if !text.is_empty() {
                blocks.push(Block::Paragraph(vec![Inline::plain(text)]));
            }
        }
        if blocks.is_empty() {
            continue;
        }

        let word_count: usize = blocks.iter().map(Block::word_count).sum();
        let title = found
            .headline
            .as_deref()
            .and_then(clean_opt)
            .or(meta.title)
            .unwrap_or_else(|| host_title(final_url));
        let published_at = found
            .published
            .as_deref()
            .and_then(parse_any_date)
            .or(meta.published_at);

        return Some(Article {
            requested_url: requested_url.to_string(),
            final_url: final_url.to_string(),
            title,
            byline: found.author.as_deref().and_then(clean_opt),
            site_name: found.publisher.as_deref().and_then(clean_opt),
            published_at,
            lang: meta.lang,
            blocks,
            toc: Vec::<TocEntry>::new(),
            anchors: HashMap::new(),
            word_count,
            image_count: 0,
            source: ExtractSource::JsonLd,
            warnings: vec!["extracted from structured data (JSON-LD)".to_string()],
        });
    }
    None
}

fn via_plain(html: &str, requested_url: &str, final_url: &str) -> Option<Article> {
    let doc = Html::parse_document(html);
    let body = Selector::parse("body")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .unwrap_or_else(|| doc.root_element());

    let mut blocks = Vec::new();
    if let Ok(selector) = Selector::parse("p") {
        for paragraph in doc.select(&selector) {
            let text = collapse_whitespace(&sanitize_text(&paragraph.text().collect::<String>()));
            if text.chars().count() > 40 {
                blocks.push(Block::Paragraph(vec![Inline::plain(text)]));
            }
        }
    }

    if blocks.is_empty() {
        let text = collapse_whitespace(&sanitize_text(&body.text().collect::<String>()));
        if text.chars().count() < 150 {
            return None;
        }
        for chunk in chunk_text(&text, 420) {
            blocks.push(Block::Paragraph(vec![Inline::plain(chunk)]));
        }
    }

    if blocks.is_empty() {
        return None;
    }
    let word_count: usize = blocks.iter().map(Block::word_count).sum();
    if word_count < 30 {
        return None;
    }

    let meta = extract_meta(html);
    let title = meta.title.unwrap_or_else(|| host_title(final_url));
    Some(Article {
        requested_url: requested_url.to_string(),
        final_url: final_url.to_string(),
        title,
        byline: meta.byline,
        site_name: meta.site_name,
        published_at: meta.published_at,
        lang: meta.lang,
        blocks,
        toc: Vec::new(),
        anchors: HashMap::new(),
        word_count,
        image_count: 0,
        source: ExtractSource::PlainText,
        warnings: vec!["reader mode approximated from plain text".to_string()],
    })
}

#[allow(clippy::too_many_arguments)]
fn build_article(
    requested_url: &str,
    final_url: &str,
    title: String,
    byline: Option<String>,
    site_name: Option<String>,
    published_at: Option<i64>,
    lang: Option<String>,
    mapped: MapResult,
    source: ExtractSource,
    warnings: Vec<String>,
) -> Article {
    let word_count: usize = mapped.blocks.iter().map(Block::word_count).sum();
    let image_count = mapped
        .blocks
        .iter()
        .filter(|b| matches!(b, Block::Image(_) | Block::Figure { .. }))
        .count();
    let mut all_warnings = warnings;
    all_warnings.extend(mapped.warnings);

    Article {
        requested_url: requested_url.to_string(),
        final_url: final_url.to_string(),
        title,
        byline,
        site_name,
        published_at,
        lang,
        blocks: mapped.blocks,
        toc: mapped.toc,
        anchors: mapped.anchors,
        word_count,
        image_count,
        source,
        warnings: all_warnings,
    }
}

struct Meta {
    title: Option<String>,
    byline: Option<String>,
    site_name: Option<String>,
    published_at: Option<i64>,
    lang: Option<String>,
}

fn extract_meta(html: &str) -> Meta {
    let doc = Html::parse_document(html);
    let mut map: HashMap<String, String> = HashMap::new();
    if let Ok(selector) = Selector::parse("meta") {
        for element in doc.select(&selector) {
            let Some(content) = element.attr("content") else {
                continue;
            };
            if content.is_empty() {
                continue;
            }
            for key in ["property", "name", "itemprop"] {
                if let Some(name) = element.attr(key) {
                    map.entry(name.to_ascii_lowercase())
                        .or_insert_with(|| content.to_string());
                }
            }
        }
    }

    let title = map
        .get("og:title")
        .or_else(|| map.get("twitter:title"))
        .cloned()
        .or_else(|| {
            Selector::parse("title")
                .ok()
                .and_then(|s| doc.select(&s).next())
                .map(|e| collapse_whitespace(&sanitize_text(&e.text().collect::<String>())))
        })
        .and_then(|s| clean_opt(&s));

    let byline = map
        .get("author")
        .or_else(|| map.get("article:author"))
        .or_else(|| map.get("parsely-author"))
        .and_then(|s| clean_opt(s));

    let site_name = map
        .get("og:site_name")
        .or_else(|| map.get("application-name"))
        .and_then(|s| clean_opt(s));

    let published_at = map
        .get("article:published_time")
        .or_else(|| map.get("datepublished"))
        .or_else(|| map.get("date"))
        .or_else(|| map.get("pubdate"))
        .and_then(|s| parse_any_date(s));

    let lang = Selector::parse("html")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .and_then(|e| e.attr("lang"))
        .map(str::to_string)
        .and_then(|s| clean_opt(&s));

    Meta {
        title,
        byline,
        site_name,
        published_at,
        lang,
    }
}

struct LdArticle {
    body: String,
    headline: Option<String>,
    author: Option<String>,
    published: Option<String>,
    publisher: Option<String>,
}

fn find_ld_article(value: &Value) -> Option<LdArticle> {
    match value {
        Value::Array(items) => items.iter().find_map(find_ld_article),
        Value::Object(object) => {
            if let Some(body) = object.get("articleBody").and_then(Value::as_str) {
                return Some(LdArticle {
                    body: body.to_string(),
                    headline: object
                        .get("headline")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    author: object.get("author").and_then(ld_name),
                    published: object
                        .get("datePublished")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    publisher: object.get("publisher").and_then(ld_name),
                });
            }
            if let Some(graph) = object.get("@graph")
                && let Some(found) = find_ld_article(graph)
            {
                return Some(found);
            }
            object.values().find_map(find_ld_article)
        }
        _ => None,
    }
}

fn ld_name(value: &Value) -> Option<String> {
    match value {
        Value::String(name) => Some(name.clone()),
        Value::Array(items) => items.iter().find_map(ld_name),
        Value::Object(object) => object
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + word.chars().count() > max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn parse_any_date(value: &str) -> Option<i64> {
    parse_rfc3339(value).or_else(|| parse_date_loose(value))
}

fn clean_opt(value: &str) -> Option<String> {
    let clean = collapse_whitespace(&sanitize_text(value));
    if clean.is_empty() { None } else { Some(clean) }
}

fn host_title(final_url: &str) -> String {
    url::Url::parse(final_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "Untitled article".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_structured_content() {
        let html = r#"<!doctype html><html lang="en"><head>
            <title>Test article</title>
            <meta property="og:site_name" content="Example"/>
            <meta property="article:published_time" content="2025-03-04T10:00:00Z"/>
            </head><body>
            <nav><a href="/">Home</a></nav>
            <article>
              <h1>A headline</h1>
              <p>This is the first paragraph with <strong>bold</strong> and <a href="/next">a link</a>.</p>
              <p>Second paragraph.</p>
              <ul><li>One</li><li>Two</li></ul>
              <blockquote><p>A quote.</p></blockquote>
              <pre><code class="language-rust">fn main() {}</code></pre>
              <table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>
            </article>
            <footer>boilerplate</footer>
            </body></html>"#;
        let article = extract(html, "https://example.com/a", "https://example.com/a").unwrap();
        assert!(!article.blocks.is_empty());
        assert!(article.word_count > 0);
        assert!(article.published_at.is_some());
    }

    #[test]
    fn rejects_tiny_page() {
        let html = "<html><body><p>hi</p></body></html>";
        assert!(extract(html, "https://example.com", "https://example.com").is_err());
    }
}
