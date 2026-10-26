//! Compiles the bundled C runtime with and without compiler overflow
//! builtins and checks every checked Int operation against Rust's
//! `checked_*` reference results on boundary values.

use std::process::Command;

const VALUES: &[i64] = &[
    i64::MIN,
    i64::MIN + 1,
    -4611686018427387904,
    -3037000500,
    -65536,
    -2,
    -1,
    0,
    1,
    2,
    3,
    65536,
    3037000499,
    3037000500,
    4611686018427387904,
    i64::MAX - 1,
    i64::MAX,
];

const HARNESS: &str = r#"
#include "beaver_rt.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    long long a = strtoll(argv[2], NULL, 10);
    long long b = strtoll(argv[3], NULL, 10);
    int64_t r = 0;
    const char *op = argv[1];
    if (!strcmp(op, "add")) r = bvr_add_i64(a, b, NULL);
    else if (!strcmp(op, "sub")) r = bvr_sub_i64(a, b, NULL);
    else if (!strcmp(op, "mul")) r = bvr_mul_i64(a, b, NULL);
    else if (!strcmp(op, "div")) r = bvr_div_i64(a, b, NULL);
    else if (!strcmp(op, "rem")) r = bvr_rem_i64(a, b, NULL);
    else if (!strcmp(op, "neg")) r = bvr_neg_i64(a, NULL);
    else return 2;
    printf("%lld\n", (long long)r);
    return 0;
}
"#;

fn cc() -> String {
    std::env::var("BEAVER_CC").unwrap_or_else(|_| "cc".into())
}

fn build(dir: &std::path::Path, fallback: bool) -> std::path::PathBuf {
    std::fs::write(dir.join("beaver_rt.h"), beaver_codegen_c::RUNTIME_HEADER).unwrap();
    std::fs::write(dir.join("beaver_rt.c"), beaver_codegen_c::RUNTIME_SOURCE).unwrap();
    std::fs::write(dir.join("harness.c"), HARNESS).unwrap();
    let exe = dir.join(if fallback { "h_fallback" } else { "h_builtin" });
    let mut c = Command::new(cc());
    c.args(["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-I"])
        .arg(dir);
    if fallback {
        c.arg("-DBVR_NO_BUILTINS");
    }
    c.arg(dir.join("harness.c"))
        .arg(dir.join("beaver_rt.c"))
        .arg("-o")
        .arg(&exe);
    let out = c.output().expect("C compiler available");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    exe
}

fn reference(op: &str, a: i64, b: i64) -> Result<i64, &'static str> {
    match op {
        "add" => a.checked_add(b).ok_or("B5001"),
        "sub" => a.checked_sub(b).ok_or("B5001"),
        "mul" => a.checked_mul(b).ok_or("B5001"),
        "div" if b == 0 => Err("B5002"),
        "div" => a.checked_div(b).ok_or("B5001"),
        "rem" if b == 0 => Err("B5002"),
        // Mathematically 0; must not invoke C's undefined INT64_MIN % -1.
        "rem" => Ok(a.checked_rem(b).unwrap_or(0)),
        "neg" => a.checked_neg().ok_or("B5001"),
        _ => unreachable!(),
    }
}

fn check_all(fallback: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let exe = build(tmp.path(), fallback);
    let mut cases = 0;
    for op in ["add", "sub", "mul", "div", "rem", "neg"] {
        for &a in VALUES {
            for &b in VALUES {
                if op == "neg" && b != 0 {
                    continue;
                }
                cases += 1;
                let out = Command::new(&exe)
                    .args([op, &a.to_string(), &b.to_string()])
                    .output()
                    .unwrap();
                let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                match reference(op, a, b) {
                    Ok(v) => assert!(
                        out.status.success() && stdout == v.to_string(),
                        "{op}({a}, {b}) fallback={fallback}: expected {v}, got {stdout} / {stderr}"
                    ),
                    Err(code) => {
                        assert_eq!(
                            out.status.code(),
                            Some(101),
                            "{op}({a}, {b}) fallback={fallback} should trap"
                        );
                        assert!(
                            stderr.contains(code),
                            "{op}({a}, {b}): expected {code}, got {stderr}"
                        );
                    }
                }
            }
        }
    }
    assert!(cases > 1000);
}

#[test]
fn checked_arithmetic_with_compiler_builtins() {
    check_all(false);
}

#[test]
fn checked_arithmetic_with_portable_c11_fallback() {
    check_all(true);
}
