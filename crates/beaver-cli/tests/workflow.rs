//! End-to-end CLI workflows: project creation, building and running native
//! executables, tests, formatting, modules, local libraries and packaging.

mod common;
use common::*;
use std::path::Path;

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

#[test]
fn new_binary_builds_runs_tests_and_releases() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let out = beaver(tmp.path(), &cache, &["new", "demo"]);
    assert!(out.status.success(), "{}", describe(&out));
    let demo = tmp.path().join("demo");
    for f in ["beaver.toml", "src/main.bv", "tests/smoke.bv", ".gitignore"] {
        assert!(demo.join(f).is_file(), "missing {f}");
    }
    assert!(std::fs::read_to_string(demo.join(".gitignore"))
        .unwrap()
        .contains("target/"));
    let run = beaver(&demo, &cache, &["run"]);
    assert!(run.status.success(), "{}", describe(&run));
    assert_eq!(text(&run.stdout), "Hello, Beaver!\n");
    let test = beaver(&demo, &cache, &["test"]);
    assert!(test.status.success(), "{}", describe(&test));
    assert!(text(&test.stderr).contains("1 passed; 0 failed"));
    let rel = beaver(&demo, &cache, &["build", "--release"]);
    assert!(rel.status.success(), "{}", describe(&rel));
    let exe = demo.join("target/release/demo");
    let direct = std::process::Command::new(&exe).output().unwrap();
    assert_eq!(text(&direct.stdout), "Hello, Beaver!\n");
    // Generated C is inspectable and the binary is native machine code.
    assert!(demo.join("target/release/generated/demo.c").is_file());
    let bytes = std::fs::read(&exe).unwrap();
    let native = bytes.starts_with(b"\x7fELF")
        || bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
        || bytes.starts_with(&[0xca, 0xfe, 0xba, 0xbe]);
    assert!(native, "not a native executable");
    // Deterministic C output for identical input.
    let c1 = std::fs::read(demo.join("target/release/generated/demo.c")).unwrap();
    assert!(beaver(&demo, &cache, &["build", "--release"])
        .status
        .success());
    assert_eq!(
        c1,
        std::fs::read(demo.join("target/release/generated/demo.c")).unwrap()
    );
}

#[test]
fn new_and_init_refuse_to_overwrite_and_validate_names() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    assert!(beaver(tmp.path(), &cache, &["new", "demo"])
        .status
        .success());
    let again = beaver(tmp.path(), &cache, &["new", "demo"]);
    assert!(!again.status.success() && text(&again.stderr).contains("already exists"));
    let bad = beaver(tmp.path(), &cache, &["new", "Bad-Name"]);
    assert!(!bad.status.success() && text(&bad.stderr).contains("invalid package name"));
    let empty = tmp.path().join("fresh_lib");
    std::fs::create_dir(&empty).unwrap();
    let init = beaver(&empty, &cache, &["init", "--lib"]);
    assert!(init.status.success(), "{}", describe(&init));
    assert!(empty.join("src/lib.bv").is_file());
    let init2 = beaver(&empty, &cache, &["init"]);
    assert!(!init2.status.success() && text(&init2.stderr).contains("refusing to overwrite"));
    let t = beaver(&empty, &cache, &["test"]);
    assert!(t.status.success(), "{}", describe(&t));
}

#[test]
fn library_packages_build_but_do_not_run() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let lib = tmp.path().join("mathkit");
    copy_dir(&spec_dir().join("examples/mathkit"), &lib);
    let b = beaver(&lib, &cache, &["build"]);
    assert!(b.status.success(), "{}", describe(&b));
    assert!(lib.join("target/debug/libmathkit.o").is_file());
    assert!(
        !lib.join("target/debug/mathkit").exists(),
        "library produced an executable"
    );
    let r = beaver(&lib, &cache, &["run"]);
    assert!(!r.status.success() && text(&r.stderr).contains("error[B6004]"));
    let t = beaver(&lib, &cache, &["test"]);
    assert!(
        t.status.success() && text(&t.stderr).contains("square ... ok"),
        "{}",
        describe(&t)
    );
}

