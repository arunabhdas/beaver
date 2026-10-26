//! Module discovery and import resolution across packages.

use beaver_source::{Diagnostic, FileId, SourceMap, Span};
use beaver_syntax::ast;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// One package taking part in a compilation.
#[derive(Debug, Clone)]
pub struct PackageInput {
    pub name: String,
    pub dir: PathBuf,
    /// Prefix for user-facing paths (`""` for the root package).
    pub display_prefix: String,
    /// Entry file relative to `dir`, e.g. `src/main.bv`.
    pub entry: PathBuf,
    /// Direct dependencies: import name -> index into `ProgramInput::packages`.
    pub deps: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramKind {
    /// Binary package: requires `fn main()` in the entry module.
    Bin,
    /// Library: entry module checked, no `main` required.
    Lib,
    /// Test driver over the given test files.
    Tests,
}

#[derive(Debug, Clone)]
pub struct ProgramInput {
    pub packages: Vec<PackageInput>,
    pub root: usize,
    pub kind: ProgramKind,
    /// Test files relative to the root package dir (e.g. `tests/smoke.bv`).
    pub test_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModKey {
    Src(Vec<String>),
    Test(String),
}

pub struct Import {
    pub alias: String,
    pub target: usize,
    pub span: Span,
}

pub struct LoadedModule {
    pub package: usize,
    pub key: ModKey,
    pub file: FileId,
    pub ast: ast::Module,
    pub imports: Vec<Import>,
    /// e.g. `main`, `net.http`, `tests/smoke`
    pub display_name: String,
}

pub struct Loaded {
    pub sources: SourceMap,
    pub modules: Vec<LoadedModule>,
    /// Module index of the root entry (for Bin/Lib).
    pub entry_module: Option<usize>,
}

const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;

pub fn entry_segments(entry: &Path) -> Option<Vec<String>> {
    let rel = entry.strip_prefix("src").ok()?;
    let mut segs = Vec::new();
    let comps: Vec<_> = rel.components().collect();
    for (i, c) in comps.iter().enumerate() {
        let s = c.as_os_str().to_str()?;
        let s = if i + 1 == comps.len() {
            s.strip_suffix(".bv")?
        } else {
            s
        };
        if !is_ident(s) {
            return None;
        }
        segs.push(s.to_string());
    }
    if segs.is_empty() {
        None
    } else {
        Some(segs)
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && beaver_syntax::token::Tok::keyword(s).is_none()
}

pub fn load(input: &ProgramInput) -> Result<Loaded, (SourceMap, Vec<Diagnostic>)> {
    let mut l = Loader {
        input,
        sources: SourceMap::new(),
        modules: Vec::new(),
        by_key: HashMap::new(),
        errors: Vec::new(),
        queue: Vec::new(),
    };
    let root = &input.packages[input.root];
    let mut entry_module = None;
    match input.kind {
        ProgramKind::Bin | ProgramKind::Lib => match entry_segments(&root.entry) {
            Some(segs) => entry_module = l.request(input.root, ModKey::Src(segs), None),
            None => l.errors.push(Diagnostic::global(
                "B2014",
                format!("invalid entry path `{}`", root.entry.display()),
            )),
        },
        ProgramKind::Tests => {
            for tf in &input.test_files {
                let stem = tf
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("test")
                    .to_string();
                l.request(input.root, ModKey::Test(stem), None);
            }
        }
    }
    while let Some(m) = l.queue.pop() {
        l.resolve_imports(m);
    }
    if l.errors.is_empty() {
        l.check_cycles();
    }
    if l.errors.is_empty() {
        Ok(Loaded {
            sources: l.sources,
            modules: l.modules,
            entry_module,
        })
    } else {
        Err((l.sources, l.errors))
    }
}

struct Loader<'a> {
    input: &'a ProgramInput,
    sources: SourceMap,
    modules: Vec<LoadedModule>,
    by_key: HashMap<(usize, ModKey), Option<usize>>,
    errors: Vec<Diagnostic>,
    queue: Vec<usize>,
}

impl<'a> Loader<'a> {
    fn module_path(&self, pkg: usize, key: &ModKey) -> PathBuf {
        let p = &self.input.packages[pkg];
        match key {
            ModKey::Src(segs) => {
                let mut path = p.dir.join("src");
                for (i, s) in segs.iter().enumerate() {
                    if i + 1 == segs.len() {
                        path.push(format!("{s}.bv"));
                    } else {
                        path.push(s);
                    }
                }
                path
            }
            ModKey::Test(stem) => p.dir.join("tests").join(format!("{stem}.bv")),
        }
    }

