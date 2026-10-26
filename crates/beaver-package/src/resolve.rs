//! Exact-version dependency resolution over path and registry packages.

use crate::cache::Cache;
use crate::config::{configured_registry, validate_registry_url};
use crate::lockfile::{LockedPackage, Lockfile, Source};
use crate::manifest::{load_manifest, DepSpec, Kind, Manifest, Version};
use crate::registry::{
    archive_url, metadata_url, Metadata, Transport, MAX_ARCHIVE_BYTES, MAX_METADATA_BYTES,
};
use beaver_source::Diagnostic;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub const MAX_DEPTH: usize = 32;
pub const MAX_PACKAGES: usize = 128;

pub struct Options<'a> {
    pub offline: bool,
    pub locked: bool,
    pub registry_flag: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub version: Version,
    pub source: Source,
    pub checksum: Option<String>,
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// Prefix used in diagnostics, e.g. `mathkit-1.0.0` or `../mathkit`.
    pub display: String,
}

pub struct Resolution {
    pub packages: BTreeMap<String, Resolved>,
    pub lock: Lockfile,
}

pub type TransportFactory<'a> = &'a dyn Fn() -> Box<dyn Transport>;

pub fn resolve(
    root_dir: &Path,
    root: &Manifest,
    existing: Option<&Lockfile>,
    opts: &Options,
    cache: &Cache,
    factory: TransportFactory,
) -> Result<Resolution, Diagnostic> {
    let configured = configured_registry(opts.registry_flag)?;
    let mut r = Resolver {
        root_dir: root_dir
            .canonicalize()
            .unwrap_or_else(|_| root_dir.to_path_buf()),
        opts,
        cache,
        lock: existing,
        configured,
        factory,
        transport: None,
        packages: BTreeMap::new(),
        chains: BTreeMap::new(),
        stack: Vec::new(),
    };
    let root_desc = format!("{}@{}", root.name, root.version);
    for (name, spec) in &root.deps {
        r.visit(name, spec, &r.root_dir.clone(), vec![root_desc.clone()])?;
    }
    let lock = r.lockfile();
    Ok(Resolution {
        packages: r.packages,
        lock,
    })
}

struct Resolver<'a> {
    root_dir: PathBuf,
    opts: &'a Options<'a>,
    cache: &'a Cache,
    lock: Option<&'a Lockfile>,
    configured: Option<String>,
    factory: TransportFactory<'a>,
    transport: Option<Box<dyn Transport>>,
    packages: BTreeMap<String, Resolved>,
    chains: BTreeMap<String, Vec<String>>,
    stack: Vec<String>,
}

fn e(code: &'static str, m: impl Into<String>) -> Diagnostic {
    Diagnostic::global(code, m.into())
}

