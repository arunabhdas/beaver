//! Registry protocol v1 client.
//!
//! `GET {base}/v1/packages/{name}/{version}/metadata.json` and
//! `GET {base}/v1/packages/{name}/{version}/archive.tar.gz`. URLs are always
//! built from the configured base; nothing in a response can redirect the
//! client to another origin (redirects are refused).

use crate::manifest::{validate_name, Version};
use beaver_source::Diagnostic;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::time::Duration;

pub const MAX_METADATA_BYTES: u64 = 128 * 1024;
pub const MAX_ARCHIVE_BYTES: u64 = 20 * 1024 * 1024;

/// Abstract HTTP GET so that offline mode can provably avoid the network.
pub trait Transport {
    /// Stream the body of a 200 response into `sink`, failing if it exceeds
    /// `max_bytes`. Returns the number of bytes written.
    fn get(
        &self,
        url: &str,
        accept: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
    ) -> Result<u64, Diagnostic>;
}

pub struct HttpTransport {
    agent: ureq::Agent,
}

impl HttpTransport {
    pub fn new() -> HttpTransport {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(concat!("beaver/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        HttpTransport { agent }
    }
}

impl Default for HttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for HttpTransport {
    fn get(
        &self,
        url: &str,
        accept: &str,
        max_bytes: u64,
        sink: &mut dyn Write,
    ) -> Result<u64, Diagnostic> {
        let resp = self
            .agent
            .get(url)
            .header("Accept", accept)
            .call()
            .map_err(|e| Diagnostic::global("B6021", format!("request to `{url}` failed: {e}")))?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            return Err(Diagnostic::global(
                "B6022",
                format!("registry responded to `{url}` with redirect {status}; redirects are not followed"),
            )
            .with_note("Beaver 0.1 only talks to the configured registry origin"));
        }
        if status == 404 {
            return Err(Diagnostic::global(
                "B6023",
                format!("not found in registry: `{url}`"),
            ));
        }
        if status != 200 {
            return Err(Diagnostic::global(
                "B6021",
                format!("registry returned HTTP {status} for `{url}`"),
            ));
        }
        if let Some(len) = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            if len > max_bytes {
                return Err(Diagnostic::global(
                    "B6024",
                    format!("response from `{url}` is {len} bytes, above the limit of {max_bytes}"),
                ));
            }
        }
        let mut reader = resp
            .into_body()
            .into_with_config()
            .limit(max_bytes + 1)
            .reader();
        let mut buf = [0u8; 16 * 1024];
        let mut total = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| {
                Diagnostic::global("B6021", format!("error while downloading `{url}`: {e}"))
            })?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > max_bytes {
                return Err(Diagnostic::global(
                    "B6024",
                    format!("response from `{url}` exceeds the limit of {max_bytes} bytes"),
                ));
            }
            sink.write_all(&buf[..n])
                .map_err(|e| Diagnostic::global("B6025", format!("cannot write download: {e}")))?;
        }
        Ok(total)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub schema: u32,
    pub name: String,
    pub version: String,
    pub sha256: String,
    pub archive_size: u64,
    pub dependencies: BTreeMap<String, String>,
}

impl Metadata {
    /// Parse and validate metadata for the requested package.
    pub fn parse(bytes: &[u8], name: &str, version: &Version) -> Result<Metadata, Diagnostic> {
        let bad = |m: String| {
            Diagnostic::global(
                "B6026",
                format!("invalid registry metadata for {name}@{version}: {m}"),
            )
        };
        if bytes.len() as u64 > MAX_METADATA_BYTES {
            return Err(bad("metadata is too large".into()));
        }
        let m: Metadata = serde_json::from_slice(bytes).map_err(|e| bad(e.to_string()))?;
        if m.schema != 1 {
            return Err(bad(format!("unsupported schema {}", m.schema)));
        }
        if m.name != name || m.version != version.to_string() {
            return Err(bad(format!(
                "metadata describes {}@{} instead",
                m.name, m.version
            )));
        }
        if !crate::lockfile::is_sha256_hex(&m.sha256) {
            return Err(bad("`sha256` must be 64 lowercase hex digits".into()));
        }
        if m.archive_size == 0 || m.archive_size > MAX_ARCHIVE_BYTES {
            return Err(bad(format!(
                "archive_size {} is outside 1..={MAX_ARCHIVE_BYTES}",
                m.archive_size
            )));
        }
        for (d, v) in &m.dependencies {
            validate_name(d).map_err(&bad)?;
            Version::parse(v).map_err(&bad)?;
        }
        Ok(m)
    }

    pub fn deps(&self) -> BTreeMap<String, Version> {
        self.dependencies
            .iter()
            .map(|(k, v)| (k.clone(), Version::parse(v).expect("validated")))
            .collect()
    }
}

pub fn metadata_url(base: &str, name: &str, version: &Version) -> String {
    format!("{base}/v1/packages/{name}/{version}/metadata.json")
}

pub fn archive_url(base: &str, name: &str, version: &Version) -> String {
    format!("{base}/v1/packages/{name}/{version}/archive.tar.gz")
}
