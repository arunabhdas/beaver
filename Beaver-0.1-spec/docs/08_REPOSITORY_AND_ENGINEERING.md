# 08 - Suggested Repository, Build and Engineering Conventions

The file layout is a recommended starting point, not an imposed crate partition. Keep semantic layering and public interfaces stable even if Astra chooses fewer Rust crates for speed.

```
beaver/
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  README.md
  CHANGELOG.md
  IMPLEMENTATION_STATUS.md
  crates/
    beaver-cli/          # `beaver` main and subcommands
    beaver-source/       # source manager, spans, diagnostics
    beaver-syntax/       # tokens, lexer, parser, AST, formatter
    beaver-sema/         # symbols, types, modules, ownership
    beaver-mir/          # typed CFG + explicit drops
    beaver-codegen-c/    # native C11 output and C compiler invocation
    beaver-package/      # TOML manifests, lockfiles, cache, registry
  runtime/
    include/beaver_rt.h
    src/beaver_rt.c
  tests/
    integration/
    compile_pass/
    compile_fail/
    runtime/
    packages/
  examples/
    hello/
    mathkit/
    consumer/
  .github/workflows/ci.yml
```

## Technology guidelines

- Rust stable only; checked in `Cargo.lock` and a supported toolchain file.
- CLI: `clap` derive. Parse manifest and lockfiles with a maintained TOML parser; use serde for strongly typed metadata.
- HTTP client: maintained Rust client supporting verified TLS, timeouts, redirect control and streaming. No handcrafted TLS.
- Checksums: maintained SHA-256 implementation. Package archives: maintained tar/gzip crates with safe manual entry validation.
- Source spans: byte offset plus computed line/col, no byte=character assumption for non-ASCII Strings/comments.
- Errors: typed `Result` in Rust internals, structured diagnostics, no panics on malformed Beaver source or package inputs.
- Generated paths and archives: stable ordering, safe joins/canonicalization, UTF-8 or explicitly documented supported paths.
- Noninteractive CLI by default; never prompt for package credentials; never run remote shell.
- Separate compilation phases in interfaces so a future LLVM/Cranelift backend can consume MIR without changing source semantics.

## Suggested Rust APIs (illustrative, not wire ABI)

```rust
pub struct CompileOptions {
    pub profile: BuildProfile,
    pub offline: bool,
    pub locked: bool,
}

pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub primary_span: SourceSpan,
    pub notes: Vec<String>,
}

pub fn lex(source: &SourceFile) -> Result<Vec<Token>, Vec<Diagnostic>>;
pub fn parse(source: &SourceFile, tokens: &[Token]) -> Result<Ast, Vec<Diagnostic>>;
pub fn check(program: &ResolvedProgram) -> Result<TypedProgram, Vec<Diagnostic>>;
pub fn borrow_check(program: &TypedProgram) -> Result<VerifiedMir, Vec<Diagnostic>>;
pub fn emit_c(mir: &VerifiedMir, out: &Path) -> Result<(), Vec<Diagnostic>>;
```

`VerifiedMir` should only be constructed by successful semantic and borrow checking; codegen should not accept an arbitrary unvalidated AST.

## Build/test contract

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
./target/release/beaver new hello
./target/release/beaver --version
```

The main README should list prerequisites: supported Rust stable toolchain, host `clang`/`cc` or equivalent C11 compiler, and platform requirements. Do not claim Windows support unless tested. No user-space C compiler installer is in scope.

## Design review checklist

- Is evaluation order identical regardless of host C compiler?
- Are there any paths from unvalidated source or dependency metadata to shell invocation?
- Can every owned non-Copy value be shown to have exactly one cleanup responsibility?
- Are parser diagnostics safe against malicious or truncated UTF-8 inputs?
- Can a malformed archive escape extraction root or exhaust memory/disk?
- Does an offline locked build avoid network access completely?
- Do CLI operations leave manifest and lockfile untouched when resolution fails?
- Can generated code use uninitialized C storage or invoke undefined C signed arithmetic?
- Is C backend independent from surface syntax to allow replacing code generation?

## Delivery hygiene

Astra must update documentation when code behavior changes, add tests for every fixed bug, and expose remaining limitations. The handoff is intentionally strict: never weaken memory or package safety merely to make a fixture pass.
