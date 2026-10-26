//! Project loading, dependency resolution and the compilation pipeline.

use beaver_codegen_c::{BuildRequest, Output};
use beaver_package::cache::Cache;
use beaver_package::lockfile::Lockfile;
use beaver_package::manifest::{load_manifest, Kind, Manifest, LOCKFILE, MANIFEST};
use beaver_package::registry::HttpTransport;
use beaver_package::resolve::{self, Resolution};
use beaver_package::{atomic_write, config};
use beaver_sema::{PackageInput, ProgramInput, ProgramKind};
use beaver_source::Diagnostic;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Errors carried to `main`: either plain diagnostics or rendered compiler
/// output (which already includes source snippets).
pub enum Failure {
    Diag(Diagnostic),
    Rendered(String),
}

impl From<Diagnostic> for Failure {
    fn from(d: Diagnostic) -> Failure {
        Failure::Diag(d)
    }
}

pub type CliResult<T> = Result<T, Failure>;

pub struct Project {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub fn find_project() -> Result<Project, Diagnostic> {
    let cwd = std::env::current_dir()
        .map_err(|e| Diagnostic::global("B6001", format!("cannot read current directory: {e}")))?;
    let mut dir = cwd.as_path();
    loop {
        if dir.join(MANIFEST).is_file() {
            let manifest = load_manifest(dir)?;
            return Ok(Project {
                dir: dir.to_path_buf(),
                manifest,
            });
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => break,
        }
    }
    Err(Diagnostic::global(
        "B6001",
        format!(
            "could not find `{MANIFEST}` in `{}` or any parent directory",
            cwd.display()
        ),
    )
    .with_help("run `beaver new NAME` to create a project, or `beaver init` here"))
}

#[derive(Clone, Copy, Default)]
pub struct DepFlags<'a> {
    pub offline: bool,
    pub locked: bool,
    pub registry_url: Option<&'a str>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// add/remove/update/fetch: the lockfile may be rewritten.
    Rewrite,
    /// check/build/run/test/package: create a missing lockfile, but refuse
    /// to silently change an existing one.
    Build,
}

pub fn read_lock(dir: &Path) -> Result<Option<Lockfile>, Diagnostic> {
    let p = dir.join(LOCKFILE);
    match std::fs::read_to_string(&p) {
        Ok(t) => Lockfile::parse(&t).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Diagnostic::global(
            "B6010",
            format!("cannot read `{}`: {e}", p.display()),
        )),
    }
}

/// Resolve the dependency graph without writing anything.
pub fn resolve_graph(
    dir: &Path,
    manifest: &Manifest,
    existing: Option<&Lockfile>,
    flags: DepFlags,
) -> Result<Resolution, Diagnostic> {
    if flags.locked && existing.is_none() && !manifest.deps.is_empty() {
        return Err(
            Diagnostic::global("B6011", "--locked requires an existing beaver.lock")
                .with_help("run `beaver fetch` first and commit beaver.lock"),
        );
    }
    if manifest.deps.is_empty() {
        return Ok(Resolution {
            packages: BTreeMap::new(),
            lock: Lockfile::default(),
        });
    }
    let cache = Cache::new(config::cache_dir());
    let factory =
        || -> Box<dyn beaver_package::registry::Transport> { Box::new(HttpTransport::new()) };
    resolve::resolve(
        dir,
        manifest,
        existing,
        &resolve::Options {
            offline: flags.offline,
            locked: flags.locked,
            registry_flag: flags.registry_url,
        },
        &cache,
        &factory,
    )
}

/// Resolve dependencies for `project` and reconcile `beaver.lock`.
pub fn resolve_project(
    project: &Project,
    flags: DepFlags,
    mode: LockMode,
) -> Result<Resolution, Diagnostic> {
    let existing = read_lock(&project.dir)?;
    let res = resolve_graph(&project.dir, &project.manifest, existing.as_ref(), flags)?;
    let changed = match &existing {
        Some(l) => *l != res.lock,
        None => !res.lock.packages.is_empty(),
    };
    if changed {
        if flags.locked {
            return Err(Diagnostic::global(
                "B6011",
                "--locked: beaver.lock is missing entries or out of date with beaver.toml",
            )
            .with_help("run `beaver fetch` without --locked to update beaver.lock"));
        }
        if mode == LockMode::Build && existing.is_some() {
            return Err(
                Diagnostic::global("B6011", "beaver.lock is out of date with beaver.toml")
                    .with_help("run `beaver fetch` (or `beaver update NAME@X.Y.Z`) to update it"),
            );
        }
        write_lock(&project.dir, &res.lock)?;
    }
    Ok(res)
}

