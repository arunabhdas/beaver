//! Remote package workflow against the static mock registry, served by a
//! small in-process HTTP server bound to 127.0.0.1 for the duration of each
//! test. Covers downloads, lockfiles, the offline cache and every rejection
//! path required by the handoff (checksum, traversal, links, gzip, cycles,
//! conflicts, insecure URLs, redirects, size limits, metadata mismatch).

mod common;
use common::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ----- tiny static HTTP server ---------------------------------------------

struct Server {
    port: u16,
    log: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start(root: PathBuf) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log2, stop2) = (log.clone(), stop.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if stop2.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(c) = conn {
                    let (root, log) = (root.clone(), log2.clone());
                    std::thread::spawn(move || serve(c, &root, &log, port));
                }
            }
        });
        Server { port, log, stop }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn serve(mut c: TcpStream, root: &Path, log: &Mutex<Vec<String>>, port: u16) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match c.read(&mut tmp) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
        if buf.len() > 64 * 1024 {
            return;
        }
    }
    let req = String::from_utf8_lossy(&buf).into_owned();
    let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
    log.lock().unwrap().push(path.clone());
    let resp: Vec<u8> = if let Some(rest) = path.strip_prefix("/redirect") {
        format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}{rest}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
    } else {
        let file = root.join(path.trim_start_matches('/'));
        match (path.contains(".."), std::fs::read(&file)) {
            (false, Ok(body)) => {
                let mut r = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                r.extend_from_slice(&body);
                r
            }
            _ => {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            }
        }
    };
    let _ = c.write_all(&resp);
    let _ = c.flush();
}

// ----- registry construction ----------------------------------------------

enum Entry {
    File(&'static str, Vec<u8>),
    Symlink(&'static str, &'static str),
    Hardlink(&'static str, &'static str),
    RawPath(Vec<u8>, Vec<u8>),
}

fn lib_manifest(name: &str, version: &str, deps: &[(&str, &str)]) -> Vec<u8> {
    let mut s = format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2026\"\nkind = \"lib\"\nentry = \"src/lib.bv\"\n\n[dependencies]\n");
    for (d, v) in deps {
        s.push_str(&format!("{d} = {{ version = \"{v}\" }}\n"));
    }
    s.into_bytes()
}

fn tar_gz(root: &str, entries: Vec<Entry>) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for e in entries {
        let mut h = tar::Header::new_ustar();
        h.set_mode(0o644);
        h.set_mtime(0);
        match e {
            Entry::File(p, data) => {
                h.set_path(format!("{root}/{p}")).unwrap();
                h.set_size(data.len() as u64);
                h.set_entry_type(tar::EntryType::Regular);
                h.set_cksum();
                b.append(&h, data.as_slice()).unwrap();
            }
            Entry::Symlink(p, target) | Entry::Hardlink(p, target) => {
                let kind = if matches!(e, Entry::Symlink(..)) {
                    tar::EntryType::Symlink
                } else {
                    tar::EntryType::Link
                };
                h.set_path(format!("{root}/{p}")).unwrap();
                h.set_size(0);
                h.set_entry_type(kind);
                h.set_link_name(target).unwrap();
                h.set_cksum();
                b.append(&h, std::io::empty()).unwrap();
            }
            Entry::RawPath(path, data) => {
                // Bypass the tar crate's path sanitizing.
                let name = &mut h.as_old_mut().name;
                name[..path.len()].copy_from_slice(&path);
                h.set_size(data.len() as u64);
                h.set_entry_type(tar::EntryType::Regular);
                h.set_cksum();
                b.append(&h, data.as_slice()).unwrap();
            }
        }
    }
    let tar = b.into_inner().unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar).unwrap();
    gz.finish().unwrap()
}

