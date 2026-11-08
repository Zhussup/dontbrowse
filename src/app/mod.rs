pub mod commands;
pub mod state;

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use futures_util::StreamExt;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use tokio::sync::mpsc;
use url::Url;

use crate::adapters::article;
use crate::adapters::youtube::{self, cache_key as yt_cache_key};
use crate::app::commands::Action;
use crate::app::state::*;
use crate::cli::Cli;
use crate::config::{Config, ThemeChoice};
use crate::db::{Db, SavedItem};
use crate::error::AppError;
use crate::event::AppEvent;
use crate::images::{self, HalfblockImage};
use crate::media::{PlaybackMode, PlayerCommand, PlayerEvent, PlayerHandle};
use crate::model::{Article, ChannelTab, ExtractSource, FeedKey, FeedSource, FeedTarget, Video};
use crate::tools::Tools;
use crate::ui;
use crate::ui::links::LinkRegistry;
use crate::ui::theme::Theme;
use crate::util::paths::Paths;
use crate::util::text::fuzzy_match;
use crate::util::time::now_unix;

pub struct App {
    pub cfg: Config,
    pub paths: Paths,
    pub cli: Cli,
    pub tools: Tools,
    pub db: Db,
    pub theme: Theme,
    theme_choice: ThemeChoice,
    pub screen: Screen,
    pub tab: Tab,
    pub home: FeedState,
    pub search: FeedState,
    pub subs: FeedState,
    pub library: LibraryState,
    pub reader: ReaderState,
    pub reader_stack: Vec<ReaderState>,
    pub search_query: String,
    pub search_focus: bool,
    pub overlay: Overlay,
    pub player: PlayerState,
    player_handle: PlayerHandle,
    pub tx: mpsc::Sender<AppEvent>,
    rx: mpsc::Receiver<AppEvent>,
    http: reqwest::Client,
    pub images: HashMap<String, Arc<HalfblockImage>>,
    pub links: LinkRegistry,
    pending_images: HashSet<String>,
    job: u64,
    pending_feed_cache: HashMap<u64, String>,
    pending_article: Option<(u64, String, String)>,
    pub toast: Option<Toast>,
    quit: bool,
    tick: u64,
    offline: bool,
    pending_g: Option<Instant>,
    reader_measure: u16,
    last_history_save: Instant,
    last_progress_save: Instant,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cli: Cli,
        mut cfg: Config,
        paths: Paths,
        tools: Tools,
        db: Db,
        tx: mpsc::Sender<AppEvent>,
        rx: mpsc::Receiver<AppEvent>,
    ) -> Self {
        if crate::util::color::no_color() {
            cfg.reader.images = false;
        }
        let theme = Theme::detect(cfg.theme, cfg.ui.animations);
        let http = reqwest::Client::builder()
            .user_agent(&cfg.reader.user_agent)
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        let player_handle =
            crate::media::mpv::spawn(cfg.clone(), paths.clone(), tx.clone(), tools.mpv_ok());
        let reader_measure = cfg.reader.width;
        let offline = cli.offline;
        let theme_choice = cfg.theme;

        let mut app = Self {
            cfg,
            paths,
            cli,
            tools,
            db,
            theme,
            theme_choice,
            screen: Screen::Feeds(Tab::Home),
            tab: Tab::Home,
            home: FeedState::default(),
            search: FeedState::default(),
            subs: FeedState::default(),
            library: LibraryState::default(),
            reader: ReaderState::default(),
            reader_stack: Vec::new(),
            search_query: String::new(),
            search_focus: false,
            overlay: Overlay::None,
            player: PlayerState::default(),
            player_handle,
            tx,
            rx,
            http,
            images: HashMap::new(),
            links: LinkRegistry::new(),
            pending_images: HashSet::new(),
            job: 0,
            pending_feed_cache: HashMap::new(),
            pending_article: None,
            toast: None,
            quit: false,
            tick: 0,
            offline,
            pending_g: None,
            reader_measure,
            last_history_save: Instant::now(),
            last_progress_save: Instant::now(),
        };

        if let Some(query) = app.cli.search.clone() {
            app.search_query = query;
            app.tab = Tab::Search;
            app.screen = Screen::Feeds(Tab::Search);
            app.search_focus = true;
            app.run_search(true);
        } else if let Some(url) = app.cli.url.clone() {
            app.open_input(url);
        } else {
            app.request_feed(FeedTarget::Home, FeedKey::Trending, false);
        }

        app
    }

    pub async fn run(
        &mut self,
        terminal: &mut ratatui::Terminal<crate::ui::backend::LinkBackend>,
    ) -> anyhow::Result<()> {
        let mut events = crossterm::event::EventStream::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(50));
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

        loop {
            tokio::select! {
                biased;
                maybe_event = events.next() => {
                    if let Some(Ok(event)) = maybe_event {
                        self.on_terminal(event);
                    }
                }
                Some(event) = self.rx.recv() => {
                    self.on_app_event(event);
                }
                _ = ticker.tick() => {
                    self.on_tick();
                }
                _ = sigterm.recv() => {
                    self.quit = true;
                }
            }

            terminal.draw(|frame| self.draw(frame))?;
            if self.quit {
                break;
            }
        }

        self.shutdown().await;
        Ok(())
    }

    async fn shutdown(&mut self) {
        self.player_handle.send(PlayerCommand::Shutdown);
        if let Some(article) = &self.reader.article {
            self.db
                .set_article_position(&article.final_url, self.reader.progress());
        }
        tokio::time::sleep(Duration::from_millis(450)).await;
    }

    // ------------------------------------------------------------------
    // drawing
    // ------------------------------------------------------------------

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        self.links.clear();
        ui::fill_background(frame, &self.theme);

        if area.width < 40 || area.height < 12 {
            let rect = ui::centered(area, 38.min(area.width), 3);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    "Terminal too small (minimum 40×12)",
                    Style::new().fg(self.theme.warning),
                ))
                .alignment(Alignment::Center),
                rect,
            );
            return;
        }

        let player_height = if self.player.active() && self.player.current.is_some() {
            4
        } else {
            0
        };
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(player_height),
            Constraint::Length(1),
        ])
        .split(area);

        let title = self.screen_title();
        let active_tab = match self.screen {
            Screen::Feeds(tab) => Some(tab),
            Screen::Reader => None,
        };
        ui::header::draw(
            frame,
            rows[0],
            &self.theme,
            &title,
            &self.tools,
            active_tab,
            "Ctrl+P palette  ·  Ctrl+L open  ·  ? help",
        );

        match self.screen {
            Screen::Feeds(Tab::Home) => {
                self.draw_feed_tab(frame, rows[1], Tab::Home);
            }
            Screen::Feeds(Tab::Search) => {
                let parts =
                    Layout::vertical([Constraint::Length(3), Constraint::Min(4)]).split(rows[1]);
                self.draw_search_bar(frame, parts[0]);
                self.draw_feed_tab(frame, parts[1], Tab::Search);
            }
            Screen::Feeds(Tab::Subscriptions) => {
                self.draw_feed_tab(frame, rows[1], Tab::Subscriptions);
            }
            Screen::Feeds(Tab::Library) => {
                self.draw_library(frame, rows[1]);
            }
            Screen::Reader => {
                ui::reader::draw(
                    frame,
                    rows[1],
                    &mut self.reader,
                    &self.theme,
                    &self.images,
                    self.reader_measure,
                    &self.links,
                );
                if let Some(cache) = &self.reader.cache {
                    self.schedule_article_images(cache.width);
                }
            }
        }

        if player_height > 0 {
            ui::player_bar::draw(frame, rows[2], &self.theme, &self.player);
        }

        let (hints, message) = self.status_line();
        let message_ref = message
            .as_ref()
            .map(|(text, color)| (text.as_str(), *color));
        ui::status_bar::draw(frame, rows[3], &self.theme, &hints, message_ref);

        self.draw_overlay(frame, area);

        if let Some(toast) = &self.toast {
            ui::toast::draw(frame, area, &self.theme, toast);
        }
    }

    fn draw_feed_tab(&mut self, frame: &mut Frame<'_>, area: Rect, tab: Tab) {
        let metrics = ui::feed::metrics(area);
        let (empty_title, empty_body) = match tab {
            Tab::Home => (
                "Nothing to show",
                "Press r to refresh, or Ctrl+L to open a URL.",
            ),
            Tab::Search => ("Search YouTube", "Type a query above and press Enter."),
            Tab::Subscriptions => (
                "No subscriptions yet",
                "Press s on a video card, or import a subscriptions file (Ctrl+P).",
            ),
            Tab::Library => ("Library", "Nothing saved yet."),
        };

        let target = match tab {
            Tab::Home => FeedTarget::Home,
            Tab::Search => FeedTarget::Search,
            Tab::Subscriptions => FeedTarget::Subscriptions,
            Tab::Library => FeedTarget::Library,
        };
        let state = match tab {
            Tab::Home => &mut self.home,
            Tab::Search => &mut self.search,
            Tab::Subscriptions => &mut self.subs,
            Tab::Library => return,
        };
        ui::feed::draw(
            frame,
            area,
            state,
            &self.theme,
            &self.images,
            empty_title,
            empty_body,
            self.tick,
        );
        self.schedule_feed_images(target, metrics);
    }

    fn draw_search_bar(&self, frame: &mut Frame<'_>, area: Rect) {
        let block = ui::panel(&self.theme);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if inner.height == 0 {
            return;
        }
        let mut spans = vec![
            Span::styled("Search YouTube  › ", Style::new().fg(self.theme.accent)),
            Span::styled(self.search_query.clone(), Style::new().fg(self.theme.text)),
        ];
        if self.search_focus {
            spans.push(Span::styled("▏", Style::new().fg(self.theme.accent)));
        }
        let hint = if self.search_focus {
            "  Enter search · ↓ results · Esc clear"
        } else {
            "  / to edit"
        };
        spans.push(Span::styled(hint, self.theme.faint()));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::new().bg(self.theme.surface)),
            inner,
        );
    }

    fn draw_library(&mut self, frame: &mut Frame<'_>, area: Rect) {
        if self.library.items.is_empty() {
            ui::feed::message_panel(
                frame,
                area,
                &self.theme,
                "Library",
                "Nothing saved yet. Press L on a video or article.",
                self.theme.text_dim,
                "Ctrl+L open URL",
            );
            return;
        }

        let visible = area.height.max(1) as usize;
        self.library.viewport_rows = visible;
        if self.library.selection >= self.library.items.len() {
            self.library.selection = self.library.items.len() - 1;
        }
        let mut lines = Vec::new();
        let start = if self.library.selection < visible {
            0
        } else {
            self.library.selection + 1 - visible
        };
        for (index, item) in self
            .library
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
        {
            let selected = index == self.library.selection;
            let style = if selected {
                Style::new()
                    .fg(self.theme.accent)
                    .bg(self.theme.selection_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(self.theme.text_dim)
            };
            let marker = if selected { "▸ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker.to_string(), style),
                Span::styled(item.title.clone(), style),
                Span::styled(
                    format!("   {}", item.kind),
                    Style::new().fg(self.theme.text_faint),
                ),
            ]));
        }
        frame.render_widget(
            Paragraph::new(lines).style(Style::new().bg(self.theme.bg)),
            area,
        );
    }

    fn draw_overlay(&mut self, frame: &mut Frame<'_>, area: Rect) {
        match &self.overlay {
            Overlay::None => {}
            Overlay::Help { scroll } => {
                ui::help::draw(frame, area, &self.theme, *scroll);
            }
            Overlay::Palette(state) => {
                let filtered = commands::filtered(&state.input);
                let names: Vec<&str> = filtered.iter().map(|command| command.name).collect();
                ui::palette::draw(frame, area, &self.theme, state, &names);
            }
            Overlay::Omnibox(input) => {
                draw_input_overlay(frame, area, &self.theme, &input.prompt, &input.value);
            }
            Overlay::LinkPicker(state) => {
                let links = self
                    .reader
                    .article
                    .as_ref()
                    .map(|article| filter_links(article, &state.input))
                    .unwrap_or_default();
                let names: Vec<String> =
                    links.iter().map(|(_, text)| truncate_link(text)).collect();
                let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
                ui::palette::draw(
                    frame,
                    area,
                    &self.theme,
                    &PaletteState {
                        input: state.input.clone(),
                        selection: state.selection,
                    },
                    &name_refs,
                );
            }
        }
    }

    fn schedule_feed_images(&mut self, target: FeedTarget, metrics: ui::feed::GridMetrics) {
        if !self.cfg.reader.images {
            return;
        }
        let pending_cap = 8usize;
        if self.pending_images.len() >= pending_cap {
            return;
        }
        let mut slots = pending_cap - self.pending_images.len();
        let items: Vec<Video> = match target {
            FeedTarget::Home => self.home.items().to_vec(),
            FeedTarget::Search => self.search.items().to_vec(),
            FeedTarget::Subscriptions => self.subs.items().to_vec(),
            FeedTarget::Library => Vec::new(),
        };
        for video in items {
            if slots == 0 {
                break;
            }
            let url = video
                .thumbnail_url
                .clone()
                .unwrap_or_else(|| format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", video.id));
            let key = ui::feed::thumbnail_key(&video.id, metrics.thumb_w, metrics.thumb_h);
            if self.images.contains_key(&key) || self.pending_images.contains(&key) {
                continue;
            }
            self.pending_images.insert(key.clone());
            slots -= 1;
            let tx = self.tx.clone();
            let paths = self.paths.clone();
            let client = self.http.clone();
            let truecolor = self.theme.truecolor;
            let width = metrics.thumb_w;
            let height = metrics.thumb_h;
            tokio::spawn(async move {
                let result = async {
                    let bytes = images::fetch_bytes(&client, &paths, &url).await?;
                    tokio::task::spawn_blocking(move || {
                        images::halfblock::from_bytes_fill(&bytes, width, height, truecolor)
                    })
                    .await?
                }
                .await;
                match result {
                    Ok(image) => {
                        let _ = tx
                            .send(AppEvent::ImageReady {
                                key,
                                image: Arc::new(image),
                            })
                            .await;
                    }
                    Err(_) => {
                        let _ = tx.send(AppEvent::ImageFailed { key }).await;
                    }
                }
            });
        }
    }

    fn schedule_article_images(&mut self, width: u16) {
        if !self.cfg.reader.images {
            return;
        }
        let Some(article) = &self.reader.article else {
            return;
        };
        let pending_cap = 4usize;
        if self.pending_images.len() >= pending_cap {
            return;
        }
        let mut slots = pending_cap - self.pending_images.len();
        let mut seen = HashSet::new();
        let mut requests = Vec::new();
        for block in &article.blocks {
            let image = match block {
                crate::model::Block::Image(image) => Some(image),
                crate::model::Block::Figure { image, .. } => Some(image),
                _ => None,
            };
            if let Some(image) = image {
                if !seen.insert(image.id.clone()) {
                    continue;
                }
                requests.push((image.id.clone(), image.src.clone()));
            }
        }
        let measure = width.max(20);
        let height = 18u16;
        for (id, url) in requests {
            if slots == 0 {
                break;
            }
            let key = format!("art:{id}:{measure}x{height}");
            if self.images.contains_key(&key) || self.pending_images.contains(&key) {
                continue;
            }
            self.pending_images.insert(key.clone());
            slots -= 1;
            let tx = self.tx.clone();
            let paths = self.paths.clone();
            let client = self.http.clone();
            let truecolor = self.theme.truecolor;
            tokio::spawn(async move {
                let result = async {
                    let bytes = images::fetch_bytes(&client, &paths, &url).await?;
                    tokio::task::spawn_blocking(move || {
                        images::halfblock::from_bytes_fit(&bytes, measure, height, truecolor)
                    })
                    .await?
                }
                .await;
                match result {
                    Ok(image) => {
                        let _ = tx
                            .send(AppEvent::ImageReady {
                                key,
                                image: Arc::new(image),
                            })
                            .await;
                    }
                    Err(_) => {
                        let _ = tx.send(AppEvent::ImageFailed { key }).await;
                    }
                }
            });
        }
    }

    fn screen_title(&self) -> String {
        match self.screen {
            Screen::Feeds(Tab::Home) => self
                .home
                .page
                .as_ref()
                .map(|page| format!("Home  ·  {}", page.title))
                .unwrap_or_else(|| "Home".to_string()),
            Screen::Feeds(Tab::Search) => {
                if self.search_query.is_empty() {
                    "Search".to_string()
                } else {
                    format!("Search  ·  {}", self.search_query)
                }
            }
            Screen::Feeds(Tab::Subscriptions) => "Subscriptions".to_string(),
            Screen::Feeds(Tab::Library) => "Library".to_string(),
            Screen::Reader => self
                .reader
                .article
                .as_ref()
                .map(|article| article.title.clone())
                .unwrap_or_else(|| {
                    if self.reader.loading {
                        "Loading…".to_string()
                    } else {
                        "Reader".to_string()
                    }
                }),
        }
    }

    fn status_line(&self) -> (String, Option<(String, ratatui::style::Color)>) {
        let message = if self.offline {
            Some(("OFFLINE".to_string(), self.theme.warning))
        } else {
            None
        };
        match self.screen {
            Screen::Feeds(Tab::Search) if self.search_focus => (
                "Enter search  ·  ↓ results  ·  Esc clear".to_string(),
                message,
            ),
            Screen::Feeds(Tab::Library) => (
                "j/k move  ·  Enter open  ·  d remove  ·  r reload".to_string(),
                message,
            ),
            Screen::Feeds(_) => (
                "h j k l move  ·  Enter play  ·  v video  ·  s subscribe  ·  L save  ·  r refresh"
                    .to_string(),
                message,
            ),
            Screen::Reader => {
                if let (Some(article), Some(_cache)) = (&self.reader.article, &self.reader.cache) {
                    let percent = (self.reader.progress() * 100.0).round() as i64;
                    (
                        format!(
                            "{percent}%  ·  {} words  ·  {} min  ·  j/k scroll  ·  [ ] headings  ·  o links  ·  Backspace back",
                            article.word_count,
                            article.reading_minutes()
                        ),
                        message,
                    )
                } else {
                    (
                        "Esc back  ·  r reload  ·  O open in browser".to_string(),
                        message,
                    )
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // input
    // ------------------------------------------------------------------

    fn on_terminal(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key),
            Event::Paste(text) => self.on_paste(text),
            _ => {}
        }
    }

    fn on_paste(&mut self, text: String) {
        let clean = crate::util::text::sanitize_text(&text).replace(['\n', '\r'], " ");
        match &mut self.overlay {
            Overlay::Omnibox(input) => input.value.push_str(&clean),
            Overlay::Palette(state) => state.input.push_str(&clean),
            Overlay::LinkPicker(state) => state.input.push_str(&clean),
            _ => {
                if self.is_search_typing() {
                    self.search_query.push_str(&clean);
                }
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.quit = true;
            return;
        }

        if !self.overlay.is_none() {
            self.on_overlay_key(key);
            return;
        }

        if self.is_search_typing() {
            self.on_search_key(key);
            return;
        }

        if self.handle_global_key(&key) {
            return;
        }

        match self.screen {
            Screen::Feeds(Tab::Library) => self.on_library_key(key),
            Screen::Feeds(_) => self.on_feed_key(key),
            Screen::Reader => self.on_reader_key(key),
        }
    }

    fn is_search_typing(&self) -> bool {
        matches!(self.screen, Screen::Feeds(Tab::Search)) && self.search_focus
    }

    fn handle_global_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') => {
                self.quit = true;
                true
            }
            KeyCode::Char('?') => {
                self.overlay = Overlay::Help { scroll: 0 };
                true
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.overlay = Overlay::Palette(PaletteState::default());
                true
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_omnibox();
                true
            }
            KeyCode::Char(':') => {
                self.open_omnibox();
                true
            }
            KeyCode::Char('1') => {
                self.switch_tab(Tab::Home);
                true
            }
            KeyCode::Char('2') => {
                self.switch_tab(Tab::Search);
                true
            }
            KeyCode::Char('3') => {
                self.switch_tab(Tab::Subscriptions);
                true
            }
            KeyCode::Char('4') => {
                self.switch_tab(Tab::Library);
                true
            }
            KeyCode::Tab => {
                self.switch_tab(next_tab(self.current_tab()));
                true
            }
            KeyCode::BackTab => {
                self.switch_tab(prev_tab(self.current_tab()));
                true
            }
            KeyCode::Esc => {
                self.on_escape();
                true
            }
            _ => false,
        }
    }

    fn on_escape(&mut self) {
        match self.screen {
            Screen::Reader => self.reader_back(),
            Screen::Feeds(Tab::Search) => {
                self.search_focus = false;
            }
            _ => {}
        }
    }

    fn on_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.run_search(false);
                self.search_focus = false;
            }
            KeyCode::Down => self.search_focus = false,
            KeyCode::Esc => {
                if self.search_query.is_empty() {
                    self.search_focus = false;
                } else {
                    self.search_query.clear();
                }
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.search_query.push(c);
            }
            _ => {}
        }
    }

    fn on_overlay_key(&mut self, key: KeyEvent) {
        match &mut self.overlay {
            Overlay::None => {}
            Overlay::Help { scroll } => {
                let mut close = false;
                match key.code {
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => close = true,
                    KeyCode::Down | KeyCode::Char('j') => {
                        *scroll = scroll.saturating_add(1);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        *scroll = scroll.saturating_sub(1);
                    }
                    KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                    _ => {}
                }
                if close {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::Palette(state) => {
                let mut close = false;
                let mut action = None;
                match key.code {
                    KeyCode::Esc => close = true,
                    KeyCode::Enter => {
                        let filtered = commands::filtered(&state.input);
                        if let Some(command) = filtered.get(state.selection).copied() {
                            action = Some(command.action);
                        }
                        close = true;
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        let len = commands::filtered(&state.input).len();
                        if state.selection + 1 < len {
                            state.selection += 1;
                        }
                    }
                    KeyCode::Up | KeyCode::BackTab => {
                        state.selection = state.selection.saturating_sub(1);
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                        state.selection = 0;
                    }
                    KeyCode::Char(c)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        state.input.push(c);
                        state.selection = 0;
                    }
                    _ => {}
                }
                if close {
                    self.overlay = Overlay::None;
                }
                if let Some(action) = action {
                    self.execute(action);
                }
            }
            Overlay::Omnibox(input) => {
                let mut close = false;
                let mut submit = None;
                match key.code {
                    KeyCode::Esc => close = true,
                    KeyCode::Enter => {
                        submit = Some((input.purpose, input.value.trim().to_string()));
                        close = true;
                    }
                    KeyCode::Backspace => {
                        input.value.pop();
                    }
                    KeyCode::Char(c)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        input.value.push(c);
                    }
                    _ => {}
                }
                if close {
                    self.overlay = Overlay::None;
                }
                if let Some((purpose, value)) = submit {
                    self.submit_input(purpose, value);
                }
            }
            Overlay::LinkPicker(state) => {
                let mut close = false;
                let mut chosen: Option<(String, bool)> = None;
                match key.code {
                    KeyCode::Esc => close = true,
                    KeyCode::Enter | KeyCode::Char('O') => {
                        if let Some(article) = &self.reader.article {
                            let links = filter_links(article, &state.input);
                            if let Some((url, _)) = links.get(state.selection) {
                                chosen = Some((url.clone(), key.code == KeyCode::Char('O')));
                            }
                        }
                        close = true;
                    }
                    KeyCode::Down | KeyCode::Tab => {
                        if let Some(article) = &self.reader.article {
                            let len = filter_links(article, &state.input).len();
                            if state.selection + 1 < len {
                                state.selection += 1;
                            }
                        }
                    }
                    KeyCode::Up | KeyCode::BackTab => {
                        state.selection = state.selection.saturating_sub(1);
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                        state.selection = 0;
                    }
                    KeyCode::Char(c)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        state.input.push(c);
                        state.selection = 0;
                    }
                    _ => {}
                }
                if close {
                    self.overlay = Overlay::None;
                }
                if let Some((url, external)) = chosen {
                    self.follow_link(&url, external);
                }
            }
        }
    }

    fn on_feed_key(&mut self, key: KeyEvent) {
        if self.try_player_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.feed_mut().move_selection(0, 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.feed_mut().move_selection(0, -1);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.feed_mut().move_selection(-1, 0);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.feed_mut().move_selection(1, 0);
            }
            KeyCode::Char('g') => {
                if self.pending_g.take().is_some() {
                    self.feed_mut().selection = 0;
                    self.feed_mut().scroll_row = 0;
                } else {
                    self.pending_g = Some(Instant::now());
                }
            }
            KeyCode::Char('G') => {
                let state = self.feed_mut();
                state.selection = state.items().len().saturating_sub(1);
            }
            KeyCode::Enter => {
                let mode = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    PlaybackMode::Video
                } else {
                    PlaybackMode::Audio
                };
                self.play_selected(mode);
            }
            KeyCode::Char('p') => self.play_selected(PlaybackMode::Audio),
            KeyCode::Char('i') => self.show_details(),
            KeyCode::Char('s') => self.toggle_subscription(),
            KeyCode::Char('r') => self.refresh_current(),
            KeyCode::Char('c') => self.copy_current_url(),
            KeyCode::Char('L') => self.save_current(),
            KeyCode::Char('/') => {
                self.switch_tab(Tab::Search);
                self.search_focus = true;
            }
            KeyCode::PageDown => self.feed_mut().move_selection(0, 3),
            KeyCode::PageUp => self.feed_mut().move_selection(0, -3),
            _ => {}
        }
    }

    fn on_reader_key(&mut self, key: KeyEvent) {
        if self.try_player_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.reader.scroll = self.reader.scroll.saturating_add(1)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.reader.scroll = self.reader.scroll.saturating_sub(1)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.reader.scroll =
                    (self.reader.scroll + self.reader.viewport_h).min(self.reader.max_scroll());
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.reader.scroll = self.reader.scroll.saturating_sub(self.reader.viewport_h);
            }
            KeyCode::Char('d') | KeyCode::PageDown => {
                let half = (self.reader.viewport_h / 2).max(1);
                self.reader.scroll = (self.reader.scroll + half).min(self.reader.max_scroll());
            }
            KeyCode::Char('u') | KeyCode::PageUp => {
                let half = (self.reader.viewport_h / 2).max(1);
                self.reader.scroll = self.reader.scroll.saturating_sub(half);
            }
            KeyCode::Char('f') => {
                self.reader.scroll =
                    (self.reader.scroll + self.reader.viewport_h).min(self.reader.max_scroll());
            }
            KeyCode::Char('b') => {
                self.reader.scroll = self.reader.scroll.saturating_sub(self.reader.viewport_h);
            }
            KeyCode::Char('g') => {
                if self.pending_g.take().is_some() {
                    self.reader.scroll = 0;
                } else {
                    self.pending_g = Some(Instant::now());
                }
            }
            KeyCode::Char('G') => self.reader.scroll = self.reader.max_scroll(),
            KeyCode::Char('[') => self.jump_heading(false),
            KeyCode::Char(']') => self.jump_heading(true),
            KeyCode::Char('t') => self.reader.toc_open = !self.reader.toc_open,
            KeyCode::Char('o') => {
                self.overlay = Overlay::LinkPicker(LinkPickerState::default());
            }
            KeyCode::Char('O') => {
                if let Some(article) = &self.reader.article {
                    open_external(&article.final_url);
                }
            }
            KeyCode::Char('w') => {
                self.reader_measure = if self.reader_measure >= 100 { 72 } else { 110 };
                self.reader.cache = None;
            }
            KeyCode::Char('r') => self.reload_article(),
            KeyCode::Char('L') => self.save_current(),
            KeyCode::Backspace => self.reader_back(),
            KeyCode::Char(' ') if self.player.current.is_none() => {
                self.reader.scroll =
                    (self.reader.scroll + self.reader.viewport_h).min(self.reader.max_scroll());
            }
            _ => {}
        }
    }

    fn on_library_key(&mut self, key: KeyEvent) {
        if self.try_player_key(&key) {
            return;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.library.selection + 1 < self.library.items.len() {
                    self.library.selection += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.library.selection = self.library.selection.saturating_sub(1);
            }
            KeyCode::Char('g') => self.library.selection = 0,
            KeyCode::Char('G') => {
                self.library.selection = self.library.items.len().saturating_sub(1)
            }
            KeyCode::Enter => self.open_library_item(),
            KeyCode::Char('d') | KeyCode::Char('L') => {
                if let Some(item) = self.library.items.get(self.library.selection).cloned() {
                    self.db.read_later_remove(&item.id);
                    self.library.items = self.db.read_later();
                    self.library.selection = self
                        .library
                        .selection
                        .min(self.library.items.len().saturating_sub(1));
                    self.toast = Some(Toast::info("removed from library"));
                }
            }
            KeyCode::Char('r') => self.load_library(),
            _ => {}
        }
    }

    fn try_player_key(&mut self, key: &KeyEvent) -> bool {
        if self.player.current.is_none() {
            return false;
        }
        match key.code {
            KeyCode::Char(' ') => {
                self.player_handle.send(PlayerCommand::TogglePause);
                true
            }
            KeyCode::Char(',') => {
                self.player_handle.send(PlayerCommand::SeekRelative(-5.0));
                true
            }
            KeyCode::Char('.') => {
                self.player_handle.send(PlayerCommand::SeekRelative(5.0));
                true
            }
            KeyCode::Char('<') => {
                self.player_handle.send(PlayerCommand::SeekRelative(-30.0));
                true
            }
            KeyCode::Char('>') => {
                self.player_handle.send(PlayerCommand::SeekRelative(30.0));
                true
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.player_handle.send(PlayerCommand::AddVolume(5.0));
                true
            }
            KeyCode::Char('-') => {
                self.player_handle.send(PlayerCommand::AddVolume(-5.0));
                true
            }
            KeyCode::Char('m') => {
                self.player.muted = !self.player.muted;
                self.player_handle
                    .send(PlayerCommand::SetMuted(self.player.muted));
                true
            }
            KeyCode::Char('v') => {
                self.toggle_video_mode();
                true
            }
            KeyCode::Char('x') => {
                self.stop_playback();
                true
            }
            KeyCode::Char('n') => {
                self.next_in_queue();
                true
            }
            _ => false,
        }
    }

    // ------------------------------------------------------------------
    // actions
    // ------------------------------------------------------------------

    fn current_tab(&self) -> Tab {
        match self.screen {
            Screen::Feeds(tab) => tab,
            Screen::Reader => self.tab,
        }
    }

    fn feed_mut(&mut self) -> &mut FeedState {
        match self.screen {
            Screen::Feeds(Tab::Search) => &mut self.search,
            Screen::Feeds(Tab::Subscriptions) => &mut self.subs,
            _ => &mut self.home,
        }
    }

    fn feed_ref(&self) -> &FeedState {
        match self.screen {
            Screen::Feeds(Tab::Search) => &self.search,
            Screen::Feeds(Tab::Subscriptions) => &self.subs,
            _ => &self.home,
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.screen = Screen::Feeds(tab);
        match tab {
            Tab::Search => {
                self.search_focus = true;
            }
            Tab::Subscriptions => {
                if self.subs.page.is_none() && !self.subs.loading {
                    self.request_feed(FeedTarget::Subscriptions, FeedKey::Subscriptions, false);
                }
            }
            Tab::Library => self.load_library(),
            Tab::Home => {
                if self.home.page.is_none() && !self.home.loading {
                    self.request_feed(FeedTarget::Home, FeedKey::Trending, false);
                }
            }
        }
    }

    fn open_omnibox(&mut self) {
        self.overlay = Overlay::Omnibox(InputState {
            prompt: "Open a URL or search YouTube".to_string(),
            value: String::new(),
            purpose: InputPurpose::Omnibox,
        });
    }

    fn open_prompt(&mut self, purpose: InputPurpose, prompt: &str) {
        self.overlay = Overlay::Omnibox(InputState {
            prompt: prompt.to_string(),
            value: String::new(),
            purpose,
        });
    }

    fn submit_input(&mut self, purpose: InputPurpose, value: String) {
        if value.is_empty() {
            return;
        }
        match purpose {
            InputPurpose::Omnibox => self.open_input(value),
            InputPurpose::ImportCookies => self.import_cookies(&value),
            InputPurpose::ImportSubscriptions => self.import_subscriptions(&value),
        }
    }

    fn open_input(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if let Ok(url) = Url::parse(&text)
            && matches!(url.scheme(), "http" | "https")
        {
            self.route_url(url);
            return;
        }
        if text.contains('.')
            && !text.contains(' ')
            && let Ok(url) = Url::parse(&format!("https://{text}"))
        {
            self.route_url(url);
            return;
        }
        self.search_query = text;
        self.tab = Tab::Search;
        self.screen = Screen::Feeds(Tab::Search);
        self.search_focus = false;
        self.run_search(false);
    }

    fn route_url(&mut self, url: Url) {
        if crate::adapters::is_youtube_url(&url) {
            self.open_youtube_url(url);
        } else {
            self.open_article(url.to_string(), false);
        }
    }

    fn open_youtube_url(&mut self, url: Url) {
        if let Some(playlist) = youtube::extract_playlist_id(&url) {
            self.tab = Tab::Home;
            self.screen = Screen::Feeds(Tab::Home);
            self.request_feed(
                FeedTarget::Home,
                FeedKey::Playlist {
                    id: playlist,
                    limit: self.cfg.youtube.limit,
                },
                false,
            );
        } else if let Some(channel) = youtube::extract_channel_ref(&url) {
            self.tab = Tab::Home;
            self.screen = Screen::Feeds(Tab::Home);
            self.request_feed(
                FeedTarget::Home,
                FeedKey::Channel {
                    id: channel,
                    tab: ChannelTab::Videos,
                    limit: self.cfg.youtube.limit,
                },
                false,
            );
        } else if let Some(id) = youtube::extract_video_id(&url) {
            let video = youtube::video_stub(url.as_str(), &id);
            self.player.queue.clear();
            self.start_playback(video, PlaybackMode::Audio, None);
        } else {
            self.toast = Some(Toast::warning("unsupported YouTube URL"));
        }
    }

    fn request_feed(&mut self, target: FeedTarget, key: FeedKey, force: bool) {
        let cache_key = feed_cache_key(&self.cfg, target, &key);
        let ttl = feed_ttl(&key);

        if let Some(mut page) = self.db.cached_feed(&cache_key) {
            let fresh = now_unix() - page.fetched_at < ttl;
            if self.offline || (!force && fresh) {
                page.source = FeedSource::Cache;
                page.stale = !fresh;
                self.apply_feed(target, page);
                return;
            }
        }

        if self.offline {
            if let Some(mut page) = self.db.cached_feed(&cache_key) {
                page.stale = true;
                page.source = FeedSource::Cache;
                page.warnings
                    .push("offline: showing cached results".to_string());
                self.apply_feed(target, page);
            } else {
                self.state_mut(target)
                    .fail("offline and no cached copy is available".to_string());
            }
            return;
        }

        self.job += 1;
        let job = self.job;
        self.state_mut(target).begin_load(job);
        self.pending_feed_cache.insert(job, cache_key);

        let tx = self.tx.clone();
        let cfg = self.cfg.clone();
        let subs = if target == FeedTarget::Subscriptions {
            self.db.subscriptions()
        } else {
            Vec::new()
        };
        let key_for_task = key.clone();

        tokio::spawn(async move {
            let result = match target {
                FeedTarget::Home => match &key_for_task {
                    FeedKey::Trending | FeedKey::Recommended => youtube::load_home(&cfg).await,
                    other => youtube::load_feed(&cfg, other).await,
                },
                FeedTarget::Subscriptions => youtube::load_subscriptions(&cfg, &subs).await,
                _ => youtube::load_feed(&cfg, &key_for_task).await,
            };
            match result {
                Ok(page) => {
                    let _ = tx.send(AppEvent::FeedLoaded { job, target, page }).await;
                }
                Err(error) => {
                    let _ = tx
                        .send(AppEvent::Failed {
                            job,
                            message: error.to_string(),
                        })
                        .await;
                }
            }
        });
    }

    fn state_mut(&mut self, target: FeedTarget) -> &mut FeedState {
        match target {
            FeedTarget::Home => &mut self.home,
            FeedTarget::Search => &mut self.search,
            FeedTarget::Subscriptions => &mut self.subs,
            FeedTarget::Library => &mut self.home,
        }
    }

    fn apply_feed(&mut self, target: FeedTarget, page: crate::model::FeedPage) {
        let key = feed_cache_key(&self.cfg, target, &page.key);
        self.db.cache_feed(&key, &page);
        let warnings = page.warnings.clone();
        let stale = page.stale;
        self.state_mut(target).apply(page);
        if let Some(warning) = warnings.first() {
            self.toast = Some(Toast::warning(warning.clone()));
        } else if stale {
            self.toast = Some(Toast::warning("showing cached results"));
        }
    }

    fn run_search(&mut self, force: bool) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.request_feed(
            FeedTarget::Search,
            FeedKey::Search {
                query,
                limit: self.cfg.youtube.limit,
            },
            force,
        );
    }

    fn refresh_current(&mut self) {
        match self.screen {
            Screen::Feeds(Tab::Home) => {
                let key = self
                    .home
                    .page
                    .as_ref()
                    .map(|page| page.key.clone())
                    .unwrap_or(FeedKey::Trending);
                self.request_feed(FeedTarget::Home, key, true)
            }
            Screen::Feeds(Tab::Search) => self.run_search(true),
            Screen::Feeds(Tab::Subscriptions) => {
                self.request_feed(FeedTarget::Subscriptions, FeedKey::Subscriptions, true)
            }
            Screen::Feeds(Tab::Library) => self.load_library(),
            Screen::Reader => self.reload_article(),
        }
    }

    fn reload_article(&mut self) {
        let url = self
            .reader
            .article
            .as_ref()
            .map(|article| article.final_url.clone())
            .unwrap_or_else(|| self.reader.url.clone());
        if !url.is_empty() {
            self.open_article(url, false);
        }
    }

    fn open_article(&mut self, url: String, from_reader: bool) {
        if from_reader {
            if let Some(article) = self.reader.article.clone() {
                let previous = ReaderState {
                    article: Some(article),
                    url: self.reader.url.clone(),
                    scroll: self.reader.scroll,
                    ..ReaderState::default()
                };
                self.reader_stack.push(previous);
            }
        } else {
            self.reader_stack.clear();
        }

        self.tab = match self.screen {
            Screen::Feeds(tab) => tab,
            Screen::Reader => self.tab,
        };
        self.screen = Screen::Reader;

        let cache_key = format!("article:v1:{}", youtube::short_hash(&url));
        if let Some((fetched_at, payload)) = self.db.cache_get(&cache_key)
            && let Ok(article) = serde_json::from_str::<Article>(&payload)
        {
            let fresh = now_unix() - fetched_at < 86_400;
            if self.offline || fresh {
                let mut article = article;
                article.source = ExtractSource::Cache;
                self.reader.pending_progress = self.db.article_position(&article.final_url);
                self.reader.apply(article);
                return;
            }
        }

        if self.offline {
            self.reader
                .fail("offline and no cached copy is available".to_string());
            return;
        }

        self.job += 1;
        let job = self.job;
        self.reader.begin_load(job, url.clone());
        self.pending_article = Some((job, url.clone(), cache_key));

        let tx = self.tx.clone();
        let cfg = self.cfg.clone();
        let paths = self.paths.clone();
        tokio::spawn(async move {
            let result = match Url::parse(&url) {
                Ok(parsed) => article::load(&cfg, &paths, &parsed).await,
                Err(error) => Err(AppError::Other(format!("invalid URL: {error}"))),
            };
            match result {
                Ok(article) => {
                    let _ = tx
                        .send(AppEvent::ArticleLoaded {
                            job,
                            article: Box::new(article),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = tx
                        .send(AppEvent::Failed {
                            job,
                            message: error.to_string(),
                        })
                        .await;
                }
            }
        });
    }

    fn follow_link(&mut self, url: &str, external: bool) {
        if external {
            open_external(url);
            return;
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            open_external(url);
            return;
        }
        let anchor_block = self.reader.article.as_ref().and_then(|article| {
            let (base, fragment) = url.split_once('#')?;
            if base != article.final_url && base != article.requested_url {
                return None;
            }
            article.anchors.get(fragment).copied()
        });
        if let Some(block) = anchor_block
            && let Some(line) = self
                .reader
                .cache
                .as_ref()
                .and_then(|cache| cache.block_starts.get(block))
                .copied()
        {
            self.reader.scroll = line;
            return;
        }
        if let Some(article) = &self.reader.article
            && url == article.final_url
        {
            return;
        }
        self.open_article(url.to_string(), true);
    }

    fn reader_back(&mut self) {
        if let Some(article) = &self.reader.article {
            self.db
                .set_article_position(&article.final_url, self.reader.progress());
        }
        if let Some(mut previous) = self.reader_stack.pop() {
            previous.cache = None;
            self.reader = previous;
            self.screen = Screen::Reader;
        } else {
            self.screen = Screen::Feeds(self.tab);
        }
    }

    fn load_library(&mut self) {
        self.library.items = self.db.read_later();
        self.library.selection = self
            .library
            .selection
            .min(self.library.items.len().saturating_sub(1));
    }

    fn open_library_item(&mut self) {
        let Some(item) = self.library.items.get(self.library.selection).cloned() else {
            return;
        };
        if item.kind == "video"
            && let Ok(url) = Url::parse(&item.url)
            && let Some(id) = youtube::extract_video_id(&url)
        {
            let video = youtube::video_stub(&item.url, &id);
            self.player.queue.clear();
            self.start_playback(video, PlaybackMode::Audio, None);
            return;
        }
        self.open_article(item.url, false);
    }

    fn play_selected(&mut self, mode: PlaybackMode) {
        let (video, rest) = {
            let state = self.feed_ref();
            (
                state.selected().cloned(),
                state
                    .items()
                    .iter()
                    .skip(state.selection + 1)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        let Some(video) = video else {
            self.toast = Some(Toast::warning("nothing selected"));
            return;
        };
        self.player.queue = rest;
        self.start_playback(video, mode, None);
    }

    fn start_playback(&mut self, video: Video, mode: PlaybackMode, start: Option<f64>) {
        self.player.current = Some(video.clone());
        self.player.mode = mode;
        self.player.phase = PlayerPhase::Loading;
        self.player.position = start.unwrap_or(0.0);
        self.player.duration = video.duration_secs.map(|value| value as f64);
        self.player.paused = false;
        self.player.buffering = false;
        self.player.error = None;
        self.player.updated_at = Instant::now();
        self.player_handle.send(PlayerCommand::Play {
            video,
            mode,
            start,
            volume: Some(self.player.volume),
            paused: None,
        });
    }

    fn toggle_video_mode(&mut self) {
        let Some(video) = self.player.current.clone() else {
            return;
        };
        let mode = match self.player.mode {
            PlaybackMode::Audio => PlaybackMode::Video,
            PlaybackMode::Video => PlaybackMode::Audio,
        };
        let position = Some(self.player.display_position());
        self.player.mode = mode;
        self.player.phase = PlayerPhase::Loading;
        self.player.updated_at = Instant::now();
        self.player_handle.send(PlayerCommand::Play {
            video,
            mode,
            start: position,
            volume: Some(self.player.volume),
            paused: Some(self.player.paused),
        });
        self.toast = Some(Toast::info(match mode {
            PlaybackMode::Audio => "switched to audio",
            PlaybackMode::Video => "switched to video window",
        }));
    }

    fn stop_playback(&mut self) {
        self.player_handle.send(PlayerCommand::Stop);
        self.player.phase = PlayerPhase::Idle;
        self.player.current = None;
        self.player.queue.clear();
        self.player.buffering = false;
    }

    fn next_in_queue(&mut self) {
        if self.player.queue.is_empty() {
            self.toast = Some(Toast::info("queue is empty"));
            return;
        }
        let next = self.player.queue.remove(0);
        let mode = self.player.mode;
        self.start_playback(next, mode, None);
    }

    fn toggle_subscription(&mut self) {
        let Some(video) = self.feed_ref().selected().cloned() else {
            return;
        };
        let Some(channel_id) = video.channel.id.clone().filter(|id| id.starts_with("UC")) else {
            self.toast = Some(Toast::warning(
                "this video has no channel id; open the channel page instead",
            ));
            return;
        };
        if self.db.is_subscribed(&channel_id) {
            self.db.unsubscribe(&channel_id);
            self.toast = Some(Toast::info(format!(
                "unsubscribed from {}",
                video.channel.name
            )));
        } else {
            self.db.subscribe(
                &channel_id,
                &video.channel.name,
                video.channel.handle.as_deref(),
            );
            self.toast = Some(Toast::success(format!(
                "subscribed to {}",
                video.channel.name
            )));
            self.subs.page = None;
        }
    }

    fn save_current(&mut self) {
        match self.screen {
            Screen::Feeds(_) => {
                let Some(video) = self.feed_ref().selected().cloned() else {
                    return;
                };
                if self.db.read_later_contains(&video.id) {
                    self.db.read_later_remove(&video.id);
                    self.toast = Some(Toast::info("removed from library"));
                } else {
                    self.db.read_later_add(&SavedItem {
                        id: video.id.clone(),
                        kind: "video".to_string(),
                        title: video.title.clone(),
                        url: video.watch_url(),
                        added_at: now_unix(),
                    });
                    self.toast = Some(Toast::success("saved to library"));
                }
            }
            Screen::Reader => {
                let Some(article) = &self.reader.article else {
                    return;
                };
                let id = format!("article:{}", youtube::short_hash(&article.final_url));
                if self.db.read_later_contains(&id) {
                    self.db.read_later_remove(&id);
                    self.toast = Some(Toast::info("removed from library"));
                } else {
                    self.db.read_later_add(&SavedItem {
                        id,
                        kind: "article".to_string(),
                        title: article.title.clone(),
                        url: article.final_url.clone(),
                        added_at: now_unix(),
                    });
                    self.toast = Some(Toast::success("saved to library"));
                }
            }
        }
    }

    fn copy_current_url(&mut self) {
        let url = match self.screen {
            Screen::Feeds(_) => self.feed_ref().selected().map(Video::watch_url),
            Screen::Reader => self
                .reader
                .article
                .as_ref()
                .map(|article| article.final_url.clone()),
        };
        if let Some(url) = url {
            match copy_to_clipboard(&url) {
                Ok(()) => self.toast = Some(Toast::success("copied URL")),
                Err(error) => self.toast = Some(Toast::error(format!("copy failed: {error}"))),
            }
        }
    }

    fn show_details(&mut self) {
        let Some(video) = self.feed_ref().selected().cloned() else {
            return;
        };
        let mut parts = vec![video.channel.name.clone()];
        if let Some(date) = video.date_label() {
            parts.push(date);
        }
        if let Some(views) = video.view_count {
            parts.push(format!("{} views", crate::util::time::format_count(views)));
        }
        if let Some(duration) = video.duration_label() {
            parts.push(duration);
        }
        self.toast = Some(Toast::info(format!(
            "{}  —  {}",
            video.title,
            parts.join("  ·  ")
        )));
    }

    fn jump_heading(&mut self, forward: bool) {
        let Some(article) = &self.reader.article else {
            return;
        };
        let Some(cache) = &self.reader.cache else {
            return;
        };
        let starts: Vec<usize> = article
            .toc
            .iter()
            .filter_map(|entry| cache.block_starts.get(entry.block_index).copied())
            .collect();
        let target = if forward {
            starts
                .iter()
                .copied()
                .find(|start| *start > self.reader.scroll + 1)
        } else {
            starts
                .iter()
                .copied()
                .rev()
                .find(|start| *start < self.reader.scroll)
        };
        if let Some(target) = target {
            self.reader.scroll = target;
        }
    }

    fn import_cookies(&mut self, path: &str) {
        let file = Path::new(path);
        let store = article::cookies::load(&self.paths);
        match article::cookies::import_netscape(file, &store) {
            Ok(count) => {
                article::cookies::save(&store, &self.paths);
                self.cfg.cookies_txt = Some(file.to_path_buf());
                self.toast = Some(Toast::success(format!("imported {count} cookies")));
            }
            Err(error) => {
                self.toast = Some(Toast::error(error.to_string()));
            }
        }
    }

    fn import_subscriptions(&mut self, path: &str) {
        match import_subscriptions_file(&self.db, Path::new(path)) {
            Ok(count) => {
                self.subs.page = None;
                self.toast = Some(Toast::success(format!("imported {count} subscriptions")));
            }
            Err(error) => {
                self.toast = Some(Toast::error(format!("import failed: {error}")));
            }
        }
    }

    fn execute(&mut self, action: Action) {
        match action {
            Action::OpenUrl => self.open_omnibox(),
            Action::Search => {
                self.switch_tab(Tab::Search);
                self.search_focus = true;
            }
            Action::Home => self.switch_tab(Tab::Home),
            Action::Subscriptions => self.switch_tab(Tab::Subscriptions),
            Action::Library => self.switch_tab(Tab::Library),
            Action::Refresh => self.refresh_current(),
            Action::ImportCookies => {
                self.open_prompt(InputPurpose::ImportCookies, "Path to cookies.txt")
            }
            Action::ImportSubscriptions => self.open_prompt(
                InputPurpose::ImportSubscriptions,
                "Path to subscriptions file",
            ),
            Action::ToggleTheme => {
                self.theme_choice = match self.theme_choice {
                    ThemeChoice::Dark => ThemeChoice::Light,
                    _ => ThemeChoice::Dark,
                };
                self.cfg.theme = self.theme_choice;
                self.theme = Theme::detect(self.theme_choice, self.cfg.ui.animations);
                self.reader.cache = None;
                self.toast = Some(Toast::info(format!("theme: {}", self.theme.name)));
            }
            Action::ToggleImages => {
                self.cfg.reader.images = !self.cfg.reader.images;
                self.reader.cache = None;
                self.toast = Some(Toast::info(if self.cfg.reader.images {
                    "images enabled"
                } else {
                    "images disabled"
                }));
            }
            Action::ToggleVideo => self.toggle_video_mode(),
            Action::StopPlayback => self.stop_playback(),
            Action::ClearCache => {
                self.db.clear_cache();
                self.images.clear();
                self.pending_images.clear();
                self.reader.cache = None;
                self.toast = Some(Toast::info("cache cleared"));
            }
            Action::Help => {
                self.overlay = Overlay::Help { scroll: 0 };
            }
            Action::Quit => self.quit = true,
        }
    }

    // ------------------------------------------------------------------
    // async events and ticks
    // ------------------------------------------------------------------

    fn on_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::FeedLoaded { job, target, page } => {
                self.pending_feed_cache.remove(&job);
                if self.state_mut(target).job != job {
                    return;
                }
                self.apply_feed(target, page);
            }
            AppEvent::ArticleLoaded { job, article } => {
                if self.reader.job != job {
                    return;
                }
                if let Some((_, _, cache_key)) = self.pending_article.take()
                    && let Ok(payload) = serde_json::to_string(&*article)
                {
                    self.db.cache_put(&cache_key, now_unix(), &payload);
                }
                self.reader.pending_progress = self.db.article_position(&article.final_url);
                self.reader.apply(*article);
            }
            AppEvent::Progress { job: _, stage } => {
                tracing::debug!("job progress: {stage}");
            }
            AppEvent::Failed { job, message } => self.on_failed(job, message),
            AppEvent::ImageReady { key, image } => {
                self.pending_images.remove(&key);
                if key.starts_with("art:") {
                    self.reader.cache = None;
                }
                self.images.insert(key, image);
            }
            AppEvent::ImageFailed { key } => {
                self.pending_images.remove(&key);
            }
            AppEvent::Player(event) => self.on_player_event(event),
        }
    }

    fn on_failed(&mut self, job: u64, message: String) {
        if let Some(cache_key) = self.pending_feed_cache.remove(&job) {
            for target in [
                FeedTarget::Home,
                FeedTarget::Search,
                FeedTarget::Subscriptions,
            ] {
                if self.state_mut(target).job != job {
                    continue;
                }
                if let Some(mut page) = self.db.cached_feed(&cache_key) {
                    page.stale = true;
                    page.source = FeedSource::Cache;
                    page.warnings.push(message.clone());
                    self.apply_feed(target, page);
                } else {
                    self.state_mut(target).fail(message.clone());
                    self.toast = Some(Toast::error(message));
                }
                return;
            }
        }

        if let Some((pending_job, _, cache_key)) = self.pending_article.take()
            && pending_job == job
        {
            if let Some((_, payload)) = self.db.cache_get(&cache_key)
                && let Ok(article) = serde_json::from_str::<Article>(&payload)
            {
                let mut article = article;
                article.source = ExtractSource::Cache;
                self.reader.pending_progress = self.db.article_position(&article.final_url);
                self.reader.apply(article);
                self.toast = Some(Toast::warning(message));
                return;
            }
            self.reader.fail(message.clone());
            self.toast = Some(Toast::error(message));
        }
    }

    fn on_player_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::Started => {
                self.player.phase = PlayerPhase::Loading;
            }
            PlayerEvent::FileLoaded | PlayerEvent::PlaybackRestart => {
                self.player.phase = if self.player.paused {
                    PlayerPhase::Paused
                } else {
                    PlayerPhase::Playing
                };
                self.player.updated_at = Instant::now();
            }
            PlayerEvent::TimePos(position) => {
                self.player.position = position;
                self.player.updated_at = Instant::now();
            }
            PlayerEvent::Duration(duration) => {
                self.player.duration = Some(duration);
            }
            PlayerEvent::Pause(paused) => {
                self.player.paused = paused;
                self.player.phase = if paused {
                    PlayerPhase::Paused
                } else {
                    PlayerPhase::Playing
                };
                self.player.updated_at = Instant::now();
            }
            PlayerEvent::Buffering(buffering) => {
                self.player.buffering = buffering;
                self.player.phase = if buffering {
                    PlayerPhase::Buffering
                } else if self.player.paused {
                    PlayerPhase::Paused
                } else {
                    PlayerPhase::Playing
                };
            }
            PlayerEvent::Volume(volume) => self.player.volume = volume,
            PlayerEvent::Speed(speed) => self.player.speed = speed,
            PlayerEvent::Muted(muted) => self.player.muted = muted,
            PlayerEvent::Ended => {
                self.player.phase = PlayerPhase::Ended;
                if !self.player.queue.is_empty() {
                    let next = self.player.queue.remove(0);
                    let mode = self.player.mode;
                    self.start_playback(next, mode, None);
                }
            }
            PlayerEvent::Error(message) => {
                self.player.phase = PlayerPhase::Failed;
                self.player.error = Some(message.clone());
                self.toast = Some(Toast::error(message));
            }
            PlayerEvent::Exited => {
                self.player.phase = PlayerPhase::Idle;
                self.player.current = None;
                self.player.queue.clear();
            }
        }
    }

    fn on_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);

        if let Some(toast) = &self.toast
            && toast.expired()
        {
            self.toast = None;
        }

        if let Some(pending) = self.pending_g
            && pending.elapsed() > Duration::from_millis(900)
        {
            self.pending_g = None;
        }

        if self.last_history_save.elapsed() > Duration::from_secs(5) {
            if let Some(video) = &self.player.current
                && matches!(self.player.phase, PlayerPhase::Playing)
            {
                self.db
                    .history_upsert(video, self.player.position, self.player.duration);
            }
            self.last_history_save = Instant::now();
        }

        if self.last_progress_save.elapsed() > Duration::from_secs(2) {
            if let (Some(article), true) =
                (&self.reader.article, matches!(self.screen, Screen::Reader))
            {
                self.db
                    .set_article_position(&article.final_url, self.reader.progress());
            }
            self.last_progress_save = Instant::now();
        }
    }
}