/// app (bin) -> local modules `util`, `geo.shapes` + path library `mathkit`.
fn multi_file_app(root: &Path) -> std::path::PathBuf {
    copy_dir(&spec_dir().join("examples/mathkit"), &root.join("mathkit"));
    let app = root.join("app");
    write(
        &app.join("beaver.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2026\"\nkind = \"bin\"\n\n[dependencies]\n",
    );
    write(
        &app.join("src/main.bv"),
        "use util\nuse geo.shapes\nuse mathkit\n\nfn main():\n    print(util.double(21))\n    print(shapes.area(shapes.Square(side=3)))\n    var p = mathkit.Point(x=2, y=7)\n    mathkit.translate_x(edit p, 5)\n    print(mathkit.square(p.x))\n",
    );
    write(
        &app.join("src/util.bv"),
        "pub fn double(n: Int) -> Int:\n    return helper(n) * 2\n\nfn helper(n: Int) -> Int:\n    return n\n",
    );
    write(
        &app.join("src/geo/shapes.bv"),
        "use util\n\npub type Square:\n    side: Int\n\npub fn area(s: Square) -> Int:\n    return util.double(s.side * s.side) / 2\n",
    );
    write(
        &app.join("tests/app_test.bv"),
        "use util\nuse mathkit\n\ntest \"double\":\n    assert_eq(util.double(4), 8)\n\ntest \"library\":\n    assert_eq(mathkit.square(3), 9)\n",
    );
    app
}

#[test]
fn multi_file_application_with_local_module_and_library_package() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = multi_file_app(tmp.path());
    let add = beaver(&app, &cache, &["add", "--path", "../mathkit"]);
    assert!(add.status.success(), "{}", describe(&add));
    let lock = std::fs::read_to_string(app.join("beaver.lock")).unwrap();
    assert!(
        lock.contains("source = \"path+../mathkit\"") && !lock.contains("checksum"),
        "{lock}"
    );
    let run = beaver(&app, &cache, &["run"]);
    assert!(run.status.success(), "{}", describe(&run));
    assert_eq!(text(&run.stdout), "42\n9\n49\n");
    let test = beaver(&app, &cache, &["test"]);
    assert!(
        test.status.success() && text(&test.stderr).contains("2 passed; 0 failed"),
        "{}",
        describe(&test)
    );
    // Symbols from different modules/packages get distinct C names.
    let c = std::fs::read_to_string(app.join("target/debug/generated/app.c")).unwrap();
    assert!(
        c.contains("bvr_3app4util_6double") && c.contains("bvr_7mathkit3lib_6square"),
        "mangling"
    );
}

#[test]
fn visibility_imports_and_cycles_are_enforced() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = multi_file_app(tmp.path());
    assert!(beaver(&app, &cache, &["add", "--path", "../mathkit"])
        .status
        .success());
    let main = std::fs::read_to_string(app.join("src/main.bv")).unwrap();
    let cases = [
        (
            "    print(mathkit.hidden_double(1))\n",
            "error[B2013]: `hidden_double` is private to module `mathkit`",
        ),
        (
            "    print(util.helper(1))\n",
            "error[B2013]: `helper` is private to module `util`",
        ),
        ("    print(mathkit.missing(1))\n", "error[B2001]"),
    ];
    for (line, needle) in cases {
        write(&app.join("src/main.bv"), &format!("{main}{line}"));
        let out = beaver(&app, &cache, &["check"]);
        assert!(
            !out.status.success() && text(&out.stderr).contains(needle),
            "{needle}\n{}",
            describe(&out)
        );
    }
    write(&app.join("src/main.bv"), &format!("use nowhere\n{main}"));
    let out = beaver(&app, &cache, &["check"]);
    assert!(
        text(&out.stderr).contains("error[B2014]: module `nowhere` not found"),
        "{}",
        describe(&out)
    );
    write(&app.join("src/main.bv"), &main);
    let util = std::fs::read_to_string(app.join("src/util.bv")).unwrap();
    write(&app.join("src/util.bv"), &format!("use geo.shapes\n{util}"));
    let out = beaver(&app, &cache, &["check"]);
    assert!(
        text(&out.stderr).contains("error[B2015]: import cycle detected"),
        "{}",
        describe(&out)
    );
    write(&app.join("src/util.bv"), &util);
    // An unlisted transitive package cannot be imported.
    write(&app.join("src/main.bv"), &format!("use requires_a\n{main}"));
    let out = beaver(&app, &cache, &["check"]);
    assert!(
        text(&out.stderr).contains("error[B2014]"),
        "{}",
        describe(&out)
    );
}

