pub mod blocks;
pub mod cookies;
pub mod extract;
pub mod fetch;
pub mod sanitize;

use url::Url;

use crate::config::Config;
use crate::error::{AppError, Result};
use crate::model::Article;
use crate::util::paths::Paths;

pub async fn load(cfg: &Config, paths: &Paths, url: &Url) -> Result<Article> {
    let store = cookies::load(paths);
    let fetched = fetch::fetch(url, cfg, store).await?;
    let article = extract::extract(&fetched.html, url.as_str(), &fetched.final_url)?;

    if looks_paywalled(&fetched.html) && article.word_count < 150 {
        let host = url.host_str().unwrap_or("this site").to_string();
        return Err(if cfg.cookies_active() {
            AppError::PaywallBlocked(format!(
                "{host} still shows a paywall; your cookies may be missing or expired"
            ))
        } else {
            AppError::CookiesRequired(host)
        });
    }

    Ok(article)
}

fn looks_paywalled(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    [
        "subscribe to read",
        "subscribe to continue",
        "subscribe now to read",
        "create a free account",
        "sign in to continue",
        "this article is for subscribers",
        "to continue reading",
    ]
    .iter()
    .any(|phrase| lower.contains(phrase))
}