fn feed_cache_key(cfg: &Config, target: FeedTarget, key: &FeedKey) -> String {
    match target {
        FeedTarget::Home => match key {
            FeedKey::Trending | FeedKey::Recommended => {
                let fingerprint = cfg
                    .cookies_from_browser
                    .as_deref()
                    .map(youtube::short_hash)
                    .unwrap_or_else(|| "anon".to_string());
                format!("feed:v1:home:{fingerprint}")
            }
            _ => yt_cache_key(cfg, key),
        },
        FeedTarget::Subscriptions => "feed:v1:subscriptions".to_string(),
        _ => yt_cache_key(cfg, key),
    }
}

fn feed_ttl(key: &FeedKey) -> i64 {
    match key {
        FeedKey::Search { .. } => 300,
        FeedKey::Channel { .. } | FeedKey::Playlist { .. } => 1800,
        FeedKey::Subscriptions => 600,
        _ => 900,
    }
}

fn next_tab(tab: Tab) -> Tab {
    match tab {
        Tab::Home => Tab::Search,
        Tab::Search => Tab::Subscriptions,
        Tab::Subscriptions => Tab::Library,
        Tab::Library => Tab::Home,
    }
}

fn prev_tab(tab: Tab) -> Tab {
    match tab {
        Tab::Home => Tab::Library,
        Tab::Search => Tab::Home,
        Tab::Subscriptions => Tab::Search,
        Tab::Library => Tab::Subscriptions,
    }
}

