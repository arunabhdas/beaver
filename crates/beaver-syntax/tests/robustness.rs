//! The lexer, parser and formatter must never panic, whatever the input,
//! and formatting must be idempotent on every conformance program.

use beaver_source::SourceMap;
use std::path::{Path, PathBuf};

/// Small deterministic xorshift PRNG (no external dependencies).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in [
        "Beaver-0.1-spec/fixtures/compile_pass",
        "Beaver-0.1-spec/fixtures/compile_fail",
        "Beaver-0.1-spec/fixtures/runtime",
        "tests/conformance/compile_pass",
        "tests/conformance/compile_fail",
        "tests/conformance/runtime",
    ] {
        for e in std::fs::read_dir(root.join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().and_then(|x| x.to_str()) == Some("bv") {
                files.push(p);
            }
        }
    }
    files.sort();
    files
}

fn run_all_stages(bytes: &[u8]) {
    let mut sm = SourceMap::new();
    if let Ok(id) = sm.add_bytes("f.bv", PathBuf::new(), bytes) {
        let _ = beaver_syntax::parse_file(&sm, id);
        let _ = beaver_syntax::fmt::format_source(id, sm.file(id));
    }
}

#[test]
fn mutated_programs_never_panic() {
    let pieces: &[&[u8]] = &[
        b"(",
        b")",
        b"[",
        b"]",
        b":",
        b"\n",
        b"    ",
        b"\t",
        b"\"",
        b"\\",
        b"#",
        b"take ",
        b"edit ",
        b"view ",
        b"try ",
        b"fn ",
        b"if ",
        b"else:",
        b"=",
        b"==",
        b"->",
        b"9999999999999999999999",
        b"1.",
        b".5",
        b"\xff",
        b"\xe2\x98",
        b"\r",
        b"\0",
        b"\\u{",
        b"elif",
        b"type ",
        b"test ",
    ];
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for f in &corpus() {
        let base = std::fs::read(f).unwrap();
        run_all_stages(&base);
        for _ in 0..40 {
            let mut m = base.clone();
            for _ in 0..1 + rng.below(4) {
                let pos = if m.is_empty() { 0 } else { rng.below(m.len()) };
                match rng.below(3) {
                    0 => {
                        let p = pieces[rng.below(pieces.len())];
                        m.splice(pos..pos, p.iter().copied());
                    }
                    1 if !m.is_empty() => {
                        let end = (pos + 1 + rng.below(8)).min(m.len());
                        m.drain(pos..end);
                    }
                    _ => m.truncate(pos),
                }
            }
            run_all_stages(&m);
        }
    }
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(42);
    for _ in 0..2000 {
        let len = rng.below(200);
        let bytes: Vec<u8> = (0..len).map(|_| (rng.next() & 0xff) as u8).collect();
        run_all_stages(&bytes);
    }
}

#[test]
fn formatting_is_idempotent_on_all_parseable_fixtures() {
    let mut formatted = 0;
    for f in corpus() {
        let text = std::fs::read_to_string(&f).unwrap();
        let mut sm = SourceMap::new();
        let id = sm.add("f.bv", PathBuf::new(), &text);
        let Ok(once) = beaver_syntax::fmt::format_source(id, sm.file(id)) else {
            continue;
        };
        let id2 = sm.add("f2.bv", PathBuf::new(), &once);
        let twice = beaver_syntax::fmt::format_source(id2, sm.file(id2)).unwrap();
        assert_eq!(once, twice, "not idempotent: {}", f.display());
        formatted += 1;
    }
    assert!(formatted > 90, "only {formatted} fixtures formatted");
}
