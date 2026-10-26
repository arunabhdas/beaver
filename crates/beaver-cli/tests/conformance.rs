//! Language conformance: the handoff fixtures (`Beaver-0.1-spec/fixtures`)
//! plus this repository's own (`tests/conformance`).
//!
//! * compile_pass: `beaver run` succeeds, all owned allocations are released,
//!   and stdout matches `<name>.stdout` when present.
//! * compile_fail: `beaver check` fails with the diagnostic code prefix in
//!   `<name>.expect`, and no executable is produced.
//! * runtime: exit status / stdout / stderr match `<name>.expect`.

mod common;
use common::*;
use std::path::{Path, PathBuf};

fn dirs(kind: &str) -> Vec<PathBuf> {
    vec![
        spec_dir().join("fixtures").join(kind),
        repo_root().join("tests/conformance").join(kind),
    ]
}

fn setup(fixture: &Path) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = std::fs::read_to_string(fixture).unwrap();
    let dir = bin_package(tmp.path(), "fixture", &src);
    (tmp, dir)
}

fn run_pass(fixture: &Path, extra_env: &[(&str, &str)]) -> Result<(), String> {
    let (tmp, dir) = setup(fixture);
    let check = beaver(&dir, &tmp.path().join("cache"), &["check"]);
    if !check.status.success() {
        return Err(format!(
            "{}: beaver check failed\n{}",
            fixture.display(),
            describe(&check)
        ));
    }
    let mut cmd = beaver_cmd(&dir, &tmp.path().join("cache"), &["run"]);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    if !out.status.success() {
        return Err(format!(
            "{}: beaver run failed\n{}",
            fixture.display(),
            describe(&out)
        ));
    }
    let stderr = text(&out.stderr);
    if !stderr.contains("all owned allocations released") {
        return Err(format!(
            "{}: leak check did not pass\n{}",
            fixture.display(),
            describe(&out)
        ));
    }
    let expected = fixture.with_extension("stdout");
    if expected.exists() {
        let want = std::fs::read_to_string(&expected).unwrap();
        if text(&out.stdout) != want {
            return Err(format!(
                "{}: stdout mismatch\n--- expected ---\n{want}\n{}",
                fixture.display(),
                describe(&out)
            ));
        }
    }
    Ok(())
}