fn draw_input_overlay(frame: &mut Frame<'_>, area: Rect, theme: &Theme, prompt: &str, value: &str) {
    let width = area.width.saturating_sub(8).clamp(30, 72);
    let rect = ui::centered(area, width, 3);
    ui::clear_overlay(frame, rect);
    let block = ui::focused_panel(theme);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{prompt}  › "), Style::new().fg(theme.accent)),
            Span::styled(value.to_string(), Style::new().fg(theme.text)),
            Span::styled("▏", Style::new().fg(theme.accent)),
        ]))
        .style(Style::new().bg(theme.surface)),
        inner,
    );
}

fn filter_links(article: &Article, input: &str) -> Vec<(String, String)> {
    let mut items: Vec<(i32, String, String)> = article
        .links()
        .into_iter()
        .filter_map(|(url, text)| {
            let haystack = format!("{text} {url}");
            fuzzy_match(input, &haystack).map(|(score, _)| (score, url, text))
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.0));
    items
        .into_iter()
        .map(|(_, url, text)| (url, text))
        .collect()
}

fn truncate_link(text: &str) -> String {
    crate::util::text::truncate_to_width(text, 62)
}

fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use crossterm::clipboard::CopyToClipboard;
    execute!(std::io::stdout(), CopyToClipboard::to_clipboard_from(text))
}

