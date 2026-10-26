//! Content-addressed download cache.
//!
//! ```text
//! <cache>/metadata/<registry-hash>/<name>/<version>.json
//! <cache>/archives/<sha256>.tar.gz
//! <cache>/sources/<sha256>/<name>-<version>/...
//! ```

use crate::archive::{self, Limits};
use crate::{atomic_write, sha256_hex};
use beaver_source::Diagnostic;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub struct Cache {
    root: PathBuf,
}

fn err(m: impl Into<String>) -> Diagnostic {
    Diagnostic::global("B6050", m.into())
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(crate::hex(&h.finalize()))
}

impl Cache {
    pub fn new(root: PathBuf) -> Cache {
        Cache { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn registry_dir(url: &str) -> String {
        sha256_hex(url.as_bytes())[..16].to_string()
    }

    pub fn metadata_path(&self, url: &str, name: &str, version: &str) -> PathBuf {
        self.root
            .join("metadata")
            .join(Self::registry_dir(url))
            .join(name)
            .join(format!("{version}.json"))
    }

    pub fn read_metadata(&self, url: &str, name: &str, version: &str) -> Option<Vec<u8>> {
        let p = self.metadata_path(url, name, version);
        let md = std::fs::metadata(&p).ok()?;
        if md.len() > crate::registry::MAX_METADATA_BYTES {
            return None;
        }
        std::fs::read(p).ok()
    }

    pub fn write_metadata(
        &self,
        url: &str,
        name: &str,
        version: &str,
        bytes: &[u8],
    ) -> Result<(), Diagnostic> {
        atomic_write(&self.metadata_path(url, name, version), bytes)
            .map_err(|e| err(format!("cannot write metadata cache: {e}")))
    }

    pub fn archive_path(&self, sha: &str) -> PathBuf {
        self.root.join("archives").join(format!("{sha}.tar.gz"))
    }

    /// True if a cached archive with this hash exists and still matches it.
    /// A corrupted cache entry is deleted.
    pub fn verified_archive(&self, sha: &str) -> bool {
        let p = self.archive_path(sha);
        if !p.is_file() {
            return false;
        }
        match sha256_file(&p) {
            Ok(h) if h == sha => true,
            _ => {
                let _ = std::fs::remove_file(&p);
                let _ = std::fs::remove_dir_all(self.root.join("sources").join(sha));
                false
            }
        }
    }

    /// A fresh temporary path inside the cache (same filesystem as targets).
    pub fn temp_path(&self, what: &str) -> Result<PathBuf, Diagnostic> {
        let dir = self.root.join("tmp");
        std::fs::create_dir_all(&dir).map_err(|e| {
            err(format!(
                "cannot create cache directory `{}`: {e}",
                dir.display()
            ))
        })?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Ok(dir.join(format!("{what}-{}-{nanos}", std::process::id())))
    }

    pub fn install_archive(&self, tmp: &Path, sha: &str) -> Result<(), Diagnostic> {
        let dest = self.archive_path(sha);
        std::fs::create_dir_all(dest.parent().unwrap())
            .map_err(|e| err(format!("cannot create cache directory: {e}")))?;
        std::fs::rename(tmp, &dest).map_err(|e| err(format!("cannot store archive in cache: {e}")))
    }

    /// Extracted sources for a verified archive, extracting on first use.
    pub fn ensure_sources(
        &self,
        sha: &str,
        name: &str,
        version: &str,
    ) -> Result<PathBuf, Diagnostic> {
        let dest = self.root.join("sources").join(sha);
        let pkg = dest.join(format!("{name}-{version}"));
        if pkg.join(crate::manifest::MANIFEST).is_file() {
            return Ok(pkg);
        }
        if dest.exists() {
            let _ = std::fs::remove_dir_all(&dest);
        }
        archive::extract(
            &self.archive_path(sha),
            name,
            version,
            &dest,
            Limits::default(),
        )
    }
}
