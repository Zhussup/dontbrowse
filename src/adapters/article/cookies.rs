use std::path::Path;
use std::sync::Arc;

use reqwest_cookie_store::CookieStoreMutex;
use url::Url;

use crate::error::{AppError, Result};
use crate::util::paths::Paths;

pub fn load(paths: &Paths) -> Arc<CookieStoreMutex> {
    let store = std::fs::File::open(paths.cookies_file())
        .ok()
        .and_then(|file| cookie_store::serde::json::load(std::io::BufReader::new(file)).ok())
        .unwrap_or_default();
    Arc::new(CookieStoreMutex::new(store))
}

pub fn save(store: &CookieStoreMutex, paths: &Paths) {
    let path = paths.cookies_file();
    let Ok(mut file) = std::fs::File::create(&path) else {
        return;
    };
    if let Ok(guard) = store.lock()
        && let Err(error) = cookie_store::serde::json::save(&guard, &mut file)
    {
        tracing::warn!("could not persist cookies: {error}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

/// Import a Netscape cookies.txt file into the persistent store.
pub fn import_netscape(path: &Path, store: &CookieStoreMutex) -> Result<usize> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| AppError::Tool(format!("could not read cookies file: {e}")))?;
    let mut guard = store
        .lock()
        .map_err(|_| AppError::Tool("cookie store is locked".to_string()))?;

    let mut imported = 0usize;
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }

        let (http_only, rest) = if let Some(rest) = line.strip_prefix("#HttpOnly_") {
            (true, rest)
        } else if line.starts_with('#') {
            continue;
        } else {
            (false, line)
        };

        let fields: Vec<&str> = rest.split('\t').collect();
        if fields.len() < 7 {
            continue;
        }

        let domain = fields[0].trim();
        let path_field = if fields[2].trim().is_empty() {
            "/"
        } else {
            fields[2].trim()
        };
        let secure = fields[3].eq_ignore_ascii_case("TRUE");
        let expiry: i64 = fields[4].trim().parse().unwrap_or(0);
        let name = fields[5].trim();
        let value = fields[6].trim_end_matches('\r');

        if name.is_empty() {
            continue;
        }
        let host = domain.trim_start_matches('.');
        if host.is_empty() {
            continue;
        }

        let scheme = if secure { "https" } else { "http" };
        let Ok(url) = Url::parse(&format!("{scheme}://{host}{path_field}")) else {
            continue;
        };

        let mut builder = cookie::Cookie::build((name.to_string(), value.to_string()))
            .path(path_field.to_string())
            .secure(secure)
            .http_only(http_only);
        if !domain.is_empty() {
            builder = builder.domain(host.to_string());
        }
        if expiry > 0
            && let Ok(when) = time::OffsetDateTime::from_unix_timestamp(expiry)
        {
            builder = builder.expires(when);
        }

        let cookie = builder.build();
        if guard.insert_raw(&cookie, &url).is_ok() {
            imported += 1;
        }
    }
    Ok(imported)
}

/// Import `cookies.txt` (if configured) into the persistent cookie jar.
pub fn import_configured(cfg: &crate::config::Config, paths: &Paths) -> Result<usize> {
    let Some(file) = &cfg.cookies_txt else {
        return Ok(0);
    };
    let store = load(paths);
    let count = import_netscape(file, &store)?;
    if count > 0 {
        save(&store, paths);
    }
    Ok(count)
}