#[test]
fn package_archives_are_deterministic_and_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let lib = tmp.path().join("mathkit");
    copy_dir(&spec_dir().join("examples/mathkit"), &lib);
    write(&lib.join("target/junk.bv"), "garbage");
    write(&lib.join("notes.txt"), "not packaged");
    let a = beaver(&lib, &cache, &["package"]);
    assert!(a.status.success(), "{}", describe(&a));
    let path = lib.join("target/package/mathkit-1.0.0.tar.gz");
    let first = std::fs::read(&path).unwrap();
    // Touch the sources (mtime changes) and repackage elsewhere.
    let src = std::fs::read(lib.join("src/lib.bv")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(lib.join("src/lib.bv"), &src).unwrap();
    let b = beaver(
        &lib,
        &cache,
        &[
            "package",
            "--output",
            tmp.path().join("out").to_str().unwrap(),
        ],
    );
    assert!(b.status.success(), "{}", describe(&b));
    let second = std::fs::read(tmp.path().join("out/mathkit-1.0.0.tar.gz")).unwrap();
    assert_eq!(first, second, "archive bytes differ between runs");
    assert!(text(&b.stdout).contains("sha256 "));
    // Inspect members.
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(first.as_slice()));
    let names: Vec<String> = ar
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().display().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "mathkit-1.0.0/beaver.toml",
            "mathkit-1.0.0/src/lib.bv",
            "mathkit-1.0.0/tests/smoke.bv"
        ]
    );

    let bin = bin_package(tmp.path(), "binpkg", "fn main():\n    pass\n");
    let out = beaver(&bin, &cache, &["package"]);
    assert!(!out.status.success() && text(&out.stderr).contains("error[B6060]"));
    let m = std::fs::read_to_string(lib.join("beaver.toml")).unwrap();
    write(
        &lib.join("beaver.toml"),
        &format!("{m}other = {{ path = \"../other\" }}\n"),
    );
    let out = beaver(&lib, &cache, &["package"]);
    assert!(
        !out.status.success() && text(&out.stderr).contains("error[B6061]"),
        "{}",
        describe(&out)
    );
    write(&lib.join("beaver.toml"), &m);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hosts", lib.join("src/evil.bv")).unwrap();
        let out = beaver(&lib, &cache, &["package"]);
        assert!(
            !out.status.success() && text(&out.stderr).contains("symlink"),
            "{}",
            describe(&out)
        );
    }
}

#[test]
fn fmt_is_idempotent_and_check_reports_unformatted_files() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(
        tmp.path(),
        "fmtapp",
        "# top comment\nfn  main( ):\n        let s = \"keep   this  # not a comment\"\n        if 1<2 :\n                print( s )   # trailing\n                # inside\n        print(add(1,-2))\n\n\n\n\nfn add(a:Int,b :Int)->Int:\n    return a+b\n",
    );
    let check = beaver(&app, &cache, &["fmt", "--check"]);
    assert!(!check.status.success() && text(&check.stderr).contains("Not formatted: src/main.bv"));
    let before_run = beaver(&app, &cache, &["run"]);
    assert!(before_run.status.success(), "{}", describe(&before_run));
    assert!(beaver(&app, &cache, &["fmt"]).status.success());
    let once = std::fs::read_to_string(app.join("src/main.bv")).unwrap();
    assert_eq!(
        once,
        "# top comment\nfn main():\n    let s = \"keep   this  # not a comment\"\n    if 1 < 2:\n        print(s)  # trailing\n        # inside\n    print(add(1, -2))\n\n\nfn add(a: Int, b: Int) -> Int:\n    return a + b\n"
    );
    assert!(beaver(&app, &cache, &["fmt"]).status.success());
    assert_eq!(
        std::fs::read_to_string(app.join("src/main.bv")).unwrap(),
        once,
        "fmt is not idempotent"
    );
    assert!(beaver(&app, &cache, &["fmt", "--check"]).status.success());
    let after_run = beaver(&app, &cache, &["run"]);
    assert_eq!(
        text(&after_run.stdout),
        text(&before_run.stdout),
        "formatting changed behaviour"
    );
    // Unparseable files are reported and never rewritten.
    write(&app.join("src/broken.bv"), "fn f(:\n");
    let out = beaver(&app, &cache, &["fmt"]);
    assert!(!out.status.success() && text(&out.stderr).contains("error[B1012]"));
    assert_eq!(
        std::fs::read_to_string(app.join("src/broken.bv")).unwrap(),
        "fn f(:\n"
    );
}

