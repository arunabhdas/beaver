# Master Implementation Prompt for Astra

You are the lead compiler engineer for **Beaver**, a new programming language. Implement Beaver version **0.1** as a production-minded, functioning MVP, including a native compiler, bundled runtime, package/project tooling, remote package downloads, and tests.

The attached documents are the binding requirements, not suggestions. Read `README.md` and all `docs/` files before editing code. Use the language examples and fixture descriptions as acceptance contracts. In case of conflict, prioritize the detailed language and memory specifications over illustrative prose; record remaining conflicts as ADRs instead of silently inventing semantics.

## Deliverable

Create a Rust workspace that builds the `beaver` command-line program. Implement real functionality, not placeholder subcommands or parser-only demonstrations. The compiler must read `.bv` files, parse, type-check, validate ownership/lifetimes, lower to a typed intermediate representation, emit C11, invoke the host C compiler, and produce runnable native binaries. Make `beaver check`, `run`, `test`, `fmt`, project creation, package creation, dependency adding/removing/updating/fetching, and mock-registry downloads work.

## Required architecture

- Rust compiler and CLI (stable Rust; no nightly required).
- Distinct modules/crates for lexer, parser/AST, diagnostics, module resolution, type checking, borrow/ownership checking, MIR, C11 backend, runtime, package manager, and CLI.
- C11 as the initial native backend. LLVM or Cranelift are future options, not prerequisites.
- Internal typed representation with explicit move/borrow/drop operations. **Never** rely on loose string substitution to translate source into C.
- Single safe compilation path; the C backend must not bypass a failed type or ownership check.
- Owned String/List/Buffer/Result values are cleaned up deterministically and exactly once along all control-flow paths.
- Lexical scopes for bound references in 0.1; temporary call borrows end at statement boundaries. This is intentionally more restrictive than Rust's non-lexical lifetimes.
- Runtime must avoid signed-overflow undefined behavior and must check array/buffer bounds and division by zero.
- Diagnostic messages must include source locations and durable error codes.

## Implementation behavior

1. Work in milestones from `docs/07_IMPLEMENTATION_PLAN.md`, maintaining a compiling repository after each vertical slice.
2. Prefer deterministic, comprehensible implementations to sophisticated optimization. Do **not** build escape-analysis optimization before a sound and tested ownership checker.
3. Use Rust stable crates where appropriate (e.g., clap, serde, toml, sha2, tar, flate2, a maintained HTTPS client). Pin versions and commit `Cargo.lock`.
4. Run unit, integration, compile-fail, runtime-safety, and mock-registry tests. Report exactly what passed; do not claim tests passed if they were not executed.
5. Implement tests before or alongside each feature, including negative tests. Failing tests are not acceptable as a final deliverable.
6. Use fixture registry for end-to-end package installation, with `http://127.0.0.1` allowed **only** under explicit local-test opt-in. Production requests must use HTTPS.
7. Do not run installation/build scripts shipped inside dependencies. Do not extract symlinks, hardlinks, absolute paths, or `..` traversal paths from archives.
8. Do not add features outside the v0.1 scope to compensate for unimplemented required features.
9. If an implementation detail is unclear, choose the smallest conservative behavior consistent with the written semantics, add a test, and document it in an ADR. Do not substitute a less safe behavior just because it compiles.
10. If you cannot complete everything in one execution, deliver a genuinely compiling and tested vertical slice plus a precise remaining task list. Never mark unfinished features as completed.

## Mandatory CLI

`beaver new`, `init`, `check`, `build`, `run`, `test`, `fmt`, `add`, `remove`, `update`, `fetch`, `package`, `--help`, `--version`. `--offline` and `--locked` are required where dependencies are read; `--release` is required for `build` and `run`. See exact contracts in `docs/05_CLI_PACKAGES_REGISTRY.md`.

## Mandatory language capabilities

- Blocks delimited by indentation; comments; clear parse errors.
- `let`, `var`, `const`; type inference; Bool/Byte/Int/Float/String/Void.
- Expressions, arithmetic, short-circuit logic, comparisons, checked overflow and division.
- `fn`, recursion, `return`, `if`/`elif`/`else`, `while`, `for ... in range(...)`, `for ... in list`, `break`, `continue`, `scope`.
- Data-only `type` records and constructors; field access; `List[T]`, `Buffer`, `Result[T]` and `try`.
- `use` and `pub`, modules, package imports, statically linked libraries.
- `view`, `edit`, `take`; compile-time use-after-move and conflicting-borrow rejection.
- Builtin `print`, `assert`, `len`, `append`, `clone`, `ok`, `fail`, `try`, `is_ok`, `unwrap`.
- Top-level test blocks in test files and `beaver test` execution.

## Acceptance gate

The implementation cannot be called complete until all conditions below hold:

- A freshly generated `beaver new demo` project builds, runs, and prints `Hello, Beaver!`.
- A multi-file Beaver application imports a local module and a separate Beaver library package.
- Source compiles to real native machine code via generated C11 and a host compiler (not interpretation).
- Valid owned values are released exactly once on normal exits, early returns, `break`, and `continue`.
- Invalid nested borrows, use-after-move, invalid assignments, and escaping references fail `beaver check`.
- Out-of-bounds and integer-overflow errors terminate safely and deterministically without C undefined behavior.
- `Result[T]` and `try` correctly propagate owned errors.
- `beaver package` creates a deterministic archive.
- `beaver add mathkit@1.0.0` resolves and downloads the fixture package and writes `beaver.lock`.
- Corrupted checksum, traversal archive, version conflict, cyclic dependency, and insecure remote HTTP are rejected.
- `beaver fetch --offline --locked` works with a warm cache, and fails clearly with an empty cache.
- Full test suite and clippy/formatter pass on at least one documented supported host toolchain; CI also exercises the supported platform matrix.

## Final output Astra must provide

1. Entire source tree and reproducible build instructions.
2. README describing the supported Beaver syntax and commands.
3. Proof via real command output: `cargo test`, `cargo clippy`, `beaver test`, compiling example programs, compiler diagnostics, package download/corruption tests.
4. List of implemented features and explicit remaining limitations.
5. A brief technical summary of how borrow checking, MIR drops, checked arithmetic, and package integrity are implemented.

Begin implementing immediately. Do not stop after writing a plan or skeleton. The priority order is a **correct, working compiler** followed by secure, reproducible package workflows and polish.