fn sha256(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// Publish into a static registry directory; `meta_edit` may tamper with
/// the metadata JSON text.
fn publish(
    reg: &Path,
    name: &str,
    version: &str,
    deps: &[(&str, &str)],
    archive: Vec<u8>,
    meta_edit: impl Fn(String) -> String,
) {
    let dir = reg.join("v1/packages").join(name).join(version);
    std::fs::create_dir_all(&dir).unwrap();
    let dep_json: Vec<String> = deps
        .iter()
        .map(|(d, v)| format!("\"{d}\": \"{v}\""))
        .collect();
    let meta = format!(
        "{{\"schema\": 1, \"name\": \"{name}\", \"version\": \"{version}\", \"sha256\": \"{}\", \"archive_size\": {}, \"dependencies\": {{{}}}}}",
        sha256(&archive),
        archive.len(),
        dep_json.join(", ")
    );
    std::fs::write(dir.join("metadata.json"), meta_edit(meta)).unwrap();
    std::fs::write(dir.join("archive.tar.gz"), archive).unwrap();
}

fn standard_lib(name: &str, version: &str, deps: &[(&str, &str)]) -> Vec<u8> {
    tar_gz(
        &format!("{name}-{version}"),
        vec![
            Entry::File("beaver.toml", lib_manifest(name, version, deps)),
            Entry::File(
                "src/lib.bv",
                b"pub fn id(v: Int) -> Int:\n    return v\n".to_vec(),
            ),
        ],
    )
}

struct Env {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    registry: PathBuf,
    cache: PathBuf,
    server: Server,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let registry = root.join("registry");
    copy_dir(&spec_dir().join("fixtures/registry"), &registry);
    let r = &registry;
    publish(
        r,
        "bigmeta",
        "1.0.0",
        &[],
        standard_lib("bigmeta", "1.0.0", &[]),
        |m| format!("{}{}", m, " ".repeat(200 * 1024)),
    );
    publish(
        r,
        "sizelie",
        "1.0.0",
        &[],
        standard_lib("sizelie", "1.0.0", &[]),
        |m| {
            let real = m
                .split("\"archive_size\": ")
                .nth(1)
                .unwrap()
                .split(',')
                .next()
                .unwrap()
                .to_string();
            m.replace(
                &format!("\"archive_size\": {real}"),
                "\"archive_size\": 5000",
            )
        },
    );
    publish(
        r,
        "depslie",
        "1.0.0",
        &[],
        standard_lib("depslie", "1.0.0", &[("mathkit", "1.0.0")]),
        |m| m,
    );
    publish(
        r,
        "evilurl",
        "1.0.0",
        &[],
        standard_lib("evilurl", "1.0.0", &[]),
        |m| {
            m.replace(
                "\"schema\": 1",
                "\"schema\": 1, \"archive_url\": \"http://127.0.0.1:9/steal\"",
            )
        },
    );
    publish(
        r,
        "wrongname",
        "1.0.0",
        &[],
        tar_gz(
            "wrongname-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("othername", "1.0.0", &[])),
                Entry::File(
                    "src/lib.bv",
                    b"pub fn id(v: Int) -> Int:\n    return v\n".to_vec(),
                ),
            ],
        ),
        |m| m,
    );
    publish(
        r,
        "hardtrap",
        "1.0.0",
        &[],
        tar_gz(
            "hardtrap-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("hardtrap", "1.0.0", &[])),
                Entry::Hardlink("src/lib.bv", "/etc/passwd"),
            ],
        ),
        |m| m,
    );
    publish(
        r,
        "reltrap",
        "1.0.0",
        &[],
        tar_gz(
            "reltrap-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("reltrap", "1.0.0", &[])),
                Entry::Symlink("src/lib.bv", "../../../../outside.bv"),
            ],
        ),
        |m| m,
    );
    publish(
        r,
        "abstrap",
        "1.0.0",
        &[],
        tar_gz(
            "abstrap-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("abstrap", "1.0.0", &[])),
                Entry::RawPath(b"/tmp/beaver-abs-escape.bv".to_vec(), b"x".to_vec()),
            ],
        ),
        |m| m,
    );
    publish(
        r,
        "bomb",
        "1.0.0",
        &[],
        tar_gz(
            "bomb-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("bomb", "1.0.0", &[])),
                Entry::File("src/lib.bv", vec![b' '; 11 * 1024 * 1024]),
            ],
        ),
        |m| m,
    );
    publish(
        r,
        "scripted",
        "1.0.0",
        &[],
        tar_gz(
            "scripted-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("scripted", "1.0.0", &[])),
                Entry::File("build.sh", b"#!/bin/sh\ntouch /tmp/pwned\n".to_vec()),
                Entry::File(
                    "src/lib.bv",
                    b"pub fn id(v: Int) -> Int:\n    return v\n".to_vec(),
                ),
            ],
        ),
        |m| m,
    );
    publish(
        r,
        "casedup",
        "1.0.0",
        &[],
        tar_gz(
            "casedup-1.0.0",
            vec![
                Entry::File("beaver.toml", lib_manifest("casedup", "1.0.0", &[])),
                Entry::File(
                    "src/lib.bv",
                    b"pub fn id(v: Int) -> Int:\n    return v\n".to_vec(),
                ),
                Entry::File(
                    "src/LIB.bv",
                    b"pub fn id(v: Int) -> Int:\n    return v\n".to_vec(),
                ),
            ],
        ),
        |m| m,
    );
    let server = Server::start(registry.clone());
    Env {
        cache: root.join("cache"),
        _tmp: tmp,
        root,
        registry,
        server,
    }
}

