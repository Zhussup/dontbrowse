use url::Url;

/// Resolve a URL against the page base and allow only safe schemes.
pub fn resolve_url(base: &Url, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("file:")
        || lower.starts_with("blob:")
    {
        return None;
    }
    if lower.starts_with("mailto:") {
        return Some(raw.to_string());
    }
    let joined = base.join(raw).ok()?;
    match joined.scheme() {
        "http" | "https" => Some(joined.to_string()),
        _ => None,
    }
}

/// True when a stored link can be emitted as an OSC 8 hyperlink safely.
pub fn is_safe_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.contains('\u{1b}') || lower.contains('\u{7}') || lower.contains('\n') {
        return false;
    }
    if lower.starts_with("mailto:") {
        return true;
    }
    Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https"))
        .unwrap_or(false)
}

pub fn largest_srcset(srcset: &str) -> Option<String> {
    srcset
        .split(',')
        .filter_map(|candidate| {
            let mut parts = candidate.split_whitespace();
            let url = parts.next()?;
            let descriptor = parts.next().unwrap_or("");
            let weight = if let Some(w) = descriptor.strip_suffix('w') {
                w.parse::<u32>().ok()
            } else if let Some(x) = descriptor.strip_suffix('x') {
                x.parse::<f32>().ok().map(|v| (v * 1000.0) as u32)
            } else {
                Some(0)
            }?;
            Some((weight, url.to_string()))
        })
        .max_by_key(|(weight, _)| *weight)
        .map(|(_, url)| url)
}
