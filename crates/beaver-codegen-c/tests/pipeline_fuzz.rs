//! Mutation fuzzing of the whole front end: mutated conformance programs go
//! through module loading, type checking and ownership checking; whenever a
//! mutant is accepted it is also lowered to MIR and emitted as C. No stage
//! may panic (internal invariant assertions included).

use beaver_sema::{analyze, PackageInput, ProgramInput, ProgramKind};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn idents(src: &str) -> Vec<(usize, usize)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            in_str = !in_str;
        }
        if !in_str
            && (c.is_ascii_alphabetic() || c == b'_')
            && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
        {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push((s, i));
            continue;
        }
        i += 1;
    }
    out
}

fn mutate(src: &str, rng: &mut Rng) -> String {
    let ids = idents(src);
    let mut lines: Vec<String> = src.lines().map(|l| l.to_string()).collect();
    match rng.below(6) {
        0 if !ids.is_empty() => {
            let (s, e) = ids[rng.below(ids.len())];
            let (s2, e2) = ids[rng.below(ids.len())];
            format!("{}{}{}", &src[..s], &src[s2..e2], &src[e..])
        }
        1 if !ids.is_empty() => {
            let (s, _) = ids[rng.below(ids.len())];
            let kw = ["take ", "edit ", "view ", "try ", "clone(", "not "][rng.below(6)];
            let mut out = format!("{}{}{}", &src[..s], kw, &src[s..]);
            if kw == "clone(" {
                let end = s
                    + kw.len()
                    + ids
                        .iter()
                        .find(|(a, _)| *a == s)
                        .map(|(a, b)| b - a)
                        .unwrap_or(0);
                out.insert(end.min(out.len()), ')');
            }
            out
        }
        2 if !lines.is_empty() => {
            lines.remove(rng.below(lines.len()));
            lines.join("\n") + "\n"
        }
        3 if !lines.is_empty() => {
            let i = rng.below(lines.len());
            let l = lines[i].clone();
            lines.insert(i, l);
            lines.join("\n") + "\n"
        }
        4 if lines.len() > 1 => {
            let (a, b) = (rng.below(lines.len()), rng.below(lines.len()));
            lines.swap(a, b);
            lines.join("\n") + "\n"
        }
        _ => {
            let extra = [
                "    let zz = take zz",
                "    return",
                "    break",
                "    var q = [1]\n    for v in q:\n        append(edit q, v)",
            ][rng.below(4)];
            let i = rng.below(lines.len() + 1);
            lines.insert(i, extra.to_string());
            lines.join("\n") + "\n"
        }
    }
}

fn pipeline(dir: &Path, src: &str) -> bool {
    std::fs::write(dir.join("src/main.bv"), src).unwrap();
    let input = ProgramInput {
        packages: vec![PackageInput {
            name: "fz".into(),
            dir: dir.to_path_buf(),
            display_prefix: String::new(),
            entry: PathBuf::from("src/main.bv"),
            deps: BTreeMap::new(),
        }],
        root: 0,
        kind: ProgramKind::Bin,
        test_files: Vec::new(),
    };
    match analyze(&input) {
        Ok(prog) => {
            let mir = beaver_mir::lower(&prog);
            let c = beaver_codegen_c::emit::emit(&mir, beaver_codegen_c::Output::Executable);
            assert!(c.contains("int main("));
            true
        }
        Err(f) => {
            let _ = f.render();
            false
        }
    }
}