/// Lexical relative path from `base` to `target` (both absolute).
fn relative(base: &Path, target: &Path) -> String {
    let b: Vec<Component> = base.components().collect();
    let t: Vec<Component> = target.components().collect();
    let common = b.iter().zip(&t).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..b.len() {
        parts.push("..".into());
    }
    for c in &t[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

struct HashingWriter<W: Write> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> Resolver<'a> {
    fn describe(name: &str, spec: &DepSpec) -> String {
        match spec {
            DepSpec::Registry(v) => format!("{name}@{v}"),
            DepSpec::Path(p) => format!("{name} (path {p})"),
        }
    }

    fn visit(
        &mut self,
        name: &str,
        spec: &DepSpec,
        from_dir: &Path,
        chain: Vec<String>,
    ) -> Result<(), Diagnostic> {
        let desc = Self::describe(name, spec);
        if let Some(pos) = self.stack.iter().position(|s| s == name) {
            let mut cyc: Vec<String> = chain[pos + 1..].to_vec();
            cyc.push(desc);
            return Err(e(
                "B6040",
                format!("dependency cycle detected: {}", cyc.join(" -> ")),
            )
            .with_note("Beaver packages must form an acyclic graph"));
        }
        if let Some(existing) = self.packages.get(name) {
            let same = match (spec, &existing.source) {
                (DepSpec::Registry(v), Source::Registry(_)) => *v == existing.version,
                (DepSpec::Path(p), Source::Path(_)) => from_dir
                    .join(p)
                    .canonicalize()
                    .map(|d| d == existing.dir)
                    .unwrap_or(false),
                _ => false,
            };
            if !same {
                let prev = self.chains.get(name).cloned().unwrap_or_default();
                let prev_desc = match &existing.source {
                    Source::Registry(_) => format!("{name}@{}", existing.version),
                    Source::Path(p) => format!("{name} (path {p})"),
                };
                return Err(e(
                    "B6041",
                    format!("conflicting requirements for package `{name}`"),
                )
                .with_note(format!("{} -> {prev_desc}", prev.join(" -> ")))
                .with_note(format!("{} -> {desc}", chain.join(" -> ")))
                .with_help(
                    "Beaver 0.1 allows only one version of each package in a dependency graph",
                ));
            }
            return Ok(());
        }
        if chain.len() > MAX_DEPTH {
            return Err(e(
                "B6042",
                format!("dependency graph is deeper than {MAX_DEPTH} levels at `{desc}`"),
            ));
        }
        if self.packages.len() >= MAX_PACKAGES {
            return Err(e(
                "B6043",
                format!("dependency graph has more than {MAX_PACKAGES} packages"),
            ));
        }
        self.stack.push(name.to_string());
        let resolved = match spec {
            DepSpec::Registry(v) => self.registry_package(name, v)?,
            DepSpec::Path(p) => self.path_package(name, from_dir, p)?,
        };
        let deps = resolved.manifest.deps.clone();
        let dir = resolved.dir.clone();
        self.packages.insert(name.to_string(), resolved);
        self.chains.insert(name.to_string(), chain.clone());
        for (dn, ds) in &deps {
            let mut c = chain.clone();
            c.push(desc.clone());
            self.visit(dn, ds, &dir, c)?;
        }
        self.stack.pop();
        Ok(())
    }

    fn path_package(
        &mut self,
        name: &str,
        from_dir: &Path,
        rel: &str,
    ) -> Result<Resolved, Diagnostic> {
        let dir = from_dir.join(rel).canonicalize().map_err(|_| {
            e(
                "B6015",
                format!(
                    "path dependency `{name}` not found at `{}`",
                    from_dir.join(rel).display()
                ),
            )
        })?;
        let manifest = load_manifest(&dir)?;
        if manifest.name != name {
            return Err(e(
                "B6016",
                format!(
                    "dependency `{name}` points to `{}`, but that package is named `{}`",
                    dir.display(),
                    manifest.name
                ),
            ));
        }
        if manifest.kind != Kind::Lib {
            return Err(e(
                "B6031",
                format!(
                    "dependency `{name}` is a binary package; only libraries can be dependencies"
                ),
            ));
        }
        let display = relative(&self.root_dir, &dir);
        Ok(Resolved {
            name: name.to_string(),
            version: manifest.version,
            source: Source::Path(display.clone()),
            checksum: None,
            dir,
            manifest,
            display,
        })
    }

    fn registry_for(&self, name: &str) -> Result<String, Diagnostic> {
        if let Some(c) = &self.configured {
            return Ok(c.clone());
        }
        if let Some(Source::Registry(u)) = self.lock.and_then(|l| l.get(name)).map(|p| &p.source) {
            return validate_registry_url(u);
        }
        Err(e("B6014", format!("no package registry is configured, so `{name}` cannot be resolved"))
            .with_note("Beaver 0.1 has no default public registry")
            .with_help("pass --registry-url URL, set BEAVER_REGISTRY_URL, or add [registry] url = \"...\" to ~/.config/beaver/config.toml"))
    }

    fn transport(&mut self) -> &dyn Transport {
        if self.transport.is_none() {
            self.transport = Some((self.factory)());
        }
        self.transport.as_deref().unwrap()
    }

    fn offline_error(what: &str) -> Diagnostic {
        e(
            "B6013",
            format!(
                "{what} is not in the local package cache, and --offline forbids network access"
            ),
        )
        .with_help("run the command once without --offline to populate the cache")
    }

    fn metadata(
        &mut self,
        reg: &str,
        name: &str,
        version: &Version,
    ) -> Result<Metadata, Diagnostic> {
        let vs = version.to_string();
        if let Some(bytes) = self.cache.read_metadata(reg, name, &vs) {
            if let Ok(m) = Metadata::parse(&bytes, name, version) {
                return Ok(m);
            }
        }
        if self.opts.offline {
            return Err(Self::offline_error(&format!(
                "metadata for {name}@{version}"
            )));
        }
        let url = metadata_url(reg, name, version);
        let mut body = Vec::new();
        self.transport()
            .get(&url, "application/json", MAX_METADATA_BYTES, &mut body)
            .map_err(|d| {
                if d.code == "B6023" {
                    e(
                        "B6023",
                        format!("package {name}@{version} was not found in registry `{reg}`"),
                    )
                    .with_note("Beaver only resolves exact versions; check the name and version")
                } else {
                    d
                }
            })?;
        let m = Metadata::parse(&body, name, version)?;
        self.cache.write_metadata(reg, name, &vs, &body)?;
        Ok(m)
    }

    fn download(
        &mut self,
        reg: &str,
        name: &str,
        version: &Version,
        sha: &str,
        size: u64,
    ) -> Result<(), Diagnostic> {
        if self.opts.offline {
            return Err(Self::offline_error(&format!(
                "archive for {name}@{version}"
            )));
        }
        let url = archive_url(reg, name, version);
        let tmp = self.cache.temp_path("download")?;
        let file = std::fs::File::create(&tmp)
            .map_err(|er| e("B6050", format!("cannot create `{}`: {er}", tmp.display())))?;
        let mut w = HashingWriter {
            inner: std::io::BufWriter::new(file),
            hasher: Sha256::new(),
        };
        let result = self.transport().get(
            &url,
            "application/gzip",
            size.min(MAX_ARCHIVE_BYTES),
            &mut w,
        );
        let n = match result.and_then(|n| {
            w.flush()
                .map(|_| n)
                .map_err(|er| e("B6050", er.to_string()))
        }) {
            Ok(n) => n,
            Err(d) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(d);
            }
        };
        let got = crate::hex(&w.hasher.clone().finalize());
        drop(w);
        if n != size {
            let _ = std::fs::remove_file(&tmp);
            return Err(e(
                "B6034",
                format!(
                    "archive for {name}@{version} is {n} bytes but the registry declared {size}"
                ),
            ));
        }
        if got != sha {
            let _ = std::fs::remove_file(&tmp);
            return Err(
                e("B6035", format!("checksum mismatch for {name}@{version}"))
                    .with_note(format!("expected sha256 {sha}"))
                    .with_note(format!("downloaded sha256 {got}"))
                    .with_note("the archive was discarded and nothing was installed"),
            );
        }
        self.cache.install_archive(&tmp, sha)
    }

    fn registry_package(&mut self, name: &str, version: &Version) -> Result<Resolved, Diagnostic> {
        let reg = self.registry_for(name)?;
        let source = Source::Registry(reg.clone());
        let locked = self
            .lock
            .and_then(|l| l.get(name))
            .filter(|l| l.version == *version && l.source == source)
            .cloned();
        if self.opts.locked && locked.is_none() {
            return Err(e(
                "B6011",
                format!("--locked: beaver.lock has no entry for {name}@{version} from {reg}"),
            )
            .with_help("run `beaver fetch` without --locked to update the lockfile"));
        }
        let (sha, meta) = match &locked {
            Some(l)
                if self
                    .cache
                    .verified_archive(l.checksum.as_deref().unwrap_or("")) =>
            {
                (l.checksum.clone().unwrap(), None)
            }
            _ => {
                let meta = self.metadata(&reg, name, version)?;
                if let Some(l) = &locked {
                    if l.checksum.as_deref() != Some(meta.sha256.as_str()) {
                        return Err(e(
                            "B6012",
                            format!("registry checksum for {name}@{version} does not match beaver.lock"),
                        )
                        .with_note(format!("beaver.lock: {}", l.checksum.clone().unwrap_or_default()))
                        .with_note(format!("registry:    {}", meta.sha256))
                        .with_note("the published artifact changed since it was locked; refusing to use it"));
                    }
                }
                if !self.cache.verified_archive(&meta.sha256) {
                    self.download(&reg, name, version, &meta.sha256, meta.archive_size)?;
                }
                (meta.sha256.clone(), Some(meta))
            }
        };
        let vs = version.to_string();
        let dir = self.cache.ensure_sources(&sha, name, &vs)?;
        let manifest = load_manifest(&dir).map_err(|d| {
            e(
                "B6031",
                format!("package {name}@{version} has an invalid manifest"),
            )
            .with_note(d.message)
        })?;
        if manifest.name != name || manifest.version != *version {
            return Err(e(
                "B6031",
                format!(
                    "archive for {name}@{version} contains package {}@{}",
                    manifest.name, manifest.version
                ),
            ));
        }
        if manifest.kind != Kind::Lib {
            return Err(e(
                "B6031",
                format!("registry package {name}@{version} is not a library"),
            ));
        }
        let mut deps = BTreeMap::new();
        for (dn, ds) in &manifest.deps {
            match ds {
                DepSpec::Registry(v) => {
                    deps.insert(dn.clone(), *v);
                }
                DepSpec::Path(_) => {
                    return Err(e(
                        "B6032",
                        format!("registry package {name}@{version} declares a path dependency `{dn}`, which is not allowed"),
                    ))
                }
            }
        }
        if let Some(meta) = &meta {
            if meta.deps() != deps {
                return Err(e(
                    "B6033",
                    format!("registry metadata and the archive manifest of {name}@{version} disagree about its dependencies"),
                ));
            }
        }
        if let Some(l) = &locked {
            let mut want: Vec<String> = deps.iter().map(|(n, v)| format!("{n}@{v}")).collect();
            want.sort();
            let mut have = l.dependencies.clone();
            have.sort();
            if want != have {
                return Err(e(
                    "B6012",
                    format!("beaver.lock lists different dependencies for {name}@{version} than its manifest"),
                ));
            }
        }
        Ok(Resolved {
            name: name.to_string(),
            version: *version,
            source,
            checksum: Some(sha),
            dir,
            manifest,
            display: format!("{name}-{version}"),
        })
    }

    fn lockfile(&self) -> Lockfile {
        let mut packages = Vec::new();
        for (name, r) in &self.packages {
            let mut deps: Vec<String> = r
                .manifest
                .deps
                .keys()
                .map(|d| format!("{d}@{}", self.packages[d].version))
                .collect();
            deps.sort();
            packages.push(LockedPackage {
                name: name.clone(),
                version: r.version,
                source: r.source.clone(),
                checksum: r.checksum.clone(),
                dependencies: deps,
            });
        }
        Lockfile { packages }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative(Path::new("/a/b/app"), Path::new("/a/b/lib")),
            "../lib"
        );
        assert_eq!(
            relative(Path::new("/a/app"), Path::new("/a/app/vendor/x")),
            "vendor/x"
        );
    }
}
