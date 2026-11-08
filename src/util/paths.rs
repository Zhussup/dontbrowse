use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use std::os::unix::fs::PermissionsExt;

/// XDG application directories.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl Default for Paths {
    fn default() -> Self {
        Self::new()
    }
}

impl Paths {
    pub fn new() -> Self {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dontbrowse");
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dontbrowse");
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dontbrowse");
        let runtime_dir = dirs::runtime_dir()
            .map(|p| p.join("dontbrowse"))
            .unwrap_or_else(|| std::env::temp_dir().join("dontbrowse-runtime"));
        Self {
            config_dir,
            data_dir,
            cache_dir,
            runtime_dir,
        }
    }

    pub fn ensure(&self) -> anyhow::Result<()> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.runtime_dir,
        ] {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        set_private(&self.data_dir)?;
        set_private(&self.runtime_dir)?;
        let thumbs = self.thumbs_dir();
        fs::create_dir_all(&thumbs).with_context(|| format!("creating {}", thumbs.display()))?;
        let log = self.log_dir();
        fs::create_dir_all(&log).with_context(|| format!("creating {}", log.display()))?;
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("db.sqlite3")
    }

    pub fn cookies_file(&self) -> PathBuf {
        self.data_dir.join("cookies.json")
    }

    pub fn thumbs_dir(&self) -> PathBuf {
        self.cache_dir.join("thumbs")
    }

    pub fn raw_cache_dir(&self) -> PathBuf {
        self.cache_dir.join("raw")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.cache_dir.join("log")
    }

    pub fn log_file(&self) -> PathBuf {
        self.log_dir().join("dontbrowse.log")
    }

    pub fn mpv_socket(&self) -> PathBuf {
        self.runtime_dir
            .join(format!("mpv-{}.sock", std::process::id()))
    }
}

fn set_private(path: &Path) -> anyhow::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms)?;
    Ok(())
}
