//! `beaver.toml` parsing, validation and editing.

use beaver_source::Diagnostic;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

pub const MANIFEST: &str = "beaver.toml";
pub const LOCKFILE: &str = "beaver.lock";
pub const EDITION: &str = "2026";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Exact `MAJOR.MINOR.PATCH` with no prerelease/build suffix.
    pub fn parse(s: &str) -> Result<Version, String> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("`{s}` is not an exact MAJOR.MINOR.PATCH version"));
        }
        let mut nums = [0u64; 3];
        for (i, p) in parts.iter().enumerate() {
            let valid = !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'));
            if !valid {
                return Err(format!(
                    "`{s}` is not an exact MAJOR.MINOR.PATCH version (ranges, prereleases and leading zeros are not supported)"
                ));
            }
            nums[i] = p
                .parse()
                .map_err(|_| format!("version component `{p}` is too large"))?;
        }
        Ok(Version {
            major: nums[0],
            minor: nums[1],
            patch: nums[2],
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bin,
    Lib,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Bin => "bin",
            Kind::Lib => "lib",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSpec {
    Registry(Version),
    /// Path exactly as written in the manifest (relative to the package).
    Path(String),
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub version: Version,
    pub kind: Kind,
    /// Relative to the package root, always under `src/`.
    pub entry: PathBuf,
    pub deps: BTreeMap<String, DepSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    package: RawPackage,
    #[serde(default)]
    dependencies: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPackage {
    name: String,
    version: String,
    edition: String,
    kind: String,
    entry: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDep {
    version: Option<String>,
    path: Option<String>,
}

const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9", "beaver",
    "std", "src", "tests", "target",
];

/// Validate a package name: `[a-z][a-z0-9_]{0,63}`, not reserved, not a keyword.
pub fn validate_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let first_ok = matches!(chars.next(), Some('a'..='z'));
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !first_ok || !rest_ok || name.len() > 64 {
        return Err(format!(
            "invalid package name `{name}`: use 1-64 characters, a lowercase ASCII letter followed by lowercase letters, digits or `_`"
        ));
    }
    if RESERVED_NAMES.contains(&name) {
        return Err(format!(
            "`{name}` is a reserved name and cannot be used as a package name"
        ));
    }
    if beaver_syntax_keyword(name) || matches!(name, "print" | "len" | "range" | "ok" | "fail") {
        return Err(format!(
            "`{name}` is a Beaver keyword or builtin and cannot be used as a package name"
        ));
    }
    Ok(())
}

fn beaver_syntax_keyword(s: &str) -> bool {
    matches!(
        s,
        "fn" | "type"
            | "pub"
            | "const"
            | "let"
            | "var"
            | "if"
            | "elif"
            | "else"
            | "while"
            | "for"
            | "in"
            | "break"
            | "continue"
            | "scope"
            | "pass"
            | "return"
            | "use"
            | "test"
            | "true"
            | "false"
            | "and"
            | "or"
            | "not"
            | "view"
            | "edit"
            | "take"
            | "try"
    )
}

/// A relative path strictly inside the package: no `..`, no absolute or
/// prefixed components, under `src/`, ending in `.bv`.
fn validate_entry(entry: &str) -> Result<PathBuf, String> {
    let p = Path::new(entry);
    if entry.contains('\\') || entry.contains('\0') {
        return Err(format!("entry `{entry}` must use `/` separators"));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            Component::CurDir => {}
            _ => return Err(format!("entry `{entry}` must be a relative path inside the package (no `..` or absolute paths)")),
        }
    }
    if !out.starts_with("src") || out.components().count() < 2 {
        return Err(format!("entry `{entry}` must be a file under `src/`"));
    }
    if out.extension().and_then(|e| e.to_str()) != Some("bv") {
        return Err(format!("entry `{entry}` must be a `.bv` file"));
    }
    Ok(out)
}

pub fn parse_manifest(text: &str, origin: &str) -> Result<Manifest, Diagnostic> {
    let err = |msg: String| Diagnostic::global("B6001", format!("{origin}: {msg}"));
    let raw: RawManifest =
        toml::from_str(text).map_err(|e| err(format!("invalid manifest: {}", e.message())))?;
    let p = raw.package;
    validate_name(&p.name).map_err(err)?;
    let version = Version::parse(&p.version).map_err(err)?;
    if p.edition != EDITION {
        return Err(err(format!(
            "unsupported edition `{}` (this compiler supports edition \"{EDITION}\")",
            p.edition
        )));
    }
    let kind = match p.kind.as_str() {
        "bin" => Kind::Bin,
        "lib" => Kind::Lib,
        other => {
            return Err(err(format!(
                "`kind` must be \"bin\" or \"lib\", not `{other}`"
            )))
        }
    };
    let entry_str = p.entry.unwrap_or_else(|| match kind {
        Kind::Bin => "src/main.bv".into(),
        Kind::Lib => "src/lib.bv".into(),
    });
    let entry = validate_entry(&entry_str).map_err(err)?;
    let mut deps = BTreeMap::new();
    for (name, value) in raw.dependencies {
        validate_name(&name).map_err(|m| err(format!("dependency {m}")))?;
        if name == p.name {
            return Err(err(format!("package `{name}` cannot depend on itself")));
        }
        let table = match value {
            toml::Value::Table(t) => t,
            toml::Value::String(s) => {
                return Err(err(format!(
                    "dependency `{name}` must be a table: write `{name} = {{ version = \"{s}\" }}`"
                )))
            }
            _ => {
                return Err(err(format!(
                    "dependency `{name}` must be a table with `version` or `path`"
                )))
            }
        };
        let dep: RawDep = toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| err(format!("dependency `{name}`: {}", e.message())))?;
        let spec = match (dep.version, dep.path) {
            (Some(v), None) => DepSpec::Registry(
                Version::parse(&v).map_err(|m| err(format!("dependency `{name}`: {m}")))?,
            ),
            (None, Some(path)) => {
                if path.is_empty() || path.contains('\0') {
                    return Err(err(format!("dependency `{name}` has an invalid path")));
                }
                DepSpec::Path(path)
            }
            (Some(_), Some(_)) => {
                return Err(err(format!(
                    "dependency `{name}` cannot have both `version` and `path`"
                )))
            }
            (None, None) => {
                return Err(err(format!(
                    "dependency `{name}` needs `version` or `path`"
                )))
            }
        };
        deps.insert(name, spec);
    }
    Ok(Manifest {
        name: p.name,
        version,
        kind,
        entry,
        deps,
    })
}

