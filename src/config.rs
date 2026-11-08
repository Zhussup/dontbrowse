use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cli::Cli;
use crate::util::paths::Paths;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Auto,
    Dark,
    Light,
}

impl ThemeChoice {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HomeMode {
    #[default]
    Auto,
    Trending,
    Recommended,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderConfig {
    /// Maximum article measure in columns.
    pub width: u16,
    pub images: bool,
    /// "none", "same-site", or "all"
    pub send_cookies_to_images: String,
    pub user_agent: String,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            width: 92,
            images: true,
            send_cookies_to_images: "same-site".to_string(),
            user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct YoutubeConfig {
    pub limit: usize,
    pub home: HomeMode,
    pub region: Option<String>,
    pub discover_query: String,
}

impl Default for YoutubeConfig {
    fn default() -> Self {
        Self {
            limit: 40,
            home: HomeMode::Auto,
            region: None,
            discover_query: "trending videos".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub animations: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { animations: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeChoice,
    pub yt_dlp_path: String,
    pub mpv_path: String,
    pub cookies_from_browser: Option<String>,
    /// Netscape cookies.txt passed to yt-dlp and imported for articles.
    pub cookies_txt: Option<PathBuf>,
    pub reader: ReaderConfig,
    pub youtube: YoutubeConfig,
    pub ui: UiConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::Auto,
            yt_dlp_path: "yt-dlp".to_string(),
            mpv_path: "mpv".to_string(),
            cookies_from_browser: None,
            cookies_txt: None,
            reader: ReaderConfig::default(),
            youtube: YoutubeConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

impl Config {
    pub fn load(paths: &Paths, cli: &Cli) -> anyhow::Result<Self> {
        let mut cfg = match std::fs::read_to_string(paths.config_file()) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(cfg) => cfg,
                Err(err) => {
                    tracing::warn!("invalid config: {err}");
                    Config::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(err) => return Err(err.into()),
        };

        if let Some(theme) = cli.theme.as_deref().and_then(ThemeChoice::parse) {
            cfg.theme = theme;
        }
        if cli.no_images {
            cfg.reader.images = false;
        }
        if let Some(cookies) = &cli.cookies {
            cfg.cookies_txt = Some(cookies.clone());
        }
        if let Some(browser) = &cli.cookies_from_browser {
            cfg.cookies_from_browser = Some(browser.clone());
        }
        if let Some(path) = &cli.yt_dlp {
            cfg.yt_dlp_path = path.clone();
        }
        if let Some(path) = &cli.mpv {
            cfg.mpv_path = path.clone();
        }
        if let Ok(v) = std::env::var("DONTBROWSE_REDUCED_MOTION")
            && !v.is_empty()
            && v != "0"
        {
            cfg.ui.animations = false;
        }
        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> anyhow::Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(paths.config_file(), text)?;
        Ok(())
    }

    pub fn cookies_active(&self) -> bool {
        self.cookies_from_browser.is_some() || self.cookies_txt.is_some()
    }
}
