# Beaver 0.1: Astra Compiler Handoff

This package is a source-controlled, implementation-ready handoff for an autonomous coding model tasked with building **a real Beaver compiler and toolchain**.

**Status:** specification and test fixtures; **not** a working compiler. The implementation must be written by Astra. All `.bv` examples are proposed Beaver programs, not yet executable.

## Read in this order

1. `ASTRA_START_HERE.md` - executable assignment and completion criteria.
2. `docs/01_SCOPE_AND_DECISIONS.md` - immutable Beaver 0.1 feature boundary.
3. `docs/02_LANGUAGE_SPEC.md` - syntax, types, and exact semantics.
4. `docs/03_MEMORY_AND_SAFETY.md` - ownership, references, and lifetime checks.
5. `docs/04_COMPILER_ARCHITECTURE.md` - passes, IR, code generation, runtime.
6. `docs/05_CLI_PACKAGES_REGISTRY.md` - project layout and remote package protocol.
7. `docs/06_DIAGNOSTICS_TESTS.md` - errors, tests, fixtures, acceptance tests.
8. `docs/07_IMPLEMENTATION_PLAN.md` - ordered milestones and definitions of done.
9. `docs/08_REPOSITORY_AND_ENGINEERING.md` - proposed Rust workspace and engineering practices.
10. `docs/09_OPEN_QUESTIONS_AND_DEFERRED.md` - intentional limitations and future work.
11. `docs/10_REFERENCE_MATERIAL.md` - official reference documentation and tooling pointers.
12. `spec/beaver.ebnf` - syntax reference and lexer notes.
13. `examples/`, `fixtures/`, `tools/` - sample source and local-registry test generator.

## Product goal

Beaver is a native, statically typed, indentation-based language. It favors Python-like readability with deterministic ownership and no user-visible C `*` / `&` pointer syntax in safe code. Its first compiler is implemented in Rust and emits portable C11, then invokes an available C compiler to produce a native executable.

Do not reinterpret this as a request to transpile Beaver into Python or to merely design a grammar. The user wants a compiled language, type checker, safety analysis, runtime, CLI, package management, download validation, and end-to-end tests.

## Important distinction

Compiler-decided *allocation strategies* and *destruction points* are possible; allocating arbitrarily sized runtime data entirely at compile time is not. In v0.1, scalars and copyable records are often C locals; strings, lists, and buffers can use runtime heap storage and are deterministically freed by generated code. No garbage collector is required.

## Version-0.1 success demonstration

After Astra implements the project, a fresh machine with Rust and a C11 compiler should be able to:

```sh
cargo build --release
./target/release/beaver new hello
cd hello
../target/release/beaver run
../target/release/beaver test
../target/release/beaver build --release
```

The exact binary path will depend on where `hello` is created; the commands above are illustrative. The binary must generate native executables, reject invalid reference usage at compile time, and download an external Beaver library from a locally hosted mock registry after SHA-256 verification.

## Non-goals

No LLVM backend, no public hosted registry, no package publishing API, no raw pointers, no asynchronous runtime, no classes, no OS kernel programming, and no cross-platform debugger are needed for v0.1.

## Research inputs

The package format borrows established ideas from Cargo: declarative manifests, exact lockfiles, checksummed archives, and reproducible dependency fetching. The protocol documented here is **a custom Beaver protocol**, not an implementation of the crates.io protocol.