#[test]
fn compile_pass_fixtures_run_and_release_everything() {
    let fixtures: Vec<PathBuf> = dirs("compile_pass")
        .iter()
        .flat_map(|d| fixtures(d))
        .collect();
    assert!(
        fixtures.len() >= 28,
        "expected at least 28 compile-pass programs"
    );
    let failures = parallel(&fixtures, |f: &PathBuf| run_pass(f, &[]));
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn compile_fail_fixtures_are_rejected_with_the_right_code() {
    let fixtures: Vec<PathBuf> = dirs("compile_fail")
        .iter()
        .flat_map(|d| fixtures(d))
        .collect();
    assert!(
        fixtures.len() >= 60,
        "expected at least 60 compile-fail programs"
    );
    let failures = parallel(&fixtures, |f| {
        let (tmp, dir) = setup(f);
        let want = std::fs::read_to_string(f.with_extension("expect"))
            .unwrap()
            .trim()
            .to_string();
        let out = beaver(&dir, &tmp.path().join("cache"), &["check"]);
        let stderr = text(&out.stderr);
        if out.status.success() {
            return Err(format!(
                "{}: expected rejection with {want}, but check passed",
                f.display()
            ));
        }
        if !stderr.contains(&format!("error[{want}")) {
            return Err(format!(
                "{}: expected error[{want}...]\n{}",
                f.display(),
                describe(&out)
            ));
        }
        if !stderr.contains("src/main.bv:") && !want.starts_with("B2020") {
            return Err(format!(
                "{}: diagnostic lacks a source location\n{}",
                f.display(),
                describe(&out)
            ));
        }
        if stderr.contains("panicked") {
            return Err(format!(
                "{}: compiler panicked\n{}",
                f.display(),
                describe(&out)
            ));
        }
        // A failing check must never produce native code.
        let build = beaver(&dir, &tmp.path().join("cache"), &["build"]);
        if build.status.success() || dir.join("target/debug/fixture").exists() {
            return Err(format!("{}: build succeeded despite errors", f.display()));
        }
        Ok(())
    });
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn run_runtime(f: &Path, extra_env: &[(&str, &str)]) -> Result<(), String> {
    let (tmp, dir) = setup(f);
    let expect = std::fs::read_to_string(f.with_extension("expect")).unwrap();
    let mut cmd = beaver_cmd(&dir, &tmp.path().join("cache"), &["run"]);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let stdout = text(&out.stdout);
    let stderr = text(&out.stderr);
    for line in expect.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("exit=") {
            let ok = match v {
                "0" => out.status.code() == Some(0),
                "nonzero" => out.status.code().is_some_and(|c| c != 0),
                _ => false,
            };
            if !ok {
                return Err(format!(
                    "{}: expected exit={v}\n{}",
                    f.display(),
                    describe(&out)
                ));
            }
            if v == "nonzero" && out.status.code() != Some(101) {
                return Err(format!(
                    "{}: runtime traps must exit with status 101\n{}",
                    f.display(),
                    describe(&out)
                ));
            }
        } else if let Some(v) = line.strip_prefix("stdout=") {
            if stdout.trim_end_matches('\n') != v {
                return Err(format!(
                    "{}: expected stdout `{v}`\n{}",
                    f.display(),
                    describe(&out)
                ));
            }
        } else if let Some(v) = line.strip_prefix("stderr=") {
            if !stderr.contains(v) {
                return Err(format!(
                    "{}: expected stderr to contain `{v}`\n{}",
                    f.display(),
                    describe(&out)
                ));
            }
        }
    }
    // Sanitizer *errors* (not allocation-failure warnings) and UBSan reports.
    if stderr.contains("ERROR: AddressSanitizer") || stderr.contains("runtime error: ") {
        return Err(format!(
            "{}: sanitizer report\n{}",
            f.display(),
            describe(&out)
        ));
    }
    Ok(())
}

#[test]
fn runtime_fixtures_trap_safely_and_deterministically() {
    let fixtures: Vec<PathBuf> = dirs("runtime").iter().flat_map(|d| fixtures(d)).collect();
    assert!(
        fixtures.len() >= 25,
        "expected at least 25 runtime programs"
    );
    let failures = parallel(&fixtures, |f| run_runtime(f, &[]));
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Does the host C compiler support ASan + UBSan?
fn sanitizers_available() -> bool {
    let tmp = tempfile::tempdir().unwrap();
    let c = tmp.path().join("probe.c");
    std::fs::write(&c, "int main(void) { return 0; }\n").unwrap();
    let cc = std::env::var("BEAVER_CC").unwrap_or_else(|_| "cc".into());
    std::process::Command::new(cc)
        .args(["-fsanitize=address,undefined", "-o"])
        .arg(tmp.path().join("probe"))
        .arg(&c)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn fixtures_are_clean_under_address_and_undefined_sanitizers() {
    if !sanitizers_available() {
        eprintln!("SKIPPED: host C compiler does not support -fsanitize=address,undefined");
        return;
    }
    let env = [
        ("BEAVER_SANITIZE", "1"),
        (
            "ASAN_OPTIONS",
            "detect_leaks=0:abort_on_error=0:allocator_may_return_null=1",
        ),
    ];
    let pass: Vec<PathBuf> = dirs("compile_pass")
        .iter()
        .flat_map(|d| fixtures(d))
        .collect();
    let mut failures = parallel(&pass, |f| run_pass(f, &env));
    let rt: Vec<PathBuf> = dirs("runtime").iter().flat_map(|d| fixtures(d)).collect();
    failures.extend(parallel(&rt, |f| run_runtime(f, &env)));
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
