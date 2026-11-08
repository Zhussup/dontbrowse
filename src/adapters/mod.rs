pub mod article;
pub mod youtube;

use url::Url;

pub fn is_youtube_url(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("youtube.com")
            | Some("www.youtube.com")
            | Some("m.youtube.com")
            | Some("music.youtube.com")
            | Some("youtu.be")
    )
}
