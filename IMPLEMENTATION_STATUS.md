# Beaver 0.1 implementation status

Last updated 2026-10-25. Every result below comes from commands actually executed on the
development host: macOS 26 (Darwin 25.6, arm64), Rust 1.98.1, Apple clang 21.0.0. Nothing on this
page was run on Linux, on gcc, or in CI. Those rows say so explicitly.

## Milestones

| Milestone | Done? | Commit | Tests executed (this host) | Failing tests | Risks / notes |
|---|---|---|---|---|---|
| M0: CLI, source manager, diagnostics, lexer | yes | uncommitted | `beaver-source` (5), `beaver-syntax` lexer unit tests, `workflow.rs` new/init/help | 0 | — |
| M1: parser, primitives, native compilation | yes | uncommitted | parser unit tests, conformance compile-pass/runtime, `workflow::new_binary_builds_runs_tests_and_releases` | 0 | — |
| M2: records, String, List, Buffer, drops | yes | uncommitted | 29 compile-pass programs with exact stdout and the runtime leak check, ASan+UBSan pass, `beaver-mir/tests/drops.rs` (9) | 0 | — |
| M3: ownership and lexical borrow checker | yes | uncommitted | 18/18 spec compile-fail fixtures + 52 more, `beaver-sema/tests/sema.rs` (30), pipeline mutation fuzz | 0 | conservative by design (ADR 0002, 0003) |
| M4: Result/try, tests, formatter | yes | uncommitted | `result_propagation.bv`, `try_failure.bv`, test-runner isolation test, fmt idempotence tests | 0 | — |
| M5: modules, visibility, local packages, `package` | yes | uncommitted | `workflow.rs` multi-file/visibility/cycle/package-determinism tests | 0 | — |
| M6: registry, lockfile, cache | yes | uncommitted | `registry.rs` (17) against the handoff mock registry plus generated hostile packages | 0 | real HTTPS/TLS registry not exercised (loopback HTTP only) |
| M7: hardening and release proof | partial | uncommitted | clippy `-D warnings`, `cargo fmt --check`, sanitizers, fuzzing, `cargo build --release` | 0 | CI matrix configured but **not run**; no coverage report; Linux/gcc untested |

"uncommitted" means the work is in the working tree, waiting for the maintainer to commit it.

## Acceptance matrix (docs/06 A01–A20)

| ID | Status | Evidence |
|---|---|---|
| A01 new package prints `Hello, Beaver!` | pass | `workflow::new_binary_builds_runs_tests_and_releases` |
| A02 Int/Float/Bool/Byte/String/records type-check | pass | compile-pass `floats`, `bytes`, `strings`, `records_nested`; `sema.rs` |
| A03 recursion, conditional returns | pass | `recursion.bv`, `recursion_lists.bv`, `while_true_returns.bv` |
| A04 `range`, break/continue | pass | `loops.bv`, `drops_break_continue.bv`, `evaluation_order.bv` |
| A05 List[Int], List[String], Buffer drop exactly once | pass | leak check on every compile-pass program, ASan, `drops.rs` |
| A06 views allowed, conflicting edit rejected | pass | `overlapping_borrow`, `lexical_borrow`, `sema.rs` |
| A07 use after `take` rejected | pass | `use_after_take` + 4 more |
| A08 `scope:` ends a borrow | pass | `lexical_borrow.bv`, `scopes_and_borrows.bv` |
| A09 bounds, div-by-zero, overflow trap safely | pass | 28 runtime programs (exit 101 + B5xxx), `runtime_arith.rs` |
| A10 Result/try propagate and drop correctly | pass | `result_propagation.bv`, `try_failure.bv`, `drops::try_error_path_*` |
| A11 local modules and `pub` enforced | pass | `workflow::visibility_imports_and_cycles_are_enforced` |
| A12 consumer with a local path library | pass | `workflow::multi_file_application_with_local_module_and_library_package` |
| A13 `beaver package` byte-identical | pass | `workflow::package_archives_are_deterministic_and_validated` |
| A14 `beaver add` downloads, verifies SHA-256, locks | pass | `registry::add_downloads_verifies_locks_and_runs_the_remote_library` |
| A15 `--offline --locked` warm cache, no HTTP | pass | `registry::offline_locked_*` (server request log unchanged) |
| A16 wrong hash, traversal, symlink, insecure HTTP fail | pass | `registry::corrupted_checksum_*`, `path_traversal_*`, `symlink_*`, `insecure_*` |
| A17 conflict and cycle fail cleanly | pass | `registry::dependency_cycles_and_version_conflicts_fail_cleanly` |
| A18 formatter idempotent, keeps comments | pass | `fmt` unit tests, `robustness::formatting_is_idempotent_*`, `workflow::fmt_*` |
| A19 generated C clean under warnings and sanitizers | pass on this host | every build in the suite uses `-Wall -Wextra -Werror`; ASan+UBSan pass |
| A20 `check` needs no C compiler | pass | `workflow::check_does_not_need_a_c_compiler` |

## Test inventory (last full run: `cargo test --workspace --locked`)

| Suite | Tests | Result |
|---|---|---|
| beaver-source unit | 5 | ok |
| beaver-syntax unit (lexer, parser, formatter) | 39 | ok |
| beaver-syntax robustness (mutation/random no-panic, fmt idempotence) | 3 | ok |
| beaver-sema (type + ownership checker) | 30 | ok |
| beaver-mir drop elaboration | 9 | ok |
| beaver-codegen-c unit | 2 | ok |
| runtime checked arithmetic (builtins and C11 fallback, >1000 cases each) | 2 | ok |
| whole-pipeline mutation fuzz (2340 mutants; accepted ones compiled `-Werror` and run under ASan+UBSan) | 2 | ok |
| beaver-package unit | 13 | ok |
| CLI conformance (29 compile-pass, 70 compile-fail, 29 runtime, sanitizer pass) | 4 | ok |
| CLI registry and security | 17 | ok |
| CLI workflows | 14 | ok |
| **Total** | **140** | **all passing** |

Also clean on this host: `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets --locked -- -D warnings`.

## Not done / remaining work

1. **CI not executed.** `.github/workflows/ci.yml` defines Linux x86_64 (gcc, clang), Linux
   aarch64 (gcc), macOS arm64 and macOS x86_64, but it has never run. gcc-specific `-Wextra`
   warnings on generated code are therefore unverified.
2. **No real HTTPS registry exercised.** TLS uses ureq 3 with rustls and webpki roots (certificate
   verification is on by default), but every end-to-end download test uses the allowed loopback
   HTTP path.
3. **No code-coverage report** was produced (M7 asks for one).
4. **Stack exhaustion** from deep recursion is not diagnosed; the process dies with the OS signal.
5. Language limitations are listed in README "Limitations of 0.1" and in the ADRs.
