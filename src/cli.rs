use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "dontbrowse",
    version,
    about = "A terminal browser for YouTube and the readable web"
)]
pub struct Cli {
    /// URL to open on launch.
    #[arg(value_name = "URL")]
    pub url: Option<String>,

    /// Run a YouTube search immediately.
    #[arg(long, value_name = "QUERY")]
    pub search: Option<String>,

    /// Serve cached content without network access.
    #[arg(long)]
    pub offline: bool,

    /// Enable verbose file logging.
    #[arg(long)]
    pub debug: bool,

    /// Netscape cookies.txt to use for articles and yt-dlp.
    #[arg(long, value_name = "FILE")]
    pub cookies: Option<PathBuf>,

    /// Import cookies from a browser (e.g. firefox, chromium:Default).
    #[arg(long, value_name = "BROWSER")]
    pub cookies_from_browser: Option<String>,

    /// Import subscriptions from a CSV/JSON file and exit.
    #[arg(long, value_name = "FILE")]
    pub import_subscriptions: Option<PathBuf>,

    /// Theme: auto, dark, or light.
    #[arg(long, value_name = "THEME")]
    pub theme: Option<String>,

    /// Do not load or render images.
    #[arg(long)]
    pub no_images: bool,

    /// Path to the yt-dlp executable.
    #[arg(long, value_name = "PATH")]
    pub yt_dlp: Option<String>,

    /// Path to the mpv executable.
    #[arg(long, value_name = "PATH")]
    pub mpv: Option<String>,
}