#[test]
fn test_runner_isolates_failures_and_reports_names() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(tmp.path(), "tested", "fn main():\n    pass\n");
    write(
        &app.join("tests/a_math.bv"),
        "test \"passes\":\n    assert_eq(1 + 1, 2)\n\ntest \"traps\":\n    let xs = [1]\n    print(xs[3])\n\ntest \"string mismatch\":\n    assert_eq(\"a\", \"b\")\n",
    );
    write(
        &app.join("tests/b_more.bv"),
        "test \"after the trap\":\n    assert(true)\n",
    );
    let out = beaver(&app, &cache, &["test"]);
    assert!(!out.status.success(), "{}", describe(&out));
    let e = text(&out.stderr);
    assert!(e.contains("running 4 tests"), "{e}");
    assert!(e.contains("test tests/a_math.bv :: passes ... ok"), "{e}");
    assert!(
        e.contains("test tests/a_math.bv :: traps ... FAILED"),
        "{e}"
    );
    assert!(
        e.contains("test tests/b_more.bv :: after the trap ... ok"),
        "{e}"
    );
    assert!(
        e.contains("B5003") && e.contains("left = \"a\", right = \"b\""),
        "{e}"
    );
    assert!(e.contains("test result: FAILED. 2 passed; 2 failed"), "{e}");
    // `test` blocks are only allowed under tests/.
    write(
        &app.join("src/main.bv"),
        "test \"x\":\n    pass\n\nfn main():\n    pass\n",
    );
    let out = beaver(&app, &cache, &["test"]);
    assert!(!out.status.success() && text(&out.stderr).contains("error[B2024]"));
    // No test files: success with 0 tests.
    let empty = bin_package(tmp.path(), "notests", "fn main():\n    pass\n");
    let out = beaver(&empty, &cache, &["test"]);
    assert!(
        out.status.success() && text(&out.stderr).contains("running 0 tests"),
        "{}",
        describe(&out)
    );
}

#[test]
fn run_propagates_exit_status() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(
        tmp.path(),
        "trapper",
        "fn main():\n    print(\"start\")\n    let z = 0\n    print(1 / z)\n",
    );
    let out = beaver(&app, &cache, &["run"]);
    assert_eq!(out.status.code(), Some(101), "{}", describe(&out));
    assert_eq!(text(&out.stdout), "start\n");
    assert!(text(&out.stderr).contains("runtime error[B5002]: division by zero"));
    assert!(
        text(&out.stderr).contains("--> src/main.bv:4:11"),
        "{}",
        describe(&out)
    );
}

#[test]
fn check_does_not_need_a_c_compiler() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(tmp.path(), "nocc", "fn main():\n    print(1)\n");
    let check = beaver_cmd(&app, &cache, &["check"])
        .env("BEAVER_CC", "/nonexistent/cc")
        .env("PATH", "/nonexistent")
        .output()
        .unwrap();
    assert!(check.status.success(), "{}", describe(&check));
    let build = beaver_cmd(&app, &cache, &["build"])
        .env("BEAVER_CC", "/nonexistent/cc")
        .output()
        .unwrap();
    assert!(!build.status.success());
    assert!(
        text(&build.stderr).contains("error[B4001]"),
        "{}",
        describe(&build)
    );
}