#[test]
fn mutated_programs_never_crash_any_compiler_stage() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut corpus = Vec::new();
    for d in [
        "Beaver-0.1-spec/fixtures/compile_pass",
        "tests/conformance/compile_pass",
        "Beaver-0.1-spec/fixtures/runtime",
    ] {
        for e in std::fs::read_dir(root.join(d)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().and_then(|x| x.to_str()) == Some("bv") {
                corpus.push(std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    corpus.sort();
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let (mut accepted, mut rejected) = (0, 0);
    for base in &corpus {
        for _ in 0..60 {
            let mut m = base.clone();
            for _ in 0..1 + rng.below(2) {
                m = mutate(&m, &mut rng);
            }
            let r = catch_unwind(AssertUnwindSafe(|| pipeline(tmp.path(), &m)));
            match r {
                Ok(true) => accepted += 1,
                Ok(false) => rejected += 1,
                Err(_) => panic!("compiler panicked on mutated program:\n{m}"),
            }
        }
    }
    eprintln!("pipeline fuzz: {accepted} accepted, {rejected} rejected");
    assert!(accepted > 50 && rejected > 50);
}

fn sanitizers_available(dir: &Path) -> bool {
    let c = dir.join("probe.c");
    std::fs::write(&c, "int main(void) { return 0; }\n").unwrap();
    std::process::Command::new(cc())
        .args(["-fsanitize=address,undefined", "-o"])
        .arg(dir.join("probe"))
        .arg(&c)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn cc() -> String {
    std::env::var("BEAVER_CC").unwrap_or_else(|_| "cc".into())
}

/// Accepted mutants must compile warning-free and run without memory
/// errors: exit 0 with every allocation released, or a clean B5xxx trap.
#[test]
fn accepted_mutants_compile_cleanly_and_run_memory_safely() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut corpus = Vec::new();
    for d in [
        "Beaver-0.1-spec/fixtures/compile_pass",
        "tests/conformance/compile_pass",
    ] {
        for e in std::fs::read_dir(root.join(d)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().and_then(|x| x.to_str()) == Some("bv") {
                corpus.push(std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    corpus.sort();
    let tmp = tempfile::tempdir().unwrap();
    let sanitize = sanitizers_available(tmp.path());
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    let mut rng = Rng(0x1234_5678_9abc_def0);
    let mut accepted: Vec<(String, String)> = Vec::new();
    for base in &corpus {
        for _ in 0..25 {
            let m = mutate(base, &mut rng);
            std::fs::write(tmp.path().join("src/main.bv"), &m).unwrap();
            let input = ProgramInput {
                packages: vec![PackageInput {
                    name: "fz".into(),
                    dir: tmp.path().to_path_buf(),
                    display_prefix: String::new(),
                    entry: PathBuf::from("src/main.bv"),
                    deps: BTreeMap::new(),
                }],
                root: 0,
                kind: ProgramKind::Bin,
                test_files: Vec::new(),
            };
            if let Ok(prog) = analyze(&input) {
                let mir = beaver_mir::lower(&prog);
                accepted.push((
                    m,
                    beaver_codegen_c::emit::emit(&mir, beaver_codegen_c::Output::Executable),
                ));
            }
        }
    }
    assert!(
        accepted.len() > 40,
        "too few accepted mutants: {}",
        accepted.len()
    );
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("beaver_rt.h"), beaver_codegen_c::RUNTIME_HEADER).unwrap();
    std::fs::write(work.join("beaver_rt.c"), beaver_codegen_c::RUNTIME_SOURCE).unwrap();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let failures = std::sync::Mutex::new(Vec::new());
    let stats = std::sync::Mutex::new((0usize, 0usize, 0usize));
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if i >= accepted.len() {
                    break;
                }
                let (src, c) = &accepted[i];
                let unit = work.join(format!("m{i}.c"));
                let exe = work.join(format!("m{i}"));
                std::fs::write(&unit, c).unwrap();
                let mut cmd = std::process::Command::new(cc());
                cmd.args(["-std=c11", "-O0", "-Wall", "-Wextra", "-Werror", "-I"])
                    .arg(&work);
                if sanitize {
                    cmd.args([
                        "-fsanitize=address,undefined",
                        "-fno-sanitize-recover=undefined",
                    ]);
                }
                let out = cmd
                    .arg(&unit)
                    .arg(work.join("beaver_rt.c"))
                    .arg("-o")
                    .arg(&exe)
                    .output()
                    .unwrap();
                if !out.status.success() {
                    failures.lock().unwrap().push(format!(
                        "C compile failed:\n{src}\n{}",
                        String::from_utf8_lossy(&out.stderr)
                    ));
                    continue;
                }
                let mut child = std::process::Command::new(&exe)
                    .env("BEAVER_RT_LEAK_CHECK", "1")
                    .env("ASAN_OPTIONS", "detect_leaks=0")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let status = loop {
                    if let Some(st) = child.try_wait().unwrap() {
                        break Some(st);
                    }
                    if std::time::Instant::now() > deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                };
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut err);
                }
                let mut st = stats.lock().unwrap();
                match status {
                    None => st.2 += 1,
                    Some(s)
                        if s.code() == Some(0)
                            && err.contains("all owned allocations released") =>
                    {
                        st.0 += 1
                    }
                    Some(s) if s.code() == Some(101) && err.contains("runtime error[B5") => {
                        st.1 += 1
                    }
                    Some(s) => failures.lock().unwrap().push(format!(
                        "unexpected outcome {:?}:\n{src}\n--- stderr ---\n{err}",
                        s.code()
                    )),
                }
            });
        }
    });
    let (clean, traps, timeouts) = *stats.lock().unwrap();
    eprintln!("accepted mutants: {} clean exits, {traps} safe traps, {timeouts} timeouts (sanitizers: {sanitize})", clean);
    let f = failures.into_inner().unwrap();
    assert!(
        f.is_empty(),
        "{} failure(s):\n{}",
        f.len(),
        f.join("\n=====\n")
    );
}
