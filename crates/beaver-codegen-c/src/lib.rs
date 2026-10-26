//! C11 backend: emits one translation unit from MIR, writes it together with
//! the bundled runtime, and invokes the host C compiler.

pub mod emit;

use beaver_mir::MirProgram;
use beaver_source::Diagnostic;
pub use emit::Output;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const RUNTIME_HEADER: &str = include_str!("../../../runtime/include/beaver_rt.h");
pub const RUNTIME_SOURCE: &str = include_str!("../../../runtime/src/beaver_rt.c");

pub struct BuildRequest<'a> {
    pub mir: &'a MirProgram,
    pub output: Output,
    /// Directory for generated C (e.g. `target/debug/generated`).
    pub gen_dir: &'a Path,
    /// Base name for the generated `.c` file.
    pub unit_name: &'a str,
    /// Final executable or object path.
    pub out_path: &'a Path,
    pub release: bool,
}

pub struct BuildReport {
    /// Warnings printed by the C compiler (normally empty).
    pub warnings: String,
    pub compiler: PathBuf,
}

/// Locate the host C compiler: `BEAVER_CC`, then `CC`, then `clang`, then `cc`.
pub fn find_c_compiler() -> Result<PathBuf, Diagnostic> {
    for var in ["BEAVER_CC", "CC"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.trim().to_string();
            if v.is_empty() {
                continue;
            }
            return resolve_program(&v).ok_or_else(|| {
                Diagnostic::global(
                    "B4001",
                    format!("C compiler `{v}` from ${var} was not found or is not an executable file"),
                )
                .with_help(format!("set {var} to the path or name of a C11 compiler such as clang or gcc (no arguments)"))
            });
        }
    }
    for name in ["clang", "cc", "gcc"] {
        if let Some(p) = resolve_program(name) {
            return Ok(p);
        }
    }
    Err(Diagnostic::global("B4001", "no C compiler found (tried $BEAVER_CC, $CC, clang, cc, gcc)")
        .with_note("Beaver compiles to C11 and needs a host C compiler to produce native executables")
        .with_help("install clang or gcc, or set BEAVER_CC to a C11 compiler; `beaver check` works without one"))
}

fn resolve_program(name: &str) -> Option<PathBuf> {
    let is_exec = |p: &Path| -> bool {
        match std::fs::metadata(p) {
            Ok(m) if m.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
            _ => false,
        }
    };
    if name.contains('/') || name.contains(std::path::MAIN_SEPARATOR) {
        let p = PathBuf::from(name);
        return is_exec(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| is_exec(p))
}

fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Ok(old) = std::fs::read_to_string(path) {
        if old == contents {
            return Ok(());
        }
    }
    std::fs::write(path, contents)
}

fn io_err(what: &str, path: &Path, e: std::io::Error) -> Diagnostic {
    Diagnostic::global("B4003", format!("cannot {what} `{}`: {e}", path.display()))
}

/// Generate C for `req.mir` and compile it with the host C compiler.
pub fn build(req: &BuildRequest) -> Result<BuildReport, Diagnostic> {
    let is_lib = matches!(req.output, Output::Library);
    let c_text = emit::emit(req.mir, req.output);
    std::fs::create_dir_all(req.gen_dir).map_err(|e| io_err("create directory", req.gen_dir, e))?;
    let unit = req.gen_dir.join(format!("{}.c", req.unit_name));
    let hdr = req.gen_dir.join("beaver_rt.h");
    let rt = req.gen_dir.join("beaver_rt.c");
    write_if_changed(&unit, &c_text).map_err(|e| io_err("write", &unit, e))?;
    write_if_changed(&hdr, RUNTIME_HEADER).map_err(|e| io_err("write", &hdr, e))?;
    write_if_changed(&rt, RUNTIME_SOURCE).map_err(|e| io_err("write", &rt, e))?;
    if let Some(parent) = req.out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err("create directory", parent, e))?;
    }

    let cc = find_c_compiler()?;
    let mut cmd = Command::new(&cc);
    cmd.arg("-std=c11");
    if req.release {
        cmd.arg("-O2");
    } else {
        cmd.args(["-O0", "-g"]);
    }
    cmd.args(["-Wall", "-Wextra"]);
    if std::env::var("BEAVER_WERROR").as_deref() == Ok("1") {
        cmd.arg("-Werror");
    }
    if std::env::var("BEAVER_SANITIZE").as_deref() == Ok("1") {
        cmd.args([
            "-fsanitize=address,undefined",
            "-fno-sanitize-recover=undefined",
            "-fno-omit-frame-pointer",
        ]);
    }
    cmd.arg("-I").arg(req.gen_dir);
    if is_lib {
        cmd.arg("-c").arg(&unit);
    } else {
        cmd.arg(&unit).arg(&rt);
    }
    cmd.arg("-o").arg(req.out_path);
    let out = cmd.output().map_err(|e| {
        Diagnostic::global(
            "B4001",
            format!("failed to run C compiler `{}`: {e}", cc.display()),
        )
    })?;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        return Err(Diagnostic::global(
            "B4002",
            format!("the C compiler `{}` failed on generated code", cc.display()),
        )
        .with_note(stderr.trim_end().to_string())
        .with_note(format!("generated source: {}", unit.display()))
        .with_help("this is a Beaver compiler bug; please report it with the generated source"));
    }
    Ok(BuildReport {
        warnings: stderr,
        compiler: cc,
    })
}
