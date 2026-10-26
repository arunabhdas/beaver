# 06 - Diagnostics, Test Strategy, and Acceptance Matrix

## Error contract

Diagnostic output should resemble:

```
error[B3002]: cannot edit `value` while it is read-borrowed
  --> src/main.bv:8:17
   |
 7 | let reader = view value
 8 | let writer = edit value
   |                 ^^^^^ conflicting exclusive borrow
   |
note: the read borrow remains active until its lexical scope ends
help: wrap `reader` in `scope:` to shorten the borrow lifetime
```

IDs are stable families (specific numbers illustrative):

| Prefix | Class |
|---|---|
| `B1xxx` | Lexing, indentation, parsing |
| `B2xxx` | Type checking, names, return analysis, imports |
| `B3xxx` | Moves, borrowing, lifetime, aliasing |
| `B4xxx` | Code-generation or host toolchain failure |
| `B5xxx` | Runtime checks (bounds, division, overflow, OOM, assertion) |
| `B6xxx` | Package metadata, registry, archive, checksum, lockfile |

Every compiler error: source path, one-based line/column, primary span, stable code, actionable explanation. A single syntax error must not crash the compiler. `--help` and bad argument handling must never panic.

## Test categories

1. Lexer unit tests: indentation, comments, CRLF, escapes, numeric literals, EOF, invalid UTF-8.
2. Parser snapshot tests: precedence, nested blocks, function signatures, record declarations, malformed source.
3. Type checker tests: valid inference, invalid mixed arithmetic, return checking, visibility, field types, missing imports.
4. Borrow checker tests: overlapping views/edits, borrowed return, use after take, edit immutable owner, loop borrows, owner-level field aliasing, move joins.
5. MIR/codegen tests: branch, loop, recursion, native record/list operations, left-to-right evaluation, short-circuiting, Result propagation.
6. Runtime tests: boundary indices, bounds traps, `INT64_MIN` cases, zero divisors, OOM guard/oversized allocation, String clone/concatenation, recursive drops.
7. Toolchain end-to-end tests: `new -> check -> run -> test -> build --release`, `fmt --check`, library module imports, correct exit propagation.
8. Package integration: path dependency, fixture HTTPS/loopback HTTP registry, cache/offline/locked behavior, checksum mismatch, corrupted archive, traversal, conflicting versions, dependency cycles, oversized metadata/archive.
9. Fuzz or property tests when feasible: lexer/parser inputs must never panic; independent reference evaluator for checked Int arithmetic; parser formatter idempotence.
10. Sanitizers (where available): compile C fixtures using ASan/UBSan, verify no double frees, out-of-bounds, use-after-free or leaks.

## Conformance suite convention

`fixtures/compile_fail/<name>.bv` is invalid source; matching `<name>.expect` contains the diagnostic code family or a required message substring. Tests must run `beaver check` and assert failure and correct error. `fixtures/runtime/*.bv` are valid source and should compile, run, and match the expected exit behavior in accompanying `.expect` files.

At minimum write 60 compiler unit tests, 20 compile-pass integration programs, 15 compile-fail tests, 10 runtime trap/cleanup tests, and 12 package/security integration tests. The supplied fixtures start the set, not exhaust it. Test the input-output contract rather than depending solely on snapshots of C output.

## Essential acceptance cases

| ID | Expected behavior |
|---|---|
| A01 | New binary package prints `Hello, Beaver!`. |
| A02 | `Int`, `Float`, `Bool`, `Byte`, Strings and records type-check. |
| A03 | Recursion and conditional return paths compile and execute. |
| A04 | `range` iterates [start, end) and supports break/continue. |
| A05 | `List[Int]`, `List[String]`, `Buffer` are safe and drop exactly once. |
| A06 | `view` aliases are allowed; conflicting `edit` while view active is rejected. |
| A07 | `take` then use is rejected, not undefined behavior at runtime. |
| A08 | `scope:` ends a named borrow and permits a later edit. |
| A09 | Index bounds, division-by-zero and Int overflow safely trap. |
| A10 | `Result[T]` success and error propagate with `try` and drop correctly. |
| A11 | Local module imports and `pub` visibility enforced. |
| A12 | Consumer compiles using a local path library. |
| A13 | `beaver package` yields identical archive bytes for identical inputs. |
| A14 | `beaver add` downloads fixture registry library, verifies SHA-256, locks exact version. |
| A15 | `--offline --locked` uses warm cache with no HTTP calls. |
| A16 | Wrong hash, traversal/symlink archive and insecure remote HTTP fail. |
| A17 | Dependency conflict/cycle fails cleanly, never hangs. |
| A18 | Formatter is idempotent, preserves source semantics and comments. |
| A19 | Generated C passes warnings/sanitizers on supported hosts. |
| A20 | `beaver check` does not depend on presence of a C compiler. |

## Test runner

`test "description":` in `tests/*.bv` creates a named test function in the generated C test driver. Tests execute in stable name/file ordering. Native `assert` failure causes nonzero test result, and the runner reports the failed test name (prefer isolated subprocess invocation per test so a single runtime trap doesn't suppress remaining tests; single-process tests may be accepted temporarily if the limitations are documented). Exit nonzero when any test fails.

## Package-security test requirements

- SHA mismatch must leave manifest/lockfile unmodified in `beaver add` and must not install/extract an artifact.
- Tar member `../../outside.txt` must be rejected; test must verify sentinel outside extraction root is unchanged.
- Symlink and hardlink archive entries must be rejected.
- 100+ MiB expansion bomb should be detected by declared and observed size checks without allocating a huge buffer (fixture can simulate limits using configurable test-only thresholds).
- HTTP hostname not loopback must not be allowed over plaintext, even with local opt-in.
- Redirects and archive URLs supplied by malicious metadata must never circumvent the registry origin policy.
- Duplicate package IDs, metadata-manifest mismatch, registry cycle and conflicting package versions must be diagnosed.
- No extracted package contains or invokes a build hook/script automatically.

## CI checks

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Unit parser/semantic tests
- End-to-end Beaver sample programs
- Package registry integration and adversarial archive suite
- C compiler `-Wall -Wextra` (and sanitizer pass on supported environment)
- CI matrix: at least current stable Rust on macOS and Linux; minimum 1 Intel and 1 ARM target where runners are available.
