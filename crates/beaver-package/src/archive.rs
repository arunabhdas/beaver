//! Deterministic package archives and validated extraction.

use crate::manifest::{Manifest, MANIFEST};
use beaver_source::Diagnostic;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::{Compression, GzBuilder};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_total_bytes: 100 * 1024 * 1024,
            max_file_bytes: 10 * 1024 * 1024,
            max_entries: 8192,
        }
    }
}

fn err(m: impl Into<String>) -> Diagnostic {
    Diagnostic::global("B6030", m.into())
}

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn valid_component(c: &str) -> bool {
    let mut chars = c.chars();
    let first = matches!(chars.next(), Some(ch) if ch.is_ascii_alphanumeric() || ch == '_');
    let stem = c.split('.').next().unwrap_or("").to_ascii_lowercase();
    first
        && c.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
        && !c.contains("..")
        && !WINDOWS_RESERVED.contains(&stem.as_str())
}

/// Package-relative paths permitted inside an archive.
pub fn allowed_package_path(parts: &[&str]) -> bool {
    match parts {
        ["beaver.toml"] | ["README.md"] | ["LICENSE"] => true,
        ["tests", f] => f.ends_with(".bv"),
        ["src", .., last] => last.ends_with(".bv"),
        _ => false,
    }
}

/// Validate one raw tar path and return the package-relative components.
pub fn check_entry_path(raw: &[u8], root: &str, is_dir: bool) -> Result<Vec<String>, String> {
    let s = std::str::from_utf8(raw).map_err(|_| "path is not valid UTF-8".to_string())?;
    if s.contains('\0') {
        return Err(format!("path `{}` contains NUL", s.escape_debug()));
    }
    if s.contains('\\') {
        return Err(format!("path `{s}` contains a backslash"));
    }
    if s.starts_with('/') {
        return Err(format!("absolute path `{s}`"));
    }
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        return Err(format!("drive-letter path `{s}`"));
    }
    let trimmed = if is_dir { s.trim_end_matches('/') } else { s };
    let parts: Vec<&str> = trimmed.split('/').collect();
    for p in &parts {
        if *p == ".." {
            return Err(format!("path traversal component `..` in `{s}`"));
        }
        if p.is_empty() || *p == "." {
            return Err(format!("empty or `.` path component in `{s}`"));
        }
    }
    if parts[0] != root {
        return Err(format!(
            "entry `{s}` is outside the package root directory `{root}/`"
        ));
    }
    let rel = &parts[1..];
    for p in rel {
        if !valid_component(p) {
            return Err(format!("prohibited file name component `{p}` in `{s}`"));
        }
    }
    if is_dir {
        let ok = rel.is_empty() || rel[0] == "src" || (rel == ["tests"]);
        if !ok {
            return Err(format!("unexpected directory `{s}`"));
        }
    } else if !allowed_package_path(rel) {
        return Err(format!("file `{s}` is not allowed in a Beaver package"));
    }
    Ok(rel.iter().map(|x| x.to_string()).collect())
}

/// Validate `archive` and extract it into a new directory `dest`.
///
/// Every entry is checked before any byte of it is written; files are
/// written only inside a freshly created private staging directory, which is
/// renamed into place after the whole archive passed validation.
pub fn extract(
    archive: &Path,
    name: &str,
    version: &str,
    dest: &Path,
    limits: Limits,
) -> Result<PathBuf, Diagnostic> {
    let root = format!("{name}-{version}");
    let parent = dest
        .parent()
        .ok_or_else(|| err("invalid extraction directory"))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| err(format!("cannot create `{}`: {e}", parent.display())))?;
    let staging = parent.join(format!(".tmp-extract-{}-{}", std::process::id(), unique()));
    create_private_dir(&staging)?;
    let result = extract_into(archive, &root, &staging, limits);
    match result {
        Ok(()) => {
            if dest.exists() {
                let _ = std::fs::remove_dir_all(&staging);
                return Ok(dest.join(&root));
            }
            std::fs::rename(&staging, dest).map_err(|e| {
                let _ = std::fs::remove_dir_all(&staging);
                err(format!("cannot move extracted package into place: {e}"))
            })?;
            Ok(dest.join(&root))
        }
        Err(d) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(d)
        }
    }
}