impl Env {
    /// A fresh copy of the handoff's `consumer` example without its
    /// dependency, so that `beaver add` is exercised.
    fn consumer(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        copy_dir(&spec_dir().join("examples/consumer"), &dir);
        let m = std::fs::read_to_string(dir.join("beaver.toml")).unwrap();
        let m = m
            .replace("mathkit = { version = \"1.0.0\" }\n", "")
            .replace("name = \"consumer\"", &format!("name = \"{name}\""));
        std::fs::write(dir.join("beaver.toml"), m).unwrap();
        dir
    }

    fn run(&self, dir: &Path, args: &[&str]) -> std::process::Output {
        beaver_cmd(dir, &self.cache, args)
            .env("BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY", "1")
            .env("BEAVER_REGISTRY_URL", self.server.url())
            .output()
            .unwrap()
    }

    fn add_fails(&self, spec: &str, needle: &str) {
        let dir = self.consumer(&format!("app_{}", spec.split('@').next().unwrap()));
        let before = std::fs::read(dir.join("beaver.toml")).unwrap();
        let out = self.run(&dir, &["add", spec]);
        assert!(
            !out.status.success(),
            "`add {spec}` should fail\n{}",
            describe(&out)
        );
        let stderr = text(&out.stderr);
        eprintln!("$ beaver add {spec}\n{stderr}");
        assert!(
            stderr.contains(needle),
            "`add {spec}`: expected `{needle}`\n{}",
            describe(&out)
        );
        assert!(!stderr.contains("panicked"), "{}", describe(&out));
        assert_eq!(
            std::fs::read(dir.join("beaver.toml")).unwrap(),
            before,
            "manifest modified after failure"
        );
        assert!(
            !dir.join("beaver.lock").exists(),
            "lockfile written after failure"
        );
    }
}

fn fixture_sha(name: &str, version: &str) -> String {
    let m = std::fs::read_to_string(spec_dir().join(format!(
        "fixtures/registry/v1/packages/{name}/{version}/metadata.json"
    )))
    .unwrap();
    m.split("\"sha256\": \"").nth(1).unwrap()[..64].to_string()
}

// ----- tests -----------------------------------------------------------------

#[test]
fn add_downloads_verifies_locks_and_runs_the_remote_library() {
    let e = env();
    let app = e.consumer("app");
    let out = e.run(&app, &["add", "mathkit@1.0.0"]);
    assert!(out.status.success(), "{}", describe(&out));
    let manifest = std::fs::read_to_string(app.join("beaver.toml")).unwrap();
    assert!(
        manifest.contains("mathkit = { version = \"1.0.0\" }"),
        "{manifest}"
    );
    let lock = std::fs::read_to_string(app.join("beaver.lock")).unwrap();
    assert!(lock.contains("name = \"mathkit\""), "{lock}");
    assert!(
        lock.contains(&format!(
            "checksum = \"{}\"",
            fixture_sha("mathkit", "1.0.0")
        )),
        "{lock}"
    );
    assert!(
        lock.contains(&format!("source = \"registry+{}\"", e.server.url())),
        "{lock}"
    );
    let reqs = e.server.requests();
    assert!(
        reqs.contains(&"/v1/packages/mathkit/1.0.0/metadata.json".to_string()),
        "{reqs:?}"
    );
    assert!(
        reqs.contains(&"/v1/packages/mathkit/1.0.0/archive.tar.gz".to_string()),
        "{reqs:?}"
    );

    let run = e.run(&app, &["run"]);
    assert!(run.status.success(), "{}", describe(&run));
    assert_eq!(text(&run.stdout), "49\n");
    let test = e.run(&app, &["test"]);
    assert!(test.status.success(), "{}", describe(&test));
    assert!(
        text(&test.stderr).contains("1 passed; 0 failed"),
        "{}",
        describe(&test)
    );

    // The lockfile is reproducible: re-fetching changes nothing.
    let fetch = e.run(&app, &["fetch"]);
    assert!(fetch.status.success(), "{}", describe(&fetch));
    assert_eq!(
        std::fs::read_to_string(app.join("beaver.lock")).unwrap(),
        lock
    );
}

