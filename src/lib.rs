pub mod adapters;
pub mod app;
pub mod cli;
pub mod config;
pub mod db;
pub mod error;
pub mod event;
pub mod images;
pub mod media;
pub mod model;
pub mod render;
pub mod tools;
pub mod ui;
pub mod util;

use tokio::sync::mpsc;

use crate::cli::Cli;
use crate::config::Config;
use crate::db::Db;
use crate::tools::Tools;
use crate::util::paths::Paths;

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    let paths = Paths::new();
    paths.ensure()?;
    init_logging(&paths, cli.debug)?;
    let cfg = Config::load(&paths, &cli)?;
    tracing::info!("starting dontbrowse");

    match crate::adapters::article::cookies::import_configured(&cfg, &paths) {
        Ok(0) => {}
        Ok(count) => tracing::info!("imported {count} cookies"),
        Err(error) => tracing::warn!("cookie import failed: {error}"),
    }

    let tools = Tools::detect(&cfg).await;
    let db = Db::open(&paths.db_file())?;

    if let Some(file) = &cli.import_subscriptions {
        let count = app::import_subscriptions_file(&db, file)?;
        println!("imported {count} subscriptions");
        return Ok(());
    }

    let (tx, rx) = mpsc::channel(512);
    let mut app = app::App::new(cli, cfg, paths, tools, db, tx, rx);

    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::DisableBracketedPaste,
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        default_hook(info);
    }));

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        std::io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste,
        crossterm::event::DisableMouseCapture
    )?;

    let backend = crate::ui::backend::LinkBackend::new(app.links.clone());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = app.run(&mut terminal).await;

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableBracketedPaste,
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::cursor::Show
    );
    let _ = crossterm::terminal::disable_raw_mode();
    result
}

fn init_logging(paths: &Paths, debug: bool) -> anyhow::Result<()> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.log_file())?;
    let level = if debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    tracing_subscriber::fmt()
        .with_writer(file)
        .with_ansi(false)
        .with_max_level(level)
        .init();
    Ok(())
}