fn unique() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn create_private_dir(p: &Path) -> Result<(), Diagnostic> {
    std::fs::create_dir(p).map_err(|e| err(format!("cannot create `{}`: {e}", p.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn extract_into(
    archive: &Path,
    root: &str,
    staging: &Path,
    limits: Limits,
) -> Result<(), Diagnostic> {
    let file =
        std::fs::File::open(archive).map_err(|e| err(format!("cannot open archive: {e}")))?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    let entries = tar
        .entries()
        .map_err(|e| err(format!("corrupt package archive: {e}")))?;
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    let mut count = 0usize;
    let mut has_manifest = false;
    std::fs::create_dir(staging.join(root))
        .map_err(|e| err(format!("cannot create package directory: {e}")))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| err(format!("corrupt package archive: {e}")))?;
        count += 1;
        if count > limits.max_entries {
            return Err(err(format!(
                "archive has more than {} entries",
                limits.max_entries
            )));
        }
        let kind = entry.header().entry_type();
        let is_dir = kind.is_dir();
        if !(kind.is_file() || is_dir) {
            let what = if kind.is_symlink() {
                "symbolic link"
            } else if kind.is_hard_link() {
                "hard link"
            } else {
                "special file"
            };
            let p = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
            return Err(err(format!(
                "archive contains a {what} (`{p}`); only regular files are allowed"
            )));
        }
        let raw = entry.path_bytes().into_owned();
        let rel = check_entry_path(&raw, root, is_dir)
            .map_err(|m| err(format!("unsafe archive entry: {m}")))?;
        let lower = rel.join("/").to_ascii_lowercase();
        if !seen.insert(lower) {
            return Err(err(format!(
                "archive contains duplicate or case-colliding path `{}`",
                String::from_utf8_lossy(&raw)
            )));
        }
        let mut target = staging.join(root);
        for c in &rel {
            target.push(c);
        }
        if is_dir {
            std::fs::create_dir_all(&target)
                .map_err(|e| err(format!("cannot create directory: {e}")))?;
            continue;
        }
        let declared = entry
            .header()
            .size()
            .map_err(|e| err(format!("corrupt entry size: {e}")))?;
        if declared > limits.max_file_bytes {
            return Err(err(format!(
                "archive file `{}` is {declared} bytes, above the {} byte limit",
                rel.join("/"),
                limits.max_file_bytes
            )));
        }
        total = total.saturating_add(declared);
        if total > limits.max_total_bytes {
            return Err(err(format!(
                "archive expands to more than {} bytes",
                limits.max_total_bytes
            )));
        }
        let mut data = Vec::with_capacity(declared.min(1 << 20) as usize);
        (&mut entry)
            .take(limits.max_file_bytes + 1)
            .read_to_end(&mut data)
            .map_err(|e| err(format!("corrupt package archive: {e}")))?;
        if data.len() as u64 != declared {
            return Err(err(format!(
                "archive entry `{}` has an inconsistent size",
                rel.join("/")
            )));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| err(format!("cannot create directory: {e}")))?;
        }
        // create_new: never follow or overwrite anything that already exists.
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| err(format!("cannot write `{}`: {e}", rel.join("/"))))?;
        f.write_all(&data)
            .map_err(|e| err(format!("cannot write `{}`: {e}", rel.join("/"))))?;
        if rel == [MANIFEST] {
            has_manifest = true;
        }
    }
    if !has_manifest {
        return Err(err(format!("archive has no `{root}/{MANIFEST}`")));
    }
    Ok(())
}

/// Collect the files that belong in a package archive, sorted.
pub fn collect_package_files(dir: &Path) -> Result<Vec<String>, Diagnostic> {
    let mut files = Vec::new();
    for f in [MANIFEST, "README.md", "LICENSE"] {
        let p = dir.join(f);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(err(format!(
                    "`{f}` is a symlink; symlinks cannot be packaged"
                )))
            }
            Ok(m) if m.is_file() => files.push(f.to_string()),
            Ok(_) => return Err(err(format!("`{f}` is not a regular file"))),
            Err(_) => {}
        }
    }
    walk(dir, "src", true, &mut files)?;
    walk(dir, "tests", false, &mut files)?;
    files.sort();
    Ok(files)
}

