use serde_json::{Value, json};

/// JSON-line messages understood by mpv's input IPC server.
pub fn loadfile(url: &str) -> Value {
    json!({"command": ["loadfile", url, "replace"]})
}

pub fn stop() -> Value {
    json!({"command": ["stop"]})
}

pub fn quit() -> Value {
    json!({"command": ["quit"]})
}

pub fn cycle_pause() -> Value {
    json!({"command": ["cycle", "pause"]})
}

pub fn set_property(name: &str, value: Value) -> Value {
    json!({"command": ["set_property", name, value]})
}

pub fn seek_relative(seconds: f64) -> Value {
    json!({"command": ["seek", seconds, "relative"]})
}

pub fn seek_absolute(seconds: f64) -> Value {
    json!({"command": ["seek", seconds, "absolute"]})
}

pub fn add_volume(delta: f64) -> Value {
    json!({"command": ["add", "volume", delta]})
}

pub fn observe(id: u64, name: &str) -> Value {
    json!({"command": ["observe_property", id, name]})
}

#[derive(Debug, Clone)]
pub enum MpvEvent {
    FileLoaded,
    PlaybackRestart,
    StartFile,
    EndFile {
        reason: String,
        error: Option<String>,
    },
    Property {
        name: String,
        data: Value,
    },
    Unknown(String),
}

pub fn parse_event(line: &str) -> Option<MpvEvent> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let event = value.get("event")?.as_str()?;
    Some(match event {
        "file-loaded" => MpvEvent::FileLoaded,
        "playback-restart" => MpvEvent::PlaybackRestart,
        "start-file" => MpvEvent::StartFile,
        "end-file" => MpvEvent::EndFile {
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            error: value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "property-change" => MpvEvent::Property {
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            data: value.get("data").cloned().unwrap_or(Value::Null),
        },
        other => MpvEvent::Unknown(other.to_string()),
    })
}