#[test]
fn offline_locked_uses_warm_cache_without_network_and_fails_clearly_when_cold() {
    let e = env();
    let app = e.consumer("app");
    assert!(e.run(&app, &["add", "mathkit@1.0.0"]).status.success());
    let before = e.server.requests().len();
    let fetch = e.run(&app, &["fetch", "--offline", "--locked"]);
    assert!(fetch.status.success(), "{}", describe(&fetch));
    let run = e.run(&app, &["run", "--offline", "--locked"]);
    assert!(run.status.success(), "{}", describe(&run));
    assert_eq!(text(&run.stdout), "49\n");
    assert_eq!(
        e.server.requests().len(),
        before,
        "offline commands made HTTP requests"
    );

    // Cold cache: a different cache directory.
    let cold = beaver_cmd(
        &app,
        &e.root.join("cold-cache"),
        &["fetch", "--offline", "--locked"],
    )
    .env("BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY", "1")
    .env("BEAVER_REGISTRY_URL", e.server.url())
    .output()
    .unwrap();
    assert!(!cold.status.success());
    let stderr = text(&cold.stderr);
    assert!(
        stderr.contains("error[B6013]") && stderr.contains("--offline"),
        "{}",
        describe(&cold)
    );
    assert_eq!(
        e.server.requests().len(),
        before,
        "offline fetch with cold cache made HTTP requests"
    );
}