    /// Load (once) and return the module index.
    fn request(&mut self, pkg: usize, key: ModKey, from: Option<Span>) -> Option<usize> {
        if let Some(existing) = self.by_key.get(&(pkg, key.clone())) {
            return *existing;
        }
        let path = self.module_path(pkg, &key);
        let p = &self.input.packages[pkg];
        let rel = path.strip_prefix(&p.dir).unwrap_or(&path).to_path_buf();
        let rel_display = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let display = if p.display_prefix.is_empty() {
            rel_display.clone()
        } else {
            format!("{}/{}", p.display_prefix, rel_display)
        };
        let module_name = match &key {
            ModKey::Src(segs) if pkg != self.input.root => {
                if entry_segments(&p.entry).as_ref() == Some(segs) {
                    p.name.clone()
                } else {
                    format!("{}.{}", p.name, segs.join("."))
                }
            }
            ModKey::Src(segs) => segs.join("."),
            ModKey::Test(stem) => format!("tests/{stem}"),
        };
        let fail = |this: &mut Self, msg: String| {
            let d = match from {
                Some(span) => Diagnostic::error("B2014", msg, span),
                None => Diagnostic::global("B2014", msg),
            };
            this.errors.push(d);
            this.by_key.insert((pkg, key.clone()), None);
            None
        };
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                return fail(
                    self,
                    format!("module `{module_name}` not found (expected file `{display}`)"),
                )
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            return fail(
                self,
                format!(
                    "module file `{display}` must be a regular file (symlinks are not followed)"
                ),
            );
        }
        if meta.len() > MAX_SOURCE_BYTES {
            return fail(self, format!("module file `{display}` is too large"));
        }
        // Reject paths whose canonical form escapes the package (e.g. via a
        // symlinked `src/` directory).
        if let (Ok(canon), Ok(root)) = (path.canonicalize(), p.dir.canonicalize()) {
            if !canon.starts_with(&root) {
                return fail(
                    self,
                    format!("module file `{display}` resolves outside its package"),
                );
            }
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return fail(self, format!("cannot read `{display}`: {e}")),
        };
        let file = match self.sources.add_bytes(display, path.clone(), &bytes) {
            Ok(f) => f,
            Err(d) => {
                self.errors.push(d);
                self.by_key.insert((pkg, key), None);
                return None;
            }
        };
        let ast = match beaver_syntax::parse_file(&self.sources, file) {
            Ok(m) => m,
            Err(ds) => {
                self.errors.extend(ds);
                self.by_key.insert((pkg, key), None);
                return None;
            }
        };
        let id = self.modules.len();
        self.modules.push(LoadedModule {
            package: pkg,
            key: key.clone(),
            file,
            ast,
            imports: Vec::new(),
            display_name: module_name,
        });
        self.by_key.insert((pkg, key), Some(id));
        self.queue.push(id);
        Some(id)
    }

    fn resolve_imports(&mut self, m: usize) {
        let pkg = self.modules[m].package;
        let uses: Vec<ast::UseDecl> = self.modules[m]
            .ast
            .items
            .iter()
            .filter_map(|i| match i {
                ast::Item::Use(u) => Some(u.clone()),
                _ => None,
            })
            .collect();
        let mut seen: HashMap<String, Span> = HashMap::new();
        let mut imports = Vec::new();
        for u in uses {
            let names: Vec<String> = u.path.iter().map(|i| i.name.clone()).collect();
            let alias = names.last().unwrap().clone();
            if let Some(prev) = seen.get(&alias) {
                let (_, line, _) = self.sources.location(*prev);
                self.errors.push(
                    Diagnostic::error(
                        "B2027",
                        format!(
                            "import name `{alias}` is already used by another `use` in this module"
                        ),
                        u.span,
                    )
                    .with_note(format!("previous import on line {line}")),
                );
                continue;
            }
            seen.insert(alias.clone(), u.span);
            let package = &self.input.packages[pkg];
            let (target_pkg, key) = if let Some(&dep) = package.deps.get(&names[0]) {
                let local_conflict = self
                    .module_path(pkg, &ModKey::Src(vec![names[0].clone()]))
                    .exists()
                    || package.dir.join("src").join(&names[0]).is_dir();
                if local_conflict {
                    self.errors.push(Diagnostic::error(
                        "B2014",
                        format!(
                            "`use {}` is ambiguous: `{}` is both a dependency and a module in this package",
                            names.join("."),
                            names[0]
                        ),
                        u.span,
                    ));
                    continue;
                }
                let dep_pkg = &self.input.packages[dep];
                let segs = if names.len() == 1 {
                    match entry_segments(&dep_pkg.entry) {
                        Some(s) => s,
                        None => {
                            self.errors.push(Diagnostic::error(
                                "B2014",
                                format!("dependency `{}` has an invalid entry path", names[0]),
                                u.span,
                            ));
                            continue;
                        }
                    }
                } else {
                    names[1..].to_vec()
                };
                (dep, ModKey::Src(segs))
            } else {
                (pkg, ModKey::Src(names.clone()))
            };
            if let Some(target) = self.request(target_pkg, key, Some(u.span)) {
                if target == m {
                    self.errors.push(Diagnostic::error(
                        "B2015",
                        "a module cannot import itself",
                        u.span,
                    ));
                    continue;
                }
                imports.push(Import {
                    alias,
                    target,
                    span: u.span,
                });
            }
        }
        self.modules[m].imports = imports;
    }

    fn check_cycles(&mut self) {
        // 0 = unvisited, 1 = on stack, 2 = done
        let mut state = vec![0u8; self.modules.len()];
        let mut stack = Vec::new();
        for start in 0..self.modules.len() {
            if state[start] == 0 && self.dfs(start, &mut state, &mut stack) {
                return;
            }
        }
    }

    fn dfs(&mut self, m: usize, state: &mut [u8], stack: &mut Vec<usize>) -> bool {
        state[m] = 1;
        stack.push(m);
        let edges: Vec<(usize, Span)> = self.modules[m]
            .imports
            .iter()
            .map(|i| (i.target, i.span))
            .collect();
        for (t, span) in edges {
            if state[t] == 1 {
                let pos = stack.iter().position(|x| *x == t).unwrap();
                let mut chain: Vec<String> = stack[pos..]
                    .iter()
                    .map(|x| self.modules[*x].display_name.clone())
                    .collect();
                chain.push(self.modules[t].display_name.clone());
                self.errors.push(
                    Diagnostic::error("B2015", "import cycle detected", span)
                        .with_note(format!("cycle: {}", chain.join(" -> "))),
                );
                return true;
            }
            if state[t] == 0 && self.dfs(t, state, stack) {
                return true;
            }
        }
        stack.pop();
        state[m] = 2;
        false
    }
}