pub fn write_lock(dir: &Path, lock: &Lockfile) -> Result<(), Diagnostic> {
    let p = dir.join(LOCKFILE);
    atomic_write(&p, lock.render().as_bytes())
        .map_err(|e| Diagnostic::global("B6010", format!("cannot write `{}`: {e}", p.display())))
}

fn program_input(
    project: &Project,
    res: &Resolution,
    kind: ProgramKind,
    test_files: Vec<PathBuf>,
) -> ProgramInput {
    let names: Vec<String> = res.packages.keys().cloned().collect();
    let index: BTreeMap<String, usize> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i + 1))
        .collect();
    let deps_of = |m: &Manifest| -> BTreeMap<String, usize> {
        m.deps.keys().map(|d| (d.clone(), index[d])).collect()
    };
    let mut packages = vec![PackageInput {
        name: project.manifest.name.clone(),
        dir: project.dir.clone(),
        display_prefix: String::new(),
        entry: project.manifest.entry.clone(),
        deps: deps_of(&project.manifest),
    }];
    for n in &names {
        let r = &res.packages[n];
        packages.push(PackageInput {
            name: r.name.clone(),
            dir: r.dir.clone(),
            display_prefix: r.display.clone(),
            entry: r.manifest.entry.clone(),
            deps: deps_of(&r.manifest),
        });
    }
    ProgramInput {
        packages,
        root: 0,
        kind,
        test_files,
    }
}

/// Type- and ownership-check the package's main program.
pub fn analyze(project: &Project, res: &Resolution) -> CliResult<beaver_sema::hir::Program> {
    let kind = match project.manifest.kind {
        Kind::Bin => ProgramKind::Bin,
        Kind::Lib => ProgramKind::Lib,
    };
    let input = program_input(project, res, kind, Vec::new());
    beaver_sema::analyze(&input).map_err(|f| Failure::Rendered(f.render()))
}

pub fn analyze_tests(
    project: &Project,
    res: &Resolution,
    files: Vec<PathBuf>,
) -> CliResult<beaver_sema::hir::Program> {
    let input = program_input(project, res, ProgramKind::Tests, files);
    beaver_sema::analyze(&input).map_err(|f| Failure::Rendered(f.render()))
}

pub fn profile_dir(project: &Project, release: bool) -> PathBuf {
    project
        .dir
        .join("target")
        .join(if release { "release" } else { "debug" })
}

pub fn native_build(
    project: &Project,
    program: &beaver_sema::hir::Program,
    output: Output,
    release: bool,
) -> CliResult<PathBuf> {
    let mir = beaver_mir::lower(program);
    let pdir = profile_dir(project, release);
    let name = &project.manifest.name;
    let (unit, out) = match output {
        Output::Executable => (name.clone(), pdir.join(exe_name(name))),
        Output::Tests => (
            format!("{name}_tests"),
            pdir.join(exe_name(&format!("{name}-tests"))),
        ),
        Output::Library => (name.clone(), pdir.join(format!("lib{name}.o"))),
    };
    let gen = pdir.join("generated");
    let report = beaver_codegen_c::build(&BuildRequest {
        mir: &mir,
        output,
        gen_dir: &gen,
        unit_name: &unit,
        out_path: &out,
        release,
    })?;
    if !report.warnings.trim().is_empty() {
        eprintln!(
            "warning[B4004]: the C compiler reported warnings for generated code (please report this):\n{}",
            report.warnings.trim_end()
        );
    }
    Ok(out)
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// `tests/*.bv`, sorted.
pub fn test_files(project: &Project) -> Vec<PathBuf> {
    let dir = project.dir.join("tests");
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let is_file = std::fs::symlink_metadata(&p)
                .map(|m| m.is_file())
                .unwrap_or(false);
            if is_file && p.extension().and_then(|x| x.to_str()) == Some("bv") {
                out.push(PathBuf::from("tests").join(e.file_name()));
            }
        }
    }
    out.sort();
    out
}
