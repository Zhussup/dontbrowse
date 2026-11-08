use std::time::Instant;

use crate::db::SavedItem;
use crate::media::PlaybackMode;
use crate::model::{Article, FeedPage, Video};
use crate::render::StyledLine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Search,
    Subscriptions,
    Library,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Home, Tab::Search, Tab::Subscriptions, Tab::Library];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Search => "Search",
            Tab::Subscriptions => "Subscriptions",
            Tab::Library => "Library",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Tab::Home => 0,
            Tab::Search => 1,
            Tab::Subscriptions => 2,
            Tab::Library => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Feeds(Tab),
    Reader,
}

#[derive(Clone, Debug, Default)]
pub struct FeedState {
    pub page: Option<FeedPage>,
    pub loading: bool,
    pub error: Option<String>,
    pub selection: usize,
    pub scroll_row: usize,
    pub cols: usize,
    pub viewport_rows: usize,
    pub job: u64,
}

impl FeedState {
    pub fn items(&self) -> &[Video] {
        self.page
            .as_ref()
            .map(|page| page.items.as_slice())
            .unwrap_or_default()
    }

    pub fn selected(&self) -> Option<&Video> {
        self.items().get(self.selection)
    }

    pub fn begin_load(&mut self, job: u64) {
        self.job = job;
        self.loading = true;
        self.error = None;
    }

    pub fn fail(&mut self, message: String) {
        self.loading = false;
        self.error = Some(message);
    }

    pub fn apply(&mut self, page: FeedPage) {
        self.selection = self.selection.min(page.items.len().saturating_sub(1));
        self.page = Some(page);
        self.loading = false;
        self.error = None;
        self.scroll_row = 0;
    }

    pub fn move_selection(&mut self, dx: i32, dy: i32) {
        let count = self.items().len();
        if count == 0 {
            return;
        }
        let cols = self.cols.max(1) as i32;
        let mut index = self.selection as i32;
        index += dx;
        if dy != 0 {
            index += dy * cols;
        }
        self.selection = index.clamp(0, count as i32 - 1) as usize;
    }
}

pub struct ReaderCache {
    pub width: u16,
    pub lines: Vec<StyledLine>,
    pub block_starts: Vec<usize>,
    pub total_lines: usize,
}

pub struct ReaderState {
    pub article: Option<Article>,
    pub loading: bool,
    pub error: Option<String>,
    pub url: String,
    pub scroll: usize,
    pub viewport_h: usize,
    pub cache: Option<ReaderCache>,
    pub job: u64,
    pub pending_progress: Option<f64>,
    pub toc_open: bool,
}

impl Default for ReaderState {
    fn default() -> Self {
        Self {
            article: None,
            loading: false,
            error: None,
            url: String::new(),
            scroll: 0,
            viewport_h: 0,
            cache: None,
            job: 0,
            pending_progress: None,
            toc_open: true,
        }
    }
}

impl ReaderState {
    pub fn begin_load(&mut self, job: u64, url: String) {
        self.job = job;
        self.url = url;
        self.loading = true;
        self.error = None;
        self.article = None;
        self.cache = None;
        self.scroll = 0;
        self.pending_progress = None;
    }

    pub fn apply(&mut self, article: Article) {
        self.url = article.final_url.clone();
        self.article = Some(article);
        self.loading = false;
        self.error = None;
        self.cache = None;
        self.scroll = 0;
    }

    pub fn fail(&mut self, message: String) {
        self.loading = false;
        self.error = Some(message);
    }

    pub fn max_scroll(&self) -> usize {
        self.cache
            .as_ref()
            .map(|cache| cache.total_lines.saturating_sub(self.viewport_h.max(1)))
            .unwrap_or(0)
    }

    pub fn progress(&self) -> f64 {
        let max = self.max_scroll();
        if max == 0 {
            0.0
        } else {
            (self.scroll as f64 / max as f64).clamp(0.0, 1.0)
        }
    }

    pub fn current_block(&self) -> Option<usize> {
        let cache = self.cache.as_ref()?;
        let mut current = None;
        for (index, start) in cache.block_starts.iter().enumerate() {
            if *start <= self.scroll {
                current = Some(index);
            } else {
                break;
            }
        }
        current
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerPhase {
    Idle,
    Loading,
    Playing,
    Paused,
    Buffering,
    Ended,
    Failed,
}

pub struct PlayerState {
    pub phase: PlayerPhase,
    pub current: Option<Video>,
    pub mode: PlaybackMode,
    pub position: f64,
    pub duration: Option<f64>,
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    pub paused: bool,
    pub buffering: bool,
    pub error: Option<String>,
    pub queue: Vec<Video>,
    pub updated_at: Instant,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            phase: PlayerPhase::Idle,
            current: None,
            mode: PlaybackMode::Audio,
            position: 0.0,
            duration: None,
            volume: 100.0,
            muted: false,
            speed: 1.0,
            paused: false,
            buffering: false,
            error: None,
            queue: Vec::new(),
            updated_at: Instant::now(),
        }
    }
}

impl PlayerState {
    pub fn active(&self) -> bool {
        self.current.is_some() || !matches!(self.phase, PlayerPhase::Idle)
    }

    pub fn display_position(&self) -> f64 {
        let mut position = self.position;
        if matches!(self.phase, PlayerPhase::Playing) {
            position += self.updated_at.elapsed().as_secs_f64();
        }
        if let Some(duration) = self.duration {
            position = position.min(duration);
        }
        position.max(0.0)
    }

    pub fn progress(&self) -> f64 {
        match self.duration {
            Some(duration) if duration > 0.0 => {
                (self.display_position() / duration).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }
}

#[derive(Default)]
pub struct LibraryState {
    pub items: Vec<SavedItem>,
    pub selection: usize,
    pub viewport_rows: usize,
}

#[derive(Default)]
pub struct InputState {
    pub prompt: String,
    pub value: String,
    pub purpose: InputPurpose,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputPurpose {
    #[default]
    Omnibox,
    ImportCookies,
    ImportSubscriptions,
}

#[derive(Default)]
pub struct PaletteState {
    pub input: String,
    pub selection: usize,
}

#[derive(Default)]
pub struct LinkPickerState {
    pub input: String,
    pub selection: usize,
}

pub enum Overlay {
    None,
    Help { scroll: u16 },
    Palette(PaletteState),
    Omnibox(InputState),
    LinkPicker(LinkPickerState),
}

impl Overlay {
    pub fn is_none(&self) -> bool {
        matches!(self, Overlay::None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub expires_at: Option<Instant>,
}

impl Toast {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Info,
            expires_at: Some(Instant::now() + std::time::Duration::from_secs(4)),
        }
    }

    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Success,
            expires_at: Some(Instant::now() + std::time::Duration::from_secs(4)),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Warning,
            expires_at: Some(Instant::now() + std::time::Duration::from_secs(6)),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: ToastKind::Error,
            expires_at: Some(Instant::now() + std::time::Duration::from_secs(10)),
        }
    }

    pub fn expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires| Instant::now() >= expires)
    }
}
