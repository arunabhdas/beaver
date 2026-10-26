# 07 - Astra Execution Plan: Vertical Milestones and Exit Gates

The milestones below are implementation order, not separate releases. Do not report Beaver 0.1 complete until all exit gates pass. Commit compiling code at each milestone and preserve existing tests. Build the smallest working vertical slice first; do not attempt the entire compiler through stubs with unimplemented branches.

## M0 - Repository, CLI, source manager, diagnostics

Tasks:
- Create Rust workspace with CLI executable `beaver` and internal library crates/modules.
- Implement `--help`, `--version`, `new`, `init`, manifest schema validation, source discovery, structured source spans/diagnostics.
- Add Rust formatting/lint/test CI. Include a README showing how to build the compiler.
- Implement lexer with `INDENT/DEDENT/NEWLINE`, comments, strings, numbers, operators, UTF-8 handling.

Exit gate: `cargo build`, `cargo test`, `beaver new hello`, `beaver init` work. Lexer tests cover invalid indentation and String escapes. New project skeleton is correct. Commands not yet implemented fail explicitly with "not yet implemented" during development, never silently succeed.

## M1 - Parser, typed primitives, basic native compilation

Tasks:
- Pratt/recursive-descent parser with error spans; AST with functions/statements/expressions.
- Type-check primitive types, simple functions, let/var, arithmetic, calls and return-path validation.
- Add a minimal typed MIR, C emitter and C runtime for checked arithmetic, print and assertions.
- Implement `beaver check`, `build`, `run`; target directory conventions; `BEAVER_CC` override.

Exit gate: `fn main(): print("Hello, Beaver!")` compiles to a native binary via C11. Factorial/recurse, `if`, `while`, `range`, `break`, `continue`, Boolean short-circuit programs pass. Out-of-range integer literal/type mismatch/invalid return are diagnosed. Host C compiler absence is actionable.

## M2 - Records, Strings, Lists, Buffer, and cleanup

Tasks:
- Define record types, typed constructors, field reads/writes, Copy trait computed structurally.
- Implement owning immutable UTF-8 String, String concatenation, clone, equality and printing.
- Implement List[T] and Buffer with all checks and element-specific drop/clone routines.
- Add MIR drop elaboration on normal scope exits, early return, break, continue.
- Add test-only runtime allocation/free counters and sanitizer-enabled integration tests.

Exit gate: records, `List[String]`, `Buffer`, list iteration, explicit append and deep clones execute safely. All owned heap values are dropped exactly once in the positive tests. Access past list/buffer bounds traps, cannot corrupt memory. No implicit copying of non-Copy values.

## M3 - Explicit ownership and lexical borrow checker

Tasks:
- Enforce `view`, `edit`, `take` in variable bindings, functions, arguments, records, lists and returns.
- Track moved values, borrow conflicts, conservative root-owner aliasing, and `scope:` lexical endings.
- Reject invalid borrow returns/escapes, concurrent mutation of an iterated list, invalid lvalues and incompatible conditional state joins.
- Test nested scopes, early returns, loops, result of moving non-Copy types, and drop correctness.

Exit gate: every `fixtures/compile_fail` case fails with the correct category, valid scope-separated read/edit sequence passes; sanitized C execution shows no use-after-free/double free. Compiler cannot emit native code from a failed ownership check.

## M4 - Basic error handling and tests

Tasks:
- Implement `Result[T]`, `ok`, `fail`, `try`, `is_ok`, `unwrap`, `unwrap_error` with correct ownership.
- Add top-level `test "label":` blocks, native test-driver compilation, assertions and reporting.
- Implement `beaver fmt` with stable 4-space output preserving strings/comments and `fmt --check`.

Exit gate: nested `try` success and failure propagate, drop both inactive/active union variants correctly, type-check `Result[T]`; `beaver test` reports named tests and exits correctly. `fmt` twice is byte-identical to `fmt` once.

## M5 - Modules, visibility, package metadata, and local deps

Tasks:
- Implement module path/import resolution, duplicate and cycle detection, `pub` visibility.
- Cross-module and cross-package naming with stable C mangle.
- Implement manifest validation, library creation, `beaver add --path`, `remove`, and local dependency builds.
- Implement package archive validation and deterministic `beaver package` for libraries.

Exit gate: a binary application imports a local module and local `mathkit` library, and cannot access private functions. Deterministic `beaver package` repeated runs have the same SHA-256. Invalid manifest paths/symlinks are rejected.

## M6 - Remote registry, dependency graph, lockfiles, cache

Tasks:
- Implement config/environment and remote registry URL policy, metadata endpoint, streaming HTTP downloads, SHA-256.
- Implement safe `.tar.gz` validation/extraction, content-addressed cache, exact transitive dependency resolution and cycles/version conflicts.
- Implement `add NAME@X.Y.Z`, `update NAME@X.Y.Z`, `fetch`, `remove`, lockfile generation; `--offline`/`--locked` across commands.
- Use mock registry fixture generator and local HTTP server. Test rollback and offline warm/empty cache.

Exit gate: fresh `consumer` resolves remote `mathkit@1.0.0`, runs, generates reproducible lock. Hash mismatch/traversal/HTTP policy/cycles/version conflicts fail with useful diagnostics and no file-system escapes or partially updated manifest.

## M7 - Hardening and release proof

Tasks:
- Run full conformance matrix; test with different C compilers and at least macOS+Linux where available.
- Run Rust lints, runtime sanitizers, stress recursive String/List ownership paths, fuzz parser if available.
- Verify docs, CLI help, reproducible archive and lock formatting, `--offline` with instrumented network denial.
- Replace all TODO/unimplemented paths in required v0.1 features with completed logic or honest scoped errors.
- Record code coverage and remaining explicitly out-of-scope features.

Exit gate: acceptance A01-A20 from `docs/06_DIAGNOSTICS_TESTS.md` pass, and all mandatory commands work. Deliver complete source, tested usage instructions, actual command logs, and risk/limitation list.

## Milestone tracking format

Astra should maintain `IMPLEMENTATION_STATUS.md` with a table:

```
| Milestone | Done? | Commit | Tests executed | Failing tests | Risks |
```

Before reporting completion for a milestone, execute at least `cargo fmt --all --check`, `cargo test --workspace`, the relevant integration tests, and an actual `beaver` command. Do not fabricate terminal output. If blocked, give the precise blocker and leave the source in a compilable state.

## Definition of complete

"Complete" means the toolchain can build and run real Beaver code, prove compile-time memory safety for the supported rules, and securely fetch and link source packages from the v1 registry protocol. A CLI help screen and parser-only repository do not satisfy this handoff.
