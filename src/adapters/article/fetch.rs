use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use reqwest_cookie_store::CookieStoreMutex;
use url::Url;

use crate::config::Config;
use crate::error::{AppError, Result};

const MAX_BODY: usize = 8 * 1024 * 1024;

pub struct FetchResult {
    pub final_url: String,
    pub html: String,
    pub status: u16,
}

pub async fn fetch(url: &Url, cfg: &Config, store: Arc<CookieStoreMutex>) -> Result<FetchResult> {
    let client = reqwest::Client::builder()
        .user_agent(&cfg.reader.user_agent)
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_provider(store)
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

    let mut response = client
        .get(url.clone())
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,text/plain;q=0.8,*/*;q=0.5",
        )
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    let status = response.status();
    if matches!(status.as_u16(), 401..=403) {
        let host = url.host_str().unwrap_or("this site");
        return Err(if cfg.cookies_active() {
            AppError::PaywallBlocked(format!("{host} returned HTTP {}", status.as_u16()))
        } else {
            AppError::CookiesRequired(host.to_string())
        });
    }
    if status.as_u16() >= 400 {
        return Err(AppError::Network(format!(
            "HTTP {} from {}",
            status.as_u16(),
            url.host_str().unwrap_or("site")
        )));
    }

    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.is_empty()
        && !content_type.contains("html")
        && !content_type.contains("xml")
        && !content_type.contains("text/plain")
    {
        return Err(AppError::Unsupported(format!(
            "content type {content_type}; press O to open externally"
        )));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?
    {
        if body.len() + chunk.len() > MAX_BODY {
            return Err(AppError::Unsupported(
                "page is larger than 8 MiB".to_string(),
            ));
        }
        body.extend_from_slice(&chunk);
    }

    let charset = charset_from_content_type(&content_type).or_else(|| sniff_charset(&body));
    let encoding = charset
        .as_deref()
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(&body);

    Ok(FetchResult {
        final_url,
        html: text.into_owned(),
        status: status.as_u16(),
    })
}

fn charset_from_content_type(content_type: &str) -> Option<String> {
    for part in content_type.split(';').skip(1) {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("charset=") {
            let value = value.trim_matches('"').trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn sniff_charset(body: &[u8]) -> Option<String> {
    let window = &body[..body.len().min(2048)];
    let text = String::from_utf8_lossy(window).to_ascii_lowercase();
    let index = text.find("charset=")?;
    let rest = &text[index + 8..];
    let value: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    if value.is_empty() { None } else { Some(value) }
}
