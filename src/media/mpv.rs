use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::config::Config;
use crate::event::AppEvent;
use crate::media::ipc::{self, MpvEvent};
use crate::model::Video;
use crate::util::paths::Paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackMode {
    Audio,
    Video,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum PlayerCommand {
    Play {
        video: Video,
        mode: PlaybackMode,
        start: Option<f64>,
        volume: Option<f64>,
        paused: Option<bool>,
    },
    TogglePause,
    SeekRelative(f64),
    SeekAbsolute(f64),
    AddVolume(f64),
    SetMuted(bool),
    Stop,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    Started,
    FileLoaded,
    PlaybackRestart,
    TimePos(f64),
    Duration(f64),
    Pause(bool),
    Buffering(bool),
    Volume(f64),
    Speed(f64),
    Muted(bool),
    Ended,
    Error(String),
    Exited,
}

pub struct PlayerHandle {
    pub tx: mpsc::UnboundedSender<PlayerCommand>,
    pub available: bool,
}

impl PlayerHandle {
    pub fn send(&self, command: PlayerCommand) {
        let _ = self.tx.send(command);
    }
}

pub fn spawn(
    cfg: Config,
    paths: Paths,
    events: mpsc::Sender<AppEvent>,
    available: bool,
) -> PlayerHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(controller(cfg, paths, rx, events, available));
    PlayerHandle { tx, available }
}

async fn controller(
    cfg: Config,
    paths: Paths,
    mut commands: mpsc::UnboundedReceiver<PlayerCommand>,
    events: mpsc::Sender<AppEvent>,
    available: bool,
) {
    let (mpv_tx, mut mpv_rx) = mpsc::unbounded_channel::<(u64, PlayerEvent)>();
    let mut session: Option<Session> = None;
    let mut session_id: u64 = 0;
    let mut current_session: Option<u64> = None;

    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    PlayerCommand::Play { video, mode, start, volume, paused } => {
                        if !available {
                            let _ = events.send(AppEvent::Player(PlayerEvent::Error(
                                "mpv is not installed. Install mpv to enable playback.".to_string()
                            ))).await;
                            continue;
                        }

                        let needs_new = session.as_ref().map(|s| s.mode != mode).unwrap_or(true);
                        if needs_new {
                            if let Some(old) = session.take() {
                                old.shutdown().await;
                            }
                            session_id += 1;
                            let id = session_id;
                            match Session::start(&cfg, &paths, mode, id, mpv_tx.clone()).await {
                                Ok(new_session) => {
                                    current_session = Some(id);
                                    session = Some(new_session);
                                }
                                Err(error) => {
                                    current_session = None;
                                    let _ = events.send(AppEvent::Player(PlayerEvent::Error(error))).await;
                                    continue;
                                }
                            }
                        }

                        if let Some(active) = session.as_mut() {
                            active.send(ipc::loadfile(&video.watch_url())).await;
                            if let Some(start) = start {
                                active.send(ipc::seek_absolute(start)).await;
                            }
                            if let Some(volume) = volume {
                                active.send(ipc::set_property("volume", serde_json::json!(volume))).await;
                            }
                            if let Some(paused) = paused {
                                active.send(ipc::set_property("pause", serde_json::json!(paused))).await;
                            }
                            let _ = events.send(AppEvent::Player(PlayerEvent::Started)).await;
                        }
                    }
                    PlayerCommand::TogglePause => {
                        if let Some(active) = session.as_mut() {
                            active.send(ipc::cycle_pause()).await;
                        }
                    }
                    PlayerCommand::SeekRelative(seconds) => {
                        if let Some(active) = session.as_mut() {
                            active.send(ipc::seek_relative(seconds)).await;
                        }
                    }
                    PlayerCommand::SeekAbsolute(seconds) => {
                        if let Some(active) = session.as_mut() {
                            active.send(ipc::seek_absolute(seconds)).await;
                        }
                    }
                    PlayerCommand::AddVolume(delta) => {
                        if let Some(active) = session.as_mut() {
                            active.send(ipc::add_volume(delta)).await;
                        }
                    }
                    PlayerCommand::SetMuted(muted) => {
                        if let Some(active) = session.as_mut() {
                            active.send(ipc::set_property("mute", serde_json::json!(muted))).await;
                        }
                    }
                    PlayerCommand::Stop => {
                        if let Some(active) = session.take() {
                            active.shutdown().await;
                        }
                        current_session = None;
                        let _ = events.send(AppEvent::Player(PlayerEvent::Exited)).await;
                    }
                    PlayerCommand::Shutdown => {
                        if let Some(active) = session.take() {
                            active.shutdown().await;
                        }
                        break;
                    }
                }
            }
            Some((id, event)) = mpv_rx.recv() => {
                if Some(id) != current_session {
                    continue;
                }
                if matches!(event, PlayerEvent::Exited) {
                    session = None;
                    current_session = None;
                }
                if events.send(AppEvent::Player(event)).await.is_err() {
                    break;
                }
            }
        }
    }

    if let Some(active) = session.take() {
        active.shutdown().await;
    }
}

