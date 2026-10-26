# 01 - Beaver 0.1 Scope and Design Freeze

## Positioning

Beaver is a Python-readable, statically typed, ahead-of-time compiled systems-oriented language, with lexically checked references, explicit ownership transfer, and deterministic destruction. It has no user-visible raw pointer syntax in safe source code.

## Design invariants

1. No `*ptr`, `&ptr`, or pointer arithmetic in safe Beaver code.
2. `view` = read borrow, `edit` = exclusive mutable borrow, `take` = ownership move.
3. An owned non-Copy value has exactly one logical owner at a time.
4. A value cannot be used after `take` until the binding is initialized anew (reinitialization of moved bindings is *deferred* in 0.1, so it remains unavailable).
5. The compiler rejects conflicts and escaping references before code generation.
6. Safe runtime indexing, integer arithmetic, and allocation operations have defined behavior; runtime traps are acceptable for unrecoverable failures.
7. No runtime garbage collector; runtime heap allocation remains necessary for dynamic data.
8. A dependency artifact is never trusted merely because it came from HTTPS: archive hashes and extraction safety checks are mandatory.
9. Only Beaver source dependencies; no download-time scripts, dynamic native plugins, or automatic code execution as part of package resolution.
10. Same source + compiler + dependency lockfile + host target should produce equivalent behavior. Bit-identical native binary output is not promised across toolchains.

## Required in v0.1

- UTF-8 lexer; four-space canonical indentation; no tabs in leading whitespace.
- File extension `.bv`; case-sensitive identifiers; lowercase keyword spelling.
- Top-level `fn`, `type`, `const`, `use`, and optional `pub` on functions/types/constants.
- Statement-level `let`, `var`, assignment, `return`, `if`, `elif`, `else`, `while`, `for`, `break`, `continue`, `scope`, `pass`, expression calls.
- Builtins `Int` (signed 64), `Float` (IEEE-754 64), `Bool`, `Byte` (unsigned 8), `String` (immutable UTF-8), `Void`, `List[T]`, `Buffer` (byte buffer), `Result[T]`.
- Expressions with arithmetic, comparison, Boolean logic, calls, record constructors, list literals, field/index access, and `view`/`edit`/`take`.
- Type inference for locals; explicit parameter and function return types except implicit `Void` return.
- Data-only record types, not classes or methods.
- Bounds checks; checked `Int` operations; safe integer conversion constructors.
- Stack/record values, runtime-managed Strings/Lists/Buffers/Results; deterministic drops.
- Cross-file and cross-package modules with controlled visibility and namespacing.
- Built-in basic recoverable error type `Result[T]` with String errors.
- `test "name":` blocks under `tests/*.bv` plus assertions.
- Executable and library packages, TOML manifests, lockfile, archives, HTTPS downloads with checksum validation.
- macOS and Linux development hosts with a usable C11 compiler; x86_64 and aarch64 where the host compiler supports them. Windows later.

## Explicitly out of v0.1

- `share`, weak references, cycle collection; async, threads and concurrency primitives.
- Raw memory addresses, `unsafe`, C FFI, manual custom allocators, kernel/device-driver APIs.
- Lifetime annotations, borrowed return values, references stored in records/lists/results.
- Generics defined by users, interfaces, protocols, traits, methods, operator overloading, enum declarations, classes or inheritance.
- Full Unicode grapheme indexing, String indexing, interpolation, advanced format strings.
- Pattern matching, exception handling, user-defined error unions, `Result[T, E]`.
- Closures, lambdas, nested function definitions, reflection, metaprogramming, macros.
- Package publish server, namespace ownership, authentication, credentials, signing, build scripts.
- SemVer ranges and automatic version selection; only exact `major.minor.patch` dependencies.
- Whole-program escape analysis, aggressive stack promotion, JIT, LLVM/Cranelift backend, LSP/debugger.

## Key compromises

- Borrowed references bound to locals last for the **remainder of the lexical scope**, not last-use lifetime. Use `scope:` to end a borrow explicitly. This is an intentional MVP simplification.
- Function call borrows for `view`/`edit` parameters expire at the end of that call expression statement.
- By-value parameters for Copy types copy; unadorned parameters for non-Copy types are read-only borrows. `take T` must explicitly move ownership. `edit T` is explicitly mutable.
- Borrowing a record field borrows its *entire owner* in v0.1. Disjoint-field alias optimization is deferred.
- `List[T]` may own Copy or non-Copy elements; partial moves out of an indexed location are not allowed.
- Type-error behavior must be conservative rather than unsound. Clear errors trump accepting borderline code.
- v0.1 uses a simple C11 codegen backend. Semantics must not be defined by incidental C undefined behavior.

## Nonfunctional targets

- Plain `cargo build --release` (no proprietary services).
- No network access needed for building the Rust compiler or for Beaver projects with zero or cached dependencies after Rust crates are locally available.
- Release builds should use appropriate C optimization; no fixed throughput promise.
- Errors consistently show filename, line, column and a stable error category.
- Package fetching streams downloads and enforces hard size ceilings.
- Every safety invariant covered by both positive and negative tests.
