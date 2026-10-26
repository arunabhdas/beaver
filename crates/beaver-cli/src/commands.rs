//! Implementations of the `beaver` subcommands.

use crate::driver::{self, CliResult, DepFlags, Failure, LockMode, Project};
use beaver_codegen_c::Output;
use beaver_package::manifest::{
    edit_dependency, load_manifest, parse_manifest, render_new, validate_name, DepSpec, Kind,
    Version, LOCKFILE, MANIFEST,
};
use beaver_package::{archive, atomic_write, sha256_hex};
use beaver_source::{Diagnostic, SourceMap};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn status(verb: &str, msg: impl AsRef<str>) {
    eprintln!("{verb:>12} {}", msg.as_ref());
}

fn io(what: &str, p: &Path, e: std::io::Error) -> Failure {
    Failure::Diag(Diagnostic::global(
        "B6002",
        format!("cannot {what} `{}`: {e}", p.display()),
    ))
}

// ----- new / init ---------------------------------------------------------

const MAIN_BV: &str = "fn main():\n    print(\"Hello, Beaver!\")\n";
const BIN_TEST: &str = "test \"arithmetic works\":\n    assert_eq(2 + 2, 4)\n";
const LIB_BV: &str = "pub fn add(a: Int, b: Int) -> Int:\n    return a + b\n";
const LIB_TEST: &str = "use lib\n\ntest \"add\":\n    assert_eq(lib.add(2, 3), 5)\n";
const GITIGNORE: &str = "/target/\n";

fn scaffold(dir: &Path, name: &str, kind: Kind) -> CliResult<()> {
    let files: Vec<(&str, String)> = vec![
        (MANIFEST, render_new(name, kind)),
        (
            if kind == Kind::Bin {
                "src/main.bv"
            } else {
                "src/lib.bv"
            },
            (if kind == Kind::Bin { MAIN_BV } else { LIB_BV }).to_string(),
        ),
        (
            "tests/smoke.bv",
            (if kind == Kind::Bin {
                BIN_TEST
            } else {
                LIB_TEST
            })
            .to_string(),
        ),
    ];
    for (rel, _) in &files {
        if dir.join(rel).exists() {
            return Err(Failure::Diag(Diagnostic::global(
                "B6003",
                format!(
                    "`{}` already exists; refusing to overwrite it",
                    dir.join(rel).display()
                ),
            )));
        }
    }
    for (rel, text) in &files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| io("create directory", &p, e))?;
        std::fs::write(&p, text).map_err(|e| io("write", &p, e))?;
    }
    let gi = dir.join(".gitignore");
    if !gi.exists() {
        std::fs::write(&gi, GITIGNORE).map_err(|e| io("write", &gi, e))?;
    }
    Ok(())
}

pub fn new(name: &str, lib: bool) -> CliResult<ExitCode> {
    validate_name(name).map_err(|m| Failure::Diag(Diagnostic::global("B6001", m)))?;
    let dir = PathBuf::from(name);
    if dir.exists() {
        return Err(Failure::Diag(
            Diagnostic::global("B6003", format!("destination `{name}` already exists")).with_help(
                "choose another name, or run `beaver init` inside an existing empty directory",
            ),
        ));
    }
    std::fs::create_dir(&dir).map_err(|e| io("create directory", &dir, e))?;
    let kind = if lib { Kind::Lib } else { Kind::Bin };
    scaffold(&dir, name, kind)?;
    status("Created", format!("{} package `{name}`", kind.as_str()));
    Ok(ExitCode::SUCCESS)
}

pub fn init(lib: bool) -> CliResult<ExitCode> {
    let dir = std::env::current_dir().map_err(|e| io("read", Path::new("."), e))?;
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    validate_name(&name).map_err(|m| {
        Failure::Diag(
            Diagnostic::global(
                "B6001",
                format!("the current directory name cannot be used as a package name: {m}"),
            )
            .with_help("rename the directory or use `beaver new NAME`"),
        )
    })?;
    let kind = if lib { Kind::Lib } else { Kind::Bin };
    scaffold(&dir, &name, kind)?;
    status(
        "Created",
        format!(
            "{} package `{name}` in the current directory",
            kind.as_str()
        ),
    );
    Ok(ExitCode::SUCCESS)
}