fn open_external(url: &str) {
    let program = std::env::var("BROWSER").unwrap_or_else(|_| "xdg-open".to_string());
    let _ = std::process::Command::new(program).arg(url).spawn();
}

pub fn import_subscriptions_file(db: &Db, path: &Path) -> anyhow::Result<usize> {
    let text = std::fs::read_to_string(path)?;
    let mut count = 0usize;

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(items) = value.as_array()
    {
        for item in items {
            let id = item
                .get("id")
                .or_else(|| item.get("channel_id"))
                .or_else(|| item.get("channelId"))
                .and_then(|value| value.as_str());
            let title = item
                .get("title")
                .or_else(|| item.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let handle = item.get("handle").and_then(|value| value.as_str());
            if let Some(id) = id {
                db.subscribe(id, if title.is_empty() { id } else { title }, handle);
                count += 1;
            }
        }
        return Ok(count);
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split([',', '\t', ';']).collect();
        let first = fields.first().copied().unwrap_or("").trim();
        let second = fields.get(1).copied().unwrap_or("").trim();

        let id = if first.starts_with("http") {
            Url::parse(first)
                .ok()
                .and_then(|url| youtube::extract_channel_ref(&url))
        } else if first.starts_with('@') || first.starts_with("UC") {
            Some(first.to_string())
        } else {
            None
        };

        if let Some(id) = id {
            let title = if second.is_empty() {
                id.clone()
            } else {
                second.to_string()
            };
            db.subscribe(&id, &title, Some(&id));
            count += 1;
        }
    }
    Ok(count)
}
