use std::time::Duration;

use tokio::process::Command;

use crate::config::Config;
use crate::error::{AppError, Result};
use crate::util::text::sanitize_text;

pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(30);
pub const FEED_TIMEOUT: Duration = Duration::from_secs(60);

/// Run yt-dlp with a fixed, predictable argument set and return stdout JSON.
pub async fn run_json(cfg: &Config, target: &str, timeout: Duration) -> Result<String> {
    let mut cmd = Command::new(&cfg.yt_dlp_path);
    cmd.args([
        "--ignore-config",
        "--no-warnings",
        "--no-progress",
        "--color",
        "never",
        "--socket-timeout",
        "15",
        "--retries",
        "2",
        "--extractor-retries",
        "2",
        "--flat-playlist",
        "--dump-single-json",
    ]);

    if let Some(browser) = &cfg.cookies_from_browser {
        cmd.args(["--cookies-from-browser", browser]);
    }
    if let Some(file) = &cfg.cookies_txt
        && let Some(path) = file.to_str()
    {
        cmd.args(["--cookies", path]);
    }

    cmd.arg(target);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.kill_on_drop(true);

    let output = match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => {
            return Err(AppError::Tool(format!(
                "yt-dlp timed out after {}s",
                timeout.as_secs()
            )));
        }
        Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::YtDlpMissing);
        }
        Ok(Err(err)) => {
            return Err(AppError::Tool(format!("could not start yt-dlp: {err}")));
        }
        Ok(Ok(out)) => out,
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify(&stderr));
    }

    if output.stdout.len() > 32 * 1024 * 1024 {
        return Err(AppError::Tool(
            "yt-dlp returned an unexpectedly large response".to_string(),
        ));
    }

    String::from_utf8(output.stdout)
        .map_err(|_| AppError::Tool("yt-dlp returned non-UTF-8 output".to_string()))
}

fn classify(stderr: &str) -> AppError {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("sign in to confirm")
        || lower.contains("confirm you're not a bot")
        || lower.contains("confirm you\u{2019}re not a bot")
        || lower.contains("cookies for the authentication")
    {
        AppError::CookiesRequired("YouTube".to_string())
    } else if lower.contains("429") || lower.contains("too many requests") {
        AppError::Network(
            "YouTube rate limited the request; wait a bit or use browser cookies".to_string(),
        )
    } else if lower.contains("could not copy") && lower.contains("cookie") {
        AppError::Tool(
            "could not read browser cookies; close the browser or export cookies.txt".to_string(),
        )
    } else if lower.contains("unable to extract") || lower.contains("nsig") {
        AppError::Tool("yt-dlp could not extract this video; it may need to be updated".to_string())
    } else if lower.contains("private video")
        || lower.contains("video unavailable")
        || lower.contains("this video is not available")
    {
        AppError::Tool(format!("YouTube: {}", tail(stderr)))
    } else {
        AppError::Tool(format!("yt-dlp: {}", tail(stderr)))
    }
}

fn tail(stderr: &str) -> String {
    let text = sanitize_text(stderr).replace('\n', " ");
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(400);
    chars[start..].iter().collect::<String>().trim().to_string()
}

/// Quick version probe used at startup for the header badge.
pub async fn version(path: &str) -> Option<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new(path).arg("--version").output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