struct Session {
    child: Child,
    writer: tokio::net::unix::OwnedWriteHalf,
    mode: PlaybackMode,
    socket: PathBuf,
}

impl Session {
    async fn start(
        cfg: &Config,
        paths: &Paths,
        mode: PlaybackMode,
        id: u64,
        events: mpsc::UnboundedSender<(u64, PlayerEvent)>,
    ) -> Result<Self, String> {
        let socket = paths.mpv_socket();
        let _ = std::fs::remove_file(&socket);

        let log_path = paths.log_dir().join("mpv.log");
        let mut command = Command::new(&cfg.mpv_path);
        command.args(["--idle=yes", "--no-terminal", "--msg-level=all=warn"]);
        command.arg(format!("--input-ipc-server={}", socket.display()));
        command.arg(format!("--log-file={}", log_path.display()));
        command.arg("--ytdl=yes");
        match mode {
            PlaybackMode::Audio => {
                command.args([
                    "--no-video",
                    "--force-window=no",
                    "--ytdl-format=bestaudio/best",
                ]);
            }
            PlaybackMode::Video => {
                command.args([
                    "--force-window=yes",
                    "--ytdl-format=bestvideo*+bestaudio/best",
                ]);
            }
        }
        command.arg(format!(
            "--script-opts=ytdl_hook-ytdl_path={}",
            cfg.yt_dlp_path
        ));
        if let Some(browser) = &cfg.cookies_from_browser {
            command.arg(format!(
                "--ytdl-raw-options=cookies-from-browser={browser},"
            ));
        }
        if let Some(cookies) = &cfg.cookies_txt
            && let Some(path) = cookies.to_str()
        {
            command.arg(format!("--ytdl-raw-options=cookies={path},"));
        }
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.kill_on_drop(true);

        let child = command.spawn().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "mpv is not installed. Install mpv to enable playback.".to_string()
            } else {
                format!("could not start mpv: {error}")
            }
        })?;

        let mut stream = None;
        for _ in 0..60 {
            match UnixStream::connect(&socket).await {
                Ok(connected) => {
                    stream = Some(connected);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        let stream = stream
            .ok_or_else(|| "mpv did not open its control socket; check the mpv log".to_string())?;

        let (read_half, write_half) = stream.into_split();
        tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Some(event) = ipc::parse_event(&line)
                            && let Some(mapped) = map_event(event)
                            && events.send((id, mapped)).is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = events.send((id, PlayerEvent::Exited));
        });

        let mut session = Session {
            child,
            writer: write_half,
            mode,
            socket,
        };

        for (id, name) in [
            (1, "time-pos"),
            (2, "duration"),
            (3, "pause"),
            (4, "paused-for-cache"),
            (5, "volume"),
            (6, "speed"),
            (7, "mute"),
        ] {
            session.send(ipc::observe(id, name)).await;
        }

        Ok(session)
    }

    async fn send(&mut self, message: serde_json::Value) {
        let mut line = serde_json::to_string(&message).unwrap_or_default();
        line.push('\n');
        if self.writer.write_all(line.as_bytes()).await.is_ok() {
            let _ = self.writer.flush().await;
        }
    }

    async fn shutdown(mut self) {
        self.send(ipc::quit()).await;
        let _ = tokio::time::timeout(Duration::from_millis(500), self.child.wait()).await;
        let _ = self.child.kill().await;
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn map_event(event: MpvEvent) -> Option<PlayerEvent> {
    match event {
        MpvEvent::FileLoaded => Some(PlayerEvent::FileLoaded),
        MpvEvent::PlaybackRestart => Some(PlayerEvent::PlaybackRestart),
        MpvEvent::StartFile => Some(PlayerEvent::Started),
        MpvEvent::EndFile { reason, error } => match reason.as_str() {
            "eof" => Some(PlayerEvent::Ended),
            "error" => Some(PlayerEvent::Error(
                error.unwrap_or_else(|| "playback failed".to_string()),
            )),
            _ => None,
        },
        MpvEvent::Property { name, data } => match name.as_str() {
            "time-pos" => data.as_f64().map(PlayerEvent::TimePos),
            "duration" => data.as_f64().map(PlayerEvent::Duration),
            "pause" => data.as_bool().map(PlayerEvent::Pause),
            "paused-for-cache" => data.as_bool().map(PlayerEvent::Buffering),
            "volume" => data.as_f64().map(PlayerEvent::Volume),
            "speed" => data.as_f64().map(PlayerEvent::Speed),
            "mute" => data.as_bool().map(PlayerEvent::Muted),
            _ => None,
        },
        MpvEvent::Unknown(_) => None,
    }
}
