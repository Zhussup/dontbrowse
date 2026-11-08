use std::time::Duration;

use tokio::process::Command;

use crate::adapters::youtube::ytdlp;
use crate::config::Config;

#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub yt_dlp_version: Option<String>,
    pub mpv_version: Option<String>,
}

impl Tools {
    pub async fn detect(cfg: &Config) -> Self {
        let yt_dlp_version = ytdlp::version(&cfg.yt_dlp_path).await;
        let mpv_version = mpv_version(&cfg.mpv_path).await;
        Self {
            yt_dlp_version,
            mpv_version,
        }
    }

    pub fn yt_dlp_ok(&self) -> bool {
        self.yt_dlp_version.is_some()
    }

    pub fn mpv_ok(&self) -> bool {
        self.mpv_version.is_some()
    }
}

async fn mpv_version(path: &str) -> Option<String> {
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .next()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
}