// ----- check / build / run / test -----------------------------------------

pub fn check(flags: DepFlags) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    let res = driver::resolve_project(&project, flags, LockMode::Build)?;
    driver::analyze(&project, &res)?;
    status(
        "Checked",
        format!(
            "{} {} (no errors)",
            project.manifest.name, project.manifest.version
        ),
    );
    Ok(ExitCode::SUCCESS)
}

fn build_project(project: &Project, release: bool, flags: DepFlags) -> CliResult<Option<PathBuf>> {
    let res = driver::resolve_project(project, flags, LockMode::Build)?;
    let program = driver::analyze(project, &res)?;
    let output = match project.manifest.kind {
        Kind::Bin => Output::Executable,
        Kind::Lib => Output::Library,
    };
    let out = driver::native_build(project, &program, output, release)?;
    let rel = out
        .strip_prefix(&project.dir)
        .unwrap_or(&out)
        .display()
        .to_string();
    let profile = if release { "release" } else { "debug" };
    match project.manifest.kind {
        Kind::Bin => {
            status(
                "Built",
                format!("{} [{profile}] -> {rel}", project.manifest.name),
            );
            Ok(Some(out))
        }
        Kind::Lib => {
            status(
                "Compiled",
                format!(
                    "library {} [{profile}] -> {rel} (libraries produce no executable)",
                    project.manifest.name
                ),
            );
            Ok(None)
        }
    }
}

pub fn build(release: bool, flags: DepFlags) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    build_project(&project, release, flags)?;
    Ok(ExitCode::SUCCESS)
}

fn exit_code_of(status: std::process::ExitStatus) -> ExitCode {
    if let Some(c) = status.code() {
        return ExitCode::from((c & 0xff) as u8);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return ExitCode::from((128 + sig).min(255) as u8);
        }
    }
    ExitCode::from(1)
}

pub fn run(release: bool, flags: DepFlags, args: &[String]) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    if project.manifest.kind == Kind::Lib {
        return Err(Failure::Diag(
            Diagnostic::global(
                "B6004",
                format!(
                    "`{}` is a library package and cannot be run",
                    project.manifest.name
                ),
            )
            .with_help(
                "use `beaver test` to run its tests, or `beaver build` to check that it compiles",
            ),
        ));
    }
    let exe = build_project(&project, release, flags)?.expect("binary package");
    let rel = exe
        .strip_prefix(&project.dir)
        .unwrap_or(&exe)
        .display()
        .to_string();
    status("Running", rel);
    let st = Command::new(&exe)
        .args(args)
        .status()
        .map_err(|e| io("run", &exe, e))?;
    Ok(exit_code_of(st))
}

pub fn test(release: bool, flags: DepFlags) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    let res = driver::resolve_project(&project, flags, LockMode::Build)?;
    // The package itself must compile.
    driver::analyze(&project, &res)?;
    let files = driver::test_files(&project);
    let program = driver::analyze_tests(&project, &res, files)?;
    let tests: Vec<(String, String)> = program
        .tests
        .iter()
        .map(|t| (t.file.clone(), t.name.clone()))
        .collect();
    let exe = driver::native_build(&project, &program, Output::Tests, release)?;
    eprintln!(
        "\nrunning {} test{}",
        tests.len(),
        if tests.len() == 1 { "" } else { "s" }
    );
    let mut failed = Vec::new();
    for (i, (file, name)) in tests.iter().enumerate() {
        let out = Command::new(&exe)
            .arg("--run")
            .arg(i.to_string())
            .output()
            .map_err(|e| io("run", &exe, e))?;
        if out.status.success() {
            eprintln!("test {file} :: {name} ... ok");
        } else {
            eprintln!("test {file} :: {name} ... FAILED");
            failed.push((file.clone(), name.clone(), out));
        }
    }
    if !failed.is_empty() {
        eprintln!("\nfailures:");
        for (file, name, out) in &failed {
            eprintln!("\n---- {file} :: {name} ----");
            let so = String::from_utf8_lossy(&out.stdout);
            if !so.trim().is_empty() {
                eprintln!("stdout:\n{}", so.trim_end());
            }
            eprintln!("{}", String::from_utf8_lossy(&out.stderr).trim_end());
            if out.status.code().is_none() {
                eprintln!("(test process terminated by a signal)");
            }
        }
    }
    let passed = tests.len() - failed.len();
    let verdict = if failed.is_empty() { "ok" } else { "FAILED" };
    eprintln!(
        "\ntest result: {verdict}. {passed} passed; {} failed",
        failed.len()
    );
    Ok(if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

// ----- fmt -----------------------------------------------------------------

fn bv_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let Ok(md) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if md.is_dir() {
            bv_files(&p, out);
        } else if md.is_file() && p.extension().and_then(|x| x.to_str()) == Some("bv") {
            out.push(p);
        }
    }
}