#[test]
fn locked_rejects_missing_or_stale_lockfiles() {
    let e = env();
    let app = e.consumer("app");
    let m = std::fs::read_to_string(app.join("beaver.toml")).unwrap();
    std::fs::write(
        app.join("beaver.toml"),
        format!("{m}mathkit = {{ version = \"1.0.0\" }}\n"),
    )
    .unwrap();
    let out = e.run(&app, &["build", "--locked"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("requires an existing beaver.lock"),
        "{}",
        describe(&out)
    );
    assert!(e.run(&app, &["fetch"]).status.success());
    // Stale: manifest now asks for 2.0.0.
    let m = std::fs::read_to_string(app.join("beaver.toml")).unwrap();
    std::fs::write(app.join("beaver.toml"), m.replace("1.0.0\" }", "2.0.0\" }")).unwrap();
    let lock = std::fs::read(app.join("beaver.lock")).unwrap();
    let out = e.run(&app, &["fetch", "--locked"]);
    assert!(!out.status.success(), "{}", describe(&out));
    assert!(
        text(&out.stderr).contains("error[B6011]"),
        "{}",
        describe(&out)
    );
    let out = e.run(&app, &["check"]);
    assert!(
        !out.status.success() && text(&out.stderr).contains("out of date"),
        "{}",
        describe(&out)
    );
    assert_eq!(
        std::fs::read(app.join("beaver.lock")).unwrap(),
        lock,
        "lockfile changed"
    );
    let out = e.run(&app, &["add", "--locked", "requires_a@1.0.0"]);
    assert!(
        !out.status.success() && text(&out.stderr).contains("cannot be used with `beaver add`")
    );
}

#[test]
fn update_and_remove_rewrite_manifest_and_lockfile() {
    let e = env();
    let app = e.consumer("app");
    assert!(e.run(&app, &["add", "mathkit@1.0.0"]).status.success());
    let out = e.run(&app, &["update", "mathkit@2.0.0"]);
    assert!(out.status.success(), "{}", describe(&out));
    let lock = std::fs::read_to_string(app.join("beaver.lock")).unwrap();
    assert!(
        lock.contains("version = \"2.0.0\"") && lock.contains(&fixture_sha("mathkit", "2.0.0")),
        "{lock}"
    );
    let out = e.run(&app, &["remove", "mathkit"]);
    assert!(out.status.success(), "{}", describe(&out));
    let manifest = std::fs::read_to_string(app.join("beaver.toml")).unwrap();
    assert!(!manifest.contains("mathkit ="), "{manifest}");
    let lock = std::fs::read_to_string(app.join("beaver.lock")).unwrap();
    assert!(!lock.contains("mathkit"), "{lock}");
    let out = e.run(&app, &["remove", "mathkit"]);
    assert!(!out.status.success() && text(&out.stderr).contains("not a direct dependency"));
}

#[test]
fn transitive_dependencies_are_locked() {
    let e = env();
    let app = e.consumer("app");
    let out = e.run(&app, &["add", "requires_a@1.0.0"]);
    assert!(out.status.success(), "{}", describe(&out));
    let lock = std::fs::read_to_string(app.join("beaver.lock")).unwrap();
    assert!(
        lock.contains("name = \"requires_a\"")
            && lock.contains("dependencies = [\"mathkit@1.0.0\"]"),
        "{lock}"
    );
    assert!(
        lock.find("name = \"mathkit\"").unwrap() < lock.find("name = \"requires_a\"").unwrap(),
        "unsorted: {lock}"
    );
}

#[test]
fn corrupted_checksum_is_rejected_and_nothing_is_installed() {
    let e = env();
    e.add_fails("badchecksum@1.0.0", "error[B6035]: checksum mismatch");
    let sources = e.cache.join("sources");
    assert!(
        !sources.exists() || std::fs::read_dir(&sources).unwrap().next().is_none(),
        "something was extracted"
    );
    let archives = e.cache.join("archives");
    assert!(
        !archives.exists() || std::fs::read_dir(&archives).unwrap().next().is_none(),
        "bad archive cached"
    );
}

#[test]
fn path_traversal_archive_is_rejected_and_sentinel_untouched() {
    let e = env();
    // Where `pathtrap-1.0.0/../../outside.txt` would land relative to the
    // extraction staging directory, and one level further up.
    std::fs::create_dir_all(e.cache.join("sources")).unwrap();
    let sentinels = [
        e.cache.join("sources/outside.txt"),
        e.cache.join("outside.txt"),
        e.root.join("outside.txt"),
    ];
    for s in &sentinels {
        std::fs::write(s, "sentinel").unwrap();
    }
    e.add_fails("pathtrap@1.0.0", "path traversal");
    for s in &sentinels {
        assert_eq!(
            std::fs::read_to_string(s).unwrap(),
            "sentinel",
            "{} modified",
            s.display()
        );
    }
}

#[test]
fn symlink_hardlink_absolute_and_scripted_archives_are_rejected() {
    let e = env();
    e.add_fails("linktrap@1.0.0", "symbolic link");
    e.add_fails("reltrap@1.0.0", "symbolic link");
    e.add_fails("hardtrap@1.0.0", "hard link");
    e.add_fails("abstrap@1.0.0", "absolute path");
    assert!(!Path::new("/tmp/beaver-abs-escape.bv").exists());
    e.add_fails("scripted@1.0.0", "is not allowed in a Beaver package");
    e.add_fails("casedup@1.0.0", "case-colliding");
}

#[test]
fn corrupt_gzip_and_size_violations_are_rejected() {
    let e = env();
    e.add_fails("badgzip@1.0.0", "corrupt package archive");
    e.add_fails("bomb@1.0.0", "above the 10485760 byte limit");
    e.add_fails("bigmeta@1.0.0", "error[B6024]");
    e.add_fails("sizelie@1.0.0", "error[B6034]");
}

#[test]
fn metadata_that_disagrees_with_the_archive_is_rejected() {
    let e = env();
    e.add_fails("depslie@1.0.0", "disagree about its dependencies");
    e.add_fails("wrongname@1.0.0", "contains package othername@1.0.0");
}

#[test]
fn metadata_cannot_redirect_downloads_to_another_origin() {
    let e = env();
    e.add_fails("evilurl@1.0.0", "error[B6026]");
    assert!(
        e.server
            .requests()
            .iter()
            .all(|r| r.starts_with("/v1/packages/evilurl/")),
        "{:?}",
        e.server.requests()
    );
    let app = e.consumer("app_redirect");
    let out = beaver_cmd(
        &app,
        &e.cache,
        &[
            "add",
            "mathkit@1.0.0",
            "--registry-url",
            &format!("{}/redirect", e.server.url()),
        ],
    )
    .env("BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY", "1")
    .output()
    .unwrap();
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("error[B6022]"),
        "{}",
        describe(&out)
    );
    assert!(
        !e.server
            .requests()
            .contains(&"/v1/packages/mathkit/1.0.0/metadata.json".to_string()),
        "redirect was followed"
    );
}

