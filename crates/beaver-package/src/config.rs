//! Registry configuration, URL security policy and cache location.

use beaver_source::Diagnostic;
use serde::Deserialize;
use std::path::PathBuf;

pub const INSECURE_OPT_IN: &str = "BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY";

/// Validate and normalize a registry base URL.
///
/// * `https://host[:port][/path]` is accepted.
/// * `http://` is accepted only for the literal loopback hosts `127.0.0.1`
///   and `[::1]`, and only when `BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1`.
/// * Credentials, queries and fragments are rejected.
pub fn validate_registry_url(raw: &str) -> Result<String, Diagnostic> {
    let bad = |m: String| Diagnostic::global("B6020", m);
    let url =
        url::Url::parse(raw).map_err(|e| bad(format!("invalid registry URL `{raw}`: {e}")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad(format!(
            "registry URL `{raw}` must not contain credentials"
        )));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(bad(format!(
            "registry URL `{raw}` must not contain a query or fragment"
        )));
    }
    let host = url.host_str().unwrap_or("").to_string();
    if host.is_empty() {
        return Err(bad(format!("registry URL `{raw}` has no host")));
    }
    match url.scheme() {
        "https" => {}
        "http" => {
            let loopback = host == "127.0.0.1" || host == "[::1]";
            if !loopback {
                return Err(bad(format!(
                    "refusing insecure registry URL `{raw}`: plain HTTP is only allowed for 127.0.0.1 or [::1]"
                ))
                .with_help("use an https:// registry"));
            }
            if std::env::var(INSECURE_OPT_IN).as_deref() != Ok("1") {
                return Err(bad(format!("refusing insecure registry URL `{raw}`"))
                    .with_note(
                        "plain HTTP to a loopback registry is only for local integration testing",
                    )
                    .with_help(format!("set {INSECURE_OPT_IN}=1 to allow it explicitly")));
            }
        }
        other => {
            return Err(bad(format!(
                "unsupported registry URL scheme `{other}` (use https)"
            )))
        }
    }
    let mut s = url.as_str().to_string();
    while s.ends_with('/') {
        s.pop();
    }
    Ok(s)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UserConfig {
    registry: Option<RegistryConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryConfig {
    url: String,
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(x).join("beaver").join("config.toml"));
    }
    std::env::var_os("HOME").map(|h| {
        PathBuf::from(h)
            .join(".config")
            .join("beaver")
            .join("config.toml")
    })
}

/// The registry from `--registry-url`, `BEAVER_REGISTRY_URL`, or the user
/// config file, in that order. `None` if none is configured.
pub fn configured_registry(flag: Option<&str>) -> Result<Option<String>, Diagnostic> {
    if let Some(f) = flag {
        return validate_registry_url(f).map(Some);
    }
    if let Ok(v) = std::env::var("BEAVER_REGISTRY_URL") {
        if !v.trim().is_empty() {
            return validate_registry_url(v.trim()).map(Some);
        }
    }
    if let Some(path) = config_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            let cfg: UserConfig = toml::from_str(&text).map_err(|e| {
                Diagnostic::global(
                    "B6020",
                    format!("invalid config file `{}`: {}", path.display(), e.message()),
                )
            })?;
            if let Some(r) = cfg.registry {
                return validate_registry_url(&r.url).map(Some);
            }
        }
    }
    Ok(None)
}

pub fn cache_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("BEAVER_CACHE_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(x);
    }
    if let Some(x) = std::env::var_os("XDG_CACHE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(x).join("beaver");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library").join("Caches").join("beaver")
    } else {
        home.join(".cache").join("beaver")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_accepted_and_normalized() {
        assert_eq!(
            validate_registry_url("https://packages.example.invalid/beaver/").unwrap(),
            "https://packages.example.invalid/beaver"
        );
    }

    #[test]
    fn insecure_and_odd_urls_rejected() {
        for u in [
            "http://example.com",
            "http://10.0.0.1:8080",
            "http://localhost:8765",
            "ftp://example.com",
            "https://user:pw@example.com",
            "https://example.com/?x=1",
            "file:///tmp/registry",
            "not a url",
        ] {
            assert!(validate_registry_url(u).is_err(), "{u}");
        }
    }
}