pub fn load_manifest(dir: &Path) -> Result<Manifest, Diagnostic> {
    let path = dir.join(MANIFEST);
    let meta = std::fs::symlink_metadata(&path).map_err(|_| {
        Diagnostic::global(
            "B6001",
            format!("no `{MANIFEST}` found in `{}`", dir.display()),
        )
        .with_help("run `beaver new NAME` or `beaver init` to create a Beaver package")
    })?;
    if !meta.is_file() {
        return Err(Diagnostic::global(
            "B6001",
            format!("`{}` must be a regular file", path.display()),
        ));
    }
    if meta.len() > 1024 * 1024 {
        return Err(Diagnostic::global(
            "B6001",
            format!("`{}` is too large", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Diagnostic::global("B6001", format!("cannot read `{}`: {e}", path.display()))
    })?;
    let m = parse_manifest(&text, &path.display().to_string())?;
    check_entry_on_disk(dir, &m)?;
    Ok(m)
}

/// The entry file must exist as a regular file inside the package (no
/// symlink escapes).
pub fn check_entry_on_disk(dir: &Path, m: &Manifest) -> Result<(), Diagnostic> {
    let entry = dir.join(&m.entry);
    let bad = |msg: String| Diagnostic::global("B6001", msg);
    let mut cur = dir.to_path_buf();
    for c in m.entry.components() {
        cur.push(c);
        if let Ok(md) = std::fs::symlink_metadata(&cur) {
            if md.file_type().is_symlink() {
                return Err(bad(format!(
                    "entry path `{}` goes through a symlink; symlinks are not allowed",
                    m.entry.display()
                )));
            }
        }
    }
    match std::fs::symlink_metadata(&entry) {
        Ok(md) if md.is_file() => Ok(()),
        _ => Err(bad(format!(
            "entry file `{}` does not exist in package `{}`",
            m.entry.display(),
            m.name
        ))),
    }
}

/// Render a fresh manifest for `beaver new` / `beaver init`.
pub fn render_new(name: &str, kind: Kind) -> String {
    let entry = match kind {
        Kind::Bin => "src/main.bv",
        Kind::Lib => "src/lib.bv",
    };
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"{EDITION}\"\nkind = \"{}\"\nentry = \"{entry}\"\n\n[dependencies]\n",
        kind.as_str()
    )
}

/// Edit `[dependencies]` preserving the rest of the document's formatting.
pub fn edit_dependency(
    text: &str,
    name: &str,
    spec: Option<&DepSpec>,
) -> Result<String, Diagnostic> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        Diagnostic::global("B6001", format!("cannot edit manifest: {e}"))
    })?;
    if !doc.contains_key("dependencies") {
        doc["dependencies"] = toml_edit::table();
    }
    let deps = doc["dependencies"]
        .as_table_mut()
        .ok_or_else(|| Diagnostic::global("B6001", "`[dependencies]` must be a table"))?;
    match spec {
        None => {
            deps.remove(name);
        }
        Some(spec) => {
            let mut t = toml_edit::InlineTable::new();
            match spec {
                DepSpec::Registry(v) => {
                    t.insert("version", v.to_string().into());
                }
                DepSpec::Path(p) => {
                    t.insert("path", p.as_str().into());
                }
            }
            deps.insert(name, toml_edit::value(t));
        }
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nedition = \"2026\"\nkind = \"bin\"\nentry = \"src/main.bv\"\n\n[dependencies]\n";

    #[test]
    fn parses_spec_examples() {
        let m = parse_manifest(BIN, "t").unwrap();
        assert_eq!(m.name, "hello");
        assert_eq!(m.kind, Kind::Bin);
        let m = parse_manifest(
            "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2026\"\nkind = \"bin\"\n\n[dependencies]\nmathkit = { version = \"1.0.0\" }\nlocalutil = { path = \"../localutil\" }\n",
            "t",
        )
        .unwrap();
        assert_eq!(m.entry, PathBuf::from("src/main.bv"));
        assert_eq!(
            m.deps["mathkit"],
            DepSpec::Registry(Version::parse("1.0.0").unwrap())
        );
        assert_eq!(m.deps["localutil"], DepSpec::Path("../localutil".into()));
    }

    #[test]
    fn rejects_bad_names() {
        for n in [
            "Hello",
            "1abc",
            "a-b",
            "a/b",
            "con",
            "type",
            "",
            &"a".repeat(65),
        ] {
            assert!(validate_name(n).is_err(), "{n}");
        }
        assert!(validate_name("math_kit2").is_ok());
    }

    #[test]
    fn rejects_bad_versions() {
        for v in ["1.0", "1.0.0-beta", "^1.0.0", "01.0.0", "1.0.0+b", "a.b.c"] {
            assert!(Version::parse(v).is_err(), "{v}");
        }
    }

    #[test]
    fn rejects_bad_entries() {
        for e in [
            "../x.bv",
            "/etc/x.bv",
            "src/../../x.bv",
            "main.bv",
            "src/main.txt",
        ] {
            let t = BIN.replace("src/main.bv", e);
            assert!(parse_manifest(&t, "t").is_err(), "{e}");
        }
    }

    #[test]
    fn rejects_unknown_keys_and_conflicts() {
        assert!(parse_manifest(&format!("{BIN}\n[extra]\n"), "t").is_err());
        assert!(parse_manifest(
            &BIN.replace("kind = \"bin\"", "kind = \"bin\"\nauthors = []"),
            "t"
        )
        .is_err());
        let both = format!("{BIN}x = {{ version = \"1.0.0\", path = \"../x\" }}\n");
        assert!(parse_manifest(&both, "t").is_err());
        let string_form = format!("{BIN}x = \"1.0.0\"\n");
        assert!(parse_manifest(&string_form, "t").is_err());
        let selfdep = format!("{BIN}hello = {{ version = \"1.0.0\" }}\n");
        assert!(parse_manifest(&selfdep, "t").is_err());
        assert!(parse_manifest(&BIN.replace("2026", "2025"), "t").is_err());
    }

    #[test]
    fn edit_preserves_other_content() {
        let text = format!("# my app\n{BIN}");
        let added = edit_dependency(
            &text,
            "mathkit",
            Some(&DepSpec::Registry(Version::parse("1.0.0").unwrap())),
        )
        .unwrap();
        assert!(added.starts_with("# my app\n"));
        let m = parse_manifest(&added, "t").unwrap();
        assert!(m.deps.contains_key("mathkit"));
        let removed = edit_dependency(&added, "mathkit", None).unwrap();
        assert!(parse_manifest(&removed, "t").unwrap().deps.is_empty());
    }
}