#[test]
fn dependency_cycles_and_version_conflicts_fail_cleanly() {
    let e = env();
    let start = std::time::Instant::now();
    e.add_fails(
        "cycle_a@1.0.0",
        "dependency cycle detected: cycle_a@1.0.0 -> cycle_b@1.0.0 -> cycle_a@1.0.0",
    );
    e.add_fails(
        "conflict_root@1.0.0",
        "conflicting requirements for package `mathkit`",
    );
    let app = e.consumer("app_conflict_detail");
    let out = e.run(&app, &["add", "conflict_root@1.0.0"]);
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("requires_a@1.0.0 -> mathkit@1.0.0")
            && stderr.contains("requires_b@1.0.0 -> mathkit@2.0.0"),
        "{stderr}"
    );
    assert!(start.elapsed().as_secs() < 60, "resolution took too long");
}

#[test]
fn insecure_or_unconfigured_registries_are_rejected_without_requests() {
    let e = env();
    let app = e.consumer("app");
    let before = e.server.requests().len();
    for url in [
        "http://example.com",
        "http://10.1.2.3:8765",
        "http://localhost:1",
    ] {
        let out = beaver_cmd(
            &app,
            &e.cache,
            &["add", "mathkit@1.0.0", "--registry-url", url],
        )
        .env("BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY", "1")
        .output()
        .unwrap();
        assert!(!out.status.success());
        assert!(
            text(&out.stderr).contains("error[B6020]"),
            "{url}: {}",
            describe(&out)
        );
    }
    // Loopback HTTP without the explicit opt-in.
    let out = beaver_cmd(
        &app,
        &e.cache,
        &["add", "mathkit@1.0.0", "--registry-url", &e.server.url()],
    )
    .output()
    .unwrap();
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains(super_opt_in()),
        "{}",
        describe(&out)
    );
    // No registry at all.
    let out = beaver_cmd(&app, &e.cache, &["add", "mathkit@1.0.0"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("error[B6014]"),
        "{}",
        describe(&out)
    );
    assert_eq!(e.server.requests().len(), before);
    assert!(!app.join("beaver.lock").exists());
}

fn super_opt_in() -> &'static str {
    "BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1"
}

#[test]
fn missing_packages_report_helpfully() {
    let e = env();
    e.add_fails("nosuchpkg@1.0.0", "was not found in registry");
    e.add_fails("mathkit@9.9.9", "only resolves exact versions");
}

#[test]
fn tampered_lockfile_checksum_is_detected() {
    let e = env();
    let app = e.consumer("app");
    assert!(e.run(&app, &["add", "mathkit@1.0.0"]).status.success());
    let lock = std::fs::read_to_string(app.join("beaver.lock")).unwrap();
    let real = fixture_sha("mathkit", "1.0.0");
    std::fs::write(
        app.join("beaver.lock"),
        lock.replace(&real, &"1".repeat(64)),
    )
    .unwrap();
    let out = e.run(&app, &["fetch"]);
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("error[B6012]"),
        "{}",
        describe(&out)
    );
}

#[test]
fn corrupted_cache_entries_are_detected_and_refetched() {
    let e = env();
    let app = e.consumer("app");
    assert!(e.run(&app, &["add", "mathkit@1.0.0"]).status.success());
    let archive = e
        .cache
        .join("archives")
        .join(format!("{}.tar.gz", fixture_sha("mathkit", "1.0.0")));
    std::fs::write(&archive, b"tampered").unwrap();
    // Offline: the corrupted entry is not trusted.
    let out = e.run(&app, &["run", "--offline", "--locked"]);
    assert!(
        !out.status.success() && text(&out.stderr).contains("error[B6013]"),
        "{}",
        describe(&out)
    );
    // Online: it is downloaded again and verified.
    let out = e.run(&app, &["run", "--locked"]);
    assert!(out.status.success(), "{}", describe(&out));
    assert_eq!(text(&out.stdout), "49\n");
}

#[test]
fn registry_fixture_is_intact() {
    let e = env();
    assert!(e
        .registry
        .join("v1/packages/mathkit/1.0.0/archive.tar.gz")
        .is_file());
}