fn walk(base: &Path, rel: &str, recursive: bool, out: &mut Vec<String>) -> Result<(), Diagnostic> {
    let dir = base.join(rel);
    let md = match std::fs::symlink_metadata(&dir) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if md.file_type().is_symlink() {
        return Err(err(format!(
            "`{rel}` is a symlink; symlinks cannot be packaged"
        )));
    }
    if !md.is_dir() {
        return Ok(());
    }
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| err(format!("cannot read `{rel}`: {e}")))?
        .filter_map(|e| e.ok())
        .collect();
    names.sort_by_key(|e| e.file_name());
    for e in names {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let child = format!("{rel}/{name}");
        let ft = std::fs::symlink_metadata(e.path())
            .map_err(|e| err(e.to_string()))?
            .file_type();
        if ft.is_symlink() {
            return Err(err(format!(
                "`{child}` is a symlink; symlinks cannot be packaged"
            )));
        }
        if ft.is_dir() {
            if recursive {
                walk(base, &child, true, out)?;
            }
        } else if ft.is_file() && name.ends_with(".bv") {
            if !valid_component(&name) {
                return Err(err(format!(
                    "file name `{child}` is not allowed in a package"
                )));
            }
            out.push(child);
        } else if !ft.is_file() {
            return Err(err(format!("`{child}` is not a regular file")));
        }
    }
    Ok(())
}

/// Build a canonical `.tar.gz`: sorted paths, ustar headers with zero
/// uid/gid/mtime, mode 0644, fixed gzip header. Identical inputs produce
/// identical bytes.
pub fn create_archive(dir: &Path, manifest: &Manifest) -> Result<Vec<u8>, Diagnostic> {
    let root = format!("{}-{}", manifest.name, manifest.version);
    let files = collect_package_files(dir)?;
    let limits = Limits::default();
    if files.len() > limits.max_entries {
        return Err(err("package has too many files"));
    }
    let gz = GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(Vec::new(), Compression::new(6));
    let mut builder = tar::Builder::new(gz);
    builder.mode(tar::HeaderMode::Deterministic);
    let mut total = 0u64;
    for rel in &files {
        let data =
            std::fs::read(dir.join(rel)).map_err(|e| err(format!("cannot read `{rel}`: {e}")))?;
        if data.len() as u64 > limits.max_file_bytes {
            return Err(err(format!(
                "`{rel}` is larger than the 10 MiB per-file limit"
            )));
        }
        total += data.len() as u64;
        let path = format!("{root}/{rel}");
        if path.len() > 100 {
            return Err(err(format!(
                "path `{path}` is too long for a package archive (max 100 bytes)"
            )));
        }
        let mut h = tar::Header::new_ustar();
        h.set_path(&path)
            .map_err(|e| err(format!("cannot add `{rel}`: {e}")))?;
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(0);
        h.set_entry_type(tar::EntryType::Regular);
        let _ = h.set_username("");
        let _ = h.set_groupname("");
        h.set_cksum();
        builder
            .append(&h, data.as_slice())
            .map_err(|e| err(format!("cannot add `{rel}`: {e}")))?;
    }
    if total > limits.max_total_bytes {
        return Err(err(
            "package is larger than the 100 MiB expanded-size limit",
        ));
    }
    let gz: GzEncoder<Vec<u8>> = builder
        .into_inner()
        .map_err(|e| err(format!("cannot finish archive: {e}")))?;
    gz.finish()
        .map_err(|e| err(format!("cannot finish archive: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_checks() {
        let ok = |p: &str| check_entry_path(p.as_bytes(), "m-1.0.0", false);
        assert!(ok("m-1.0.0/beaver.toml").is_ok());
        assert!(ok("m-1.0.0/src/a/b.bv").is_ok());
        assert!(ok("m-1.0.0/tests/smoke.bv").is_ok());
        for bad in [
            "m-1.0.0/../../outside.txt",
            "/etc/passwd",
            "C:/x.bv",
            "m-1.0.0/src/..\\x.bv",
            "other-1.0.0/beaver.toml",
            "m-1.0.0/build.sh",
            "m-1.0.0/src/x.so",
            "m-1.0.0/src/con.bv",
            "m-1.0.0//src/x.bv",
            "m-1.0.0/./beaver.toml",
            "m-1.0.0/src/.hidden.bv",
        ] {
            assert!(ok(bad).is_err(), "{bad}");
        }
        assert!(check_entry_path(b"m-1.0.0/src/\xff.bv", "m-1.0.0", false).is_err());
    }
}
