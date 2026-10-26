//! Shared helpers for `beaver` end-to-end tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn beaver_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_beaver"))
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

pub fn spec_dir() -> PathBuf {
    repo_root().join("Beaver-0.1-spec")
}

/// A `beaver` invocation with an isolated environment: no user registry or
/// config, a private cache, strict C warnings and runtime leak checking.
pub fn beaver(dir: &Path, cache: &Path, args: &[&str]) -> Output {
    beaver_cmd(dir, cache, args).output().expect("run beaver")
}

pub fn beaver_cmd(dir: &Path, cache: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(beaver_bin());
    c.args(args)
        .current_dir(dir)
        .env("BEAVER_CACHE_DIR", cache)
        .env("XDG_CONFIG_HOME", cache.join("config"))
        .env("BEAVER_WERROR", "1")
        .env("BEAVER_RT_LEAK_CHECK", "1")
        .env_remove("BEAVER_REGISTRY_URL")
        .env_remove("BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY")
        .env_remove("BEAVER_SANITIZE");
    c
}

pub fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub fn describe(o: &Output) -> String {
    format!(
        "status: {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        o.status.code(),
        text(&o.stdout),
        text(&o.stderr)
    )
}

/// Create a binary package whose `src/main.bv` is `source`.
pub fn bin_package(root: &Path, name: &str, source: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("beaver.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\nkind = \"bin\"\nentry = \"src/main.bv\"\n\n[dependencies]\n"),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.bv"), source).unwrap();
    dir
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let t = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

/// Fixtures `<dir>/*.bv`, sorted.
pub fn fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("bv"))
        .collect();
    v.sort();
    v
}

/// Run `f` over `items` on a few threads, collecting failure messages.
pub fn parallel<T: Sync>(items: &[T], f: impl Fn(&T) -> Result<(), String> + Sync) -> Vec<String> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if i >= items.len() {
                    break;
                }
                if let Err(m) = f(&items[i]) {
                    failures.lock().unwrap().push(m);
                }
            });
        }
    });
    failures.into_inner().unwrap()
}