pub fn fmt(check: bool) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    let mut files = Vec::new();
    bv_files(&project.dir.join("src"), &mut files);
    bv_files(&project.dir.join("tests"), &mut files);
    let mut sm = SourceMap::new();
    let mut errors = String::new();
    let mut changed = Vec::new();
    for f in &files {
        let rel = f
            .strip_prefix(&project.dir)
            .unwrap_or(f)
            .display()
            .to_string();
        let bytes = std::fs::read(f).map_err(|e| io("read", f, e))?;
        let id = match sm.add_bytes(rel.clone(), f.clone(), &bytes) {
            Ok(id) => id,
            Err(d) => {
                errors.push_str(&d.render(Some(&sm)));
                continue;
            }
        };
        match beaver_syntax::fmt::format_source(id, sm.file(id)) {
            Ok(formatted) => {
                if formatted.as_bytes() != bytes.as_slice() {
                    changed.push((f.clone(), rel, formatted));
                }
            }
            Err(ds) => {
                for d in ds {
                    errors.push_str(&d.render(Some(&sm)));
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(Failure::Rendered(format!(
            "{errors}error: files with syntax errors were not formatted\n"
        )));
    }
    if check {
        for (_, rel, _) in &changed {
            eprintln!("Not formatted: {rel}");
        }
        if changed.is_empty() {
            status("Checked", format!("{} file(s) are formatted", files.len()));
            return Ok(ExitCode::SUCCESS);
        }
        eprintln!(
            "error: {} file(s) need formatting; run `beaver fmt`",
            changed.len()
        );
        return Ok(ExitCode::from(1));
    }
    for (path, rel, text) in &changed {
        atomic_write(path, text.as_bytes()).map_err(|e| io("write", path, e))?;
        status("Formatted", rel);
    }
    if changed.is_empty() {
        status(
            "Unchanged",
            format!("{} file(s) already formatted", files.len()),
        );
    }
    Ok(ExitCode::SUCCESS)
}

// ----- dependency management ---------------------------------------------

fn parse_spec(spec: &str) -> Result<(String, Version), Failure> {
    let Some((name, ver)) = spec.split_once('@') else {
        return Err(Failure::Diag(
            Diagnostic::global("B6001", format!("expected NAME@X.Y.Z, found `{spec}`")).with_help(
                "Beaver 0.1 dependencies use exact versions, e.g. `beaver add mathkit@1.0.0`",
            ),
        ));
    };
    validate_name(name).map_err(|m| Failure::Diag(Diagnostic::global("B6001", m)))?;
    let v = Version::parse(ver).map_err(|m| Failure::Diag(Diagnostic::global("B6001", m)))?;
    Ok((name.to_string(), v))
}

fn reject_locked(cmd: &str, locked: bool) -> CliResult<()> {
    if locked {
        return Err(Failure::Diag(Diagnostic::global(
            "B6011",
            format!("`--locked` cannot be used with `beaver {cmd}`, which must modify beaver.lock"),
        )));
    }
    Ok(())
}

/// Resolve with an edited manifest; only if that succeeds, write the
/// lockfile and the manifest (rolling the lockfile back if the manifest
/// write fails).
fn apply_manifest_change(
    project: &Project,
    new_text: &str,
    flags: DepFlags,
) -> CliResult<beaver_package::resolve::Resolution> {
    let mpath = project.dir.join(MANIFEST);
    let new_manifest = parse_manifest(new_text, &mpath.display().to_string())?;
    let existing = driver::read_lock(&project.dir)?;
    let res = driver::resolve_graph(&project.dir, &new_manifest, existing.as_ref(), flags)?;
    let lpath = project.dir.join(LOCKFILE);
    let old_lock = std::fs::read(&lpath).ok();
    let new_lock = res.lock.render();
    let lock_needed = !res.lock.packages.is_empty() || old_lock.is_some();
    if lock_needed {
        atomic_write(&lpath, new_lock.as_bytes()).map_err(|e| io("write", &lpath, e))?;
    }
    if let Err(e) = atomic_write(&mpath, new_text.as_bytes()) {
        match old_lock {
            Some(b) => {
                let _ = atomic_write(&lpath, &b);
            }
            None => {
                let _ = std::fs::remove_file(&lpath);
            }
        }
        return Err(io("write", &mpath, e));
    }
    Ok(res)
}

fn manifest_text(project: &Project) -> CliResult<String> {
    let p = project.dir.join(MANIFEST);
    std::fs::read_to_string(&p).map_err(|e| io("read", &p, e))
}

pub fn add(
    spec: Option<&str>,
    path: Option<&str>,
    registry: Option<&str>,
    offline: bool,
    locked: bool,
) -> CliResult<ExitCode> {
    reject_locked("add", locked)?;
    let project = driver::find_project()?;
    let (name, dep) = match (spec, path) {
        (Some(s), None) => {
            let (n, v) = parse_spec(s)?;
            (n, DepSpec::Registry(v))
        }
        (None, Some(p)) => {
            let dir = project.dir.join(p);
            let m = load_manifest(&dir).map_err(|d| {
                Failure::Diag(
                    Diagnostic::global("B6015", format!("cannot add path dependency `{p}`"))
                        .with_note(d.message),
                )
            })?;
            if m.kind != Kind::Lib {
                return Err(Failure::Diag(Diagnostic::global(
                    "B6031",
                    format!(
                        "`{p}` contains binary package `{}`; only libraries can be dependencies",
                        m.name
                    ),
                )));
            }
            (m.name, DepSpec::Path(p.replace('\\', "/")))
        }
        _ => {
            return Err(Failure::Diag(
                Diagnostic::global(
                    "B6001",
                    "specify a dependency as NAME@X.Y.Z or with --path PATH",
                )
                .with_help("examples: `beaver add mathkit@1.0.0`, `beaver add --path ../mathkit`"),
            ))
        }
    };
    if name == project.manifest.name {
        return Err(Failure::Diag(Diagnostic::global(
            "B6001",
            format!("package `{name}` cannot depend on itself"),
        )));
    }
    if project.manifest.deps.contains_key(&name) {
        return Err(Failure::Diag(
            Diagnostic::global("B6005", format!("`{name}` is already a dependency"))
                .with_help(format!("use `beaver update {name}@X.Y.Z` to change its version, or `beaver remove {name}` first")),
        ));
    }
    let text = manifest_text(&project)?;
    let new_text = edit_dependency(&text, &name, Some(&dep))?;
    let flags = DepFlags {
        offline,
        locked: false,
        registry_url: registry,
    };
    let res = apply_manifest_change(&project, &new_text, flags)?;
    let r = &res.packages[&name];
    match &r.checksum {
        Some(c) => status("Added", format!("{name}@{} (sha256 {c})", r.version)),
        None => status(
            "Added",
            format!("{name} {} (path {})", r.version, r.display),
        ),
    }
    status(
        "Locked",
        format!("{} package(s) in {LOCKFILE}", res.lock.packages.len()),
    );
    Ok(ExitCode::SUCCESS)
}

pub fn remove(name: &str, offline: bool, locked: bool) -> CliResult<ExitCode> {
    reject_locked("remove", locked)?;
    let project = driver::find_project()?;
    if !project.manifest.deps.contains_key(name) {
        return Err(Failure::Diag(Diagnostic::global(
            "B6006",
            format!(
                "`{name}` is not a direct dependency of `{}`",
                project.manifest.name
            ),
        )));
    }
    let text = manifest_text(&project)?;
    let new_text = edit_dependency(&text, name, None)?;
    let res = apply_manifest_change(
        &project,
        &new_text,
        DepFlags {
            offline,
            locked: false,
            registry_url: None,
        },
    )?;
    status("Removed", name);
    status(
        "Locked",
        format!("{} package(s) in {LOCKFILE}", res.lock.packages.len()),
    );
    Ok(ExitCode::SUCCESS)
}

pub fn update(
    spec: &str,
    registry: Option<&str>,
    offline: bool,
    locked: bool,
) -> CliResult<ExitCode> {
    reject_locked("update", locked)?;
    let project = driver::find_project()?;
    let (name, v) = parse_spec(spec)?;
    match project.manifest.deps.get(&name) {
        Some(DepSpec::Registry(_)) => {}
        Some(DepSpec::Path(_)) => {
            return Err(Failure::Diag(Diagnostic::global(
                "B6006",
                format!("`{name}` is a path dependency; edit beaver.toml or remove it and add a registry version"),
            )))
        }
        None => {
            return Err(Failure::Diag(
                Diagnostic::global("B6006", format!("`{name}` is not a direct dependency"))
                    .with_help(format!("use `beaver add {spec}` to add it")),
            ))
        }
    }
    let text = manifest_text(&project)?;
    let new_text = edit_dependency(&text, &name, Some(&DepSpec::Registry(v)))?;
    let res = apply_manifest_change(
        &project,
        &new_text,
        DepFlags {
            offline,
            locked: false,
            registry_url: registry,
        },
    )?;
    status("Updated", format!("{name} to {v}"));
    status(
        "Locked",
        format!("{} package(s) in {LOCKFILE}", res.lock.packages.len()),
    );
    Ok(ExitCode::SUCCESS)
}

pub fn fetch(flags: DepFlags) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    let res = driver::resolve_project(&project, flags, LockMode::Rewrite)?;
    for r in res.packages.values() {
        match &r.checksum {
            Some(c) => status("Fetched", format!("{}@{} (sha256 {c})", r.name, r.version)),
            None => status(
                "Using",
                format!("{} {} (path {})", r.name, r.version, r.display),
            ),
        }
    }
    status(
        "Ready",
        format!(
            "{} dependenc{}",
            res.packages.len(),
            if res.packages.len() == 1 { "y" } else { "ies" }
        ),
    );
    Ok(ExitCode::SUCCESS)
}

// ----- package -------------------------------------------------------------

pub fn package(output: Option<&str>, flags: DepFlags) -> CliResult<ExitCode> {
    let project = driver::find_project()?;
    let m = &project.manifest;
    if m.kind != Kind::Lib {
        return Err(Failure::Diag(
            Diagnostic::global(
                "B6060",
                format!(
                    "`{}` is a binary package; only libraries can be packaged",
                    m.name
                ),
            )
            .with_note("package archives are Beaver source libraries for use as dependencies"),
        ));
    }
    if let Some((d, _)) = m.deps.iter().find(|(_, s)| matches!(s, DepSpec::Path(_))) {
        return Err(Failure::Diag(
            Diagnostic::global("B6061", format!("cannot package `{}`: dependency `{d}` is a path dependency", m.name))
                .with_note("registry packages must be reproducible elsewhere, so they may only use exact registry versions"),
        ));
    }
    let res = driver::resolve_project(&project, flags, LockMode::Build)?;
    driver::analyze(&project, &res)?;
    let bytes = archive::create_archive(&project.dir, m)?;
    let out_dir = match output {
        Some(o) => PathBuf::from(o),
        None => project.dir.join("target").join("package"),
    };
    let file = out_dir.join(format!("{}-{}.tar.gz", m.name, m.version));
    atomic_write(&file, &bytes).map_err(|e| io("write", &file, e))?;
    // Self-check: the archive must pass the same validation as downloads.
    let verify = out_dir.join(format!(".verify-{}", std::process::id()));
    let checked = archive::extract(
        &file,
        &m.name,
        &m.version.to_string(),
        &verify,
        archive::Limits::default(),
    );
    let _ = std::fs::remove_dir_all(&verify);
    checked?;
    let sha = sha256_hex(&bytes);
    status(
        "Packaged",
        format!("{} ({} bytes)", file.display(), bytes.len()),
    );
    println!("{}", file.display());
    println!("sha256 {sha}");
    Ok(ExitCode::SUCCESS)
}
