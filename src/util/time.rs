use time::OffsetDateTime;

pub fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Human relative time such as "just now", "12m ago", "3d ago".
pub fn relative_time(ts: i64, now: i64) -> String {
    let secs = now.saturating_sub(ts);
    if secs < 60 {
        return "just now".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d ago");
    }
    let weeks = days / 7;
    if weeks < 5 {
        return format!("{weeks}w ago");
    }
    let months = days / 30;
    if months < 12 {
        return format!("{months}mo ago");
    }
    format!("{}y ago", days / 365)
}

/// "4:12" or "1:02:03".
pub fn format_clock(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    format_duration(total as u32)
}

pub fn format_duration(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 1234 -> "1.2K", 3_400_000 -> "3.4M".
pub fn format_count(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    if n < 1_000_000 {
        let v = n as f64 / 1_000.0;
        return trim_float(v, "K");
    }
    if n < 1_000_000_000 {
        let v = n as f64 / 1_000_000.0;
        return trim_float(v, "M");
    }
    let v = n as f64 / 1_000_000_000.0;
    trim_float(v, "B")
}

fn trim_float(v: f64, suffix: &str) -> String {
    if v >= 100.0 {
        format!("{v:.0}{suffix}")
    } else {
        format!("{v:.1}{suffix}")
    }
}

pub fn format_views(n: u64) -> String {
    format!("{} views", format_count(n))
}

pub fn reading_minutes(words: usize) -> usize {
    (words / 220).max(1)
}

pub fn parse_rfc3339(s: &str) -> Option<i64> {
    OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|t| t.unix_timestamp())
}

#[allow(deprecated)]
pub fn parse_date_loose(s: &str) -> Option<i64> {
    if let Some(ts) = parse_rfc3339(s) {
        return Some(ts);
    }
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%d %B %Y", "%B %d, %Y"] {
        if let Ok(format) = time::format_description::parse(
            &fmt.replace("%Y", "[year]")
                .replace("%m", "[month]")
                .replace("%d", "[day]")
                .replace("%B", "[month repr:long]"),
        ) && let Ok(date) = time::Date::parse(s, &format)
        {
            return Some(date.midnight().assume_utc().unix_timestamp());
        }
    }
    None
}

/// Format a unix timestamp as a short absolute date, e.g. "Mar 4, 2025".
#[allow(deprecated)]
pub fn format_date(ts: i64) -> String {
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(ts) else {
        return String::new();
    };
    let fmt = time::format_description::parse("[month repr:short] [day], [year]").ok();
    match fmt {
        Some(f) => dt.format(&f).unwrap_or_default(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_thresholds() {
        let now = 1_000_000_000;
        assert_eq!(relative_time(now - 5, now), "just now");
        assert_eq!(relative_time(now - 120, now), "2m ago");
        assert_eq!(relative_time(now - 7200, now), "2h ago");
        assert_eq!(relative_time(now - 86400 * 3, now), "3d ago");
        assert_eq!(relative_time(now - 86400 * 14, now), "2w ago");
        assert_eq!(relative_time(now - 86400 * 40, now), "1mo ago");
        assert_eq!(relative_time(now - 86400 * 400, now), "1y ago");
    }

    #[test]
    fn counts() {
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_200), "1.2K");
        assert_eq!(format_count(3_400_000), "3.4M");
        assert_eq!(format_duration(38), "0:38");
        assert_eq!(format_duration(3723), "1:02:03");
    }
}