#[test]
fn beaver_cc_override_is_honored() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(tmp.path(), "withcc", "fn main():\n    print(7)\n");
    let cc = ["clang", "cc", "gcc"]
        .iter()
        .find(|c| {
            std::process::Command::new(c)
                .arg("--version")
                .output()
                .is_ok()
        })
        .unwrap()
        .to_string();
    let out = beaver_cmd(&app, &cache, &["run"])
        .env("BEAVER_CC", &cc)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", describe(&out));
    assert_eq!(text(&out.stdout), "7\n");
}

#[test]
fn help_version_and_bad_arguments_never_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let help = beaver(tmp.path(), &cache, &["--help"]);
    assert!(help.status.success());
    let h = text(&help.stdout);
    for cmd in [
        "new", "init", "check", "build", "run", "test", "fmt", "add", "remove", "update", "fetch",
        "package",
    ] {
        assert!(h.contains(cmd), "help lacks {cmd}");
    }
    let v = beaver(tmp.path(), &cache, &["--version"]);
    assert!(v.status.success() && text(&v.stdout).contains("beaver 0.1.0"));
    for args in [
        vec!["frobnicate"],
        vec!["build", "--nope"],
        vec!["add"],
        vec!["add", "nover"],
        vec!["check"],
        vec!["update", "x@1"],
    ] {
        let o = beaver(tmp.path(), &cache, &args);
        assert!(!o.status.success(), "{args:?}");
        assert!(
            !text(&o.stderr).contains("panicked"),
            "{args:?}: {}",
            describe(&o)
        );
    }
}

#[test]
fn invalid_manifests_and_entries_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(tmp.path(), "badman", "fn main():\n    pass\n");
    let good = std::fs::read_to_string(app.join("beaver.toml")).unwrap();
    for (bad, needle) in [
        (
            good.replace("edition = \"2026\"", "edition = \"2025\""),
            "unsupported edition",
        ),
        (
            good.replace("src/main.bv", "../escape.bv"),
            "inside the package",
        ),
        (format!("{good}\n[workspace]\n"), "unknown field"),
        (format!("{good}x = \"1.0.0\"\n"), "must be a table"),
        (
            format!("{good}x = {{ version = \"^1.0\" }}\n"),
            "exact MAJOR.MINOR.PATCH",
        ),
    ] {
        write(&app.join("beaver.toml"), &bad);
        let out = beaver(&app, &cache, &["check"]);
        assert!(
            !out.status.success()
                && text(&out.stderr).contains("error[B6001]")
                && text(&out.stderr).contains(needle),
            "{needle}: {}",
            describe(&out)
        );
    }
    write(&app.join("beaver.toml"), &good);
    #[cfg(unix)]
    {
        let outside = tmp.path().join("outside.bv");
        write(&outside, "fn main():\n    pass\n");
        std::fs::remove_file(app.join("src/main.bv")).unwrap();
        std::os::unix::fs::symlink(&outside, app.join("src/main.bv")).unwrap();
        let out = beaver(&app, &cache, &["check"]);
        assert!(
            !out.status.success() && text(&out.stderr).contains("symlink"),
            "{}",
            describe(&out)
        );
    }
}

#[test]
fn diagnostics_have_codes_locations_and_help() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let app = bin_package(
        tmp.path(),
        "diag",
        "fn main():\n    var value = 1\n    let reader = view value\n    let writer = edit value\n    print(reader)\n",
    );
    let out = beaver(&app, &cache, &["check"]);
    let e = text(&out.stderr);
    assert!(
        e.contains("error[B3002]: cannot edit `value` while it is borrowed"),
        "{e}"
    );
    assert!(e.contains("--> src/main.bv:4:18"), "{e}");
    assert!(e.contains("4 |     let writer = edit value"), "{e}");
    assert!(e.contains("help: wrap `reader` in `scope:`"), "{e}");
}
