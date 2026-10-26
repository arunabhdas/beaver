# Beaver 0.1 - Complete Single-File Astra Handoff

This file is a convenience concatenation of the normative handoff. The ZIP contains editable individual files, example projects, conformance fixtures and the static mock registry.



---

# Source: `README.md`

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


---

# Source: `ASTRA_START_HERE.md`

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


---

# Source: `AGENTS.md`

# Instructions for Autonomous Coding Agents (including Astra)

Read `ASTRA_START_HERE.md`, followed by `docs/01_SCOPE_AND_DECISIONS.md` through `docs/09_OPEN_QUESTIONS_AND_DEFERRED.md` before implementation.

Your job is to **implement a working Beaver 0.1 compiler and package manager**, not to write more unexecuted plans. The attached specs are mandatory; the examples, grammar, language fixtures and mock registry are test contracts. Finish the smallest correct vertical slice first and maintain `IMPLEMENTATION_STATUS.md` with actual test results.

Do not weaken ownership checking, runtime safety, or package verification to get a program to compile. Do not add a fake public registry URL. No publisher accounts, package upload service, LLVM backend, FFI, raw pointers, async runtime, classes, or shared ownership are necessary for v0.1.

If completion is partial, leave a genuinely compiling/tested repo and list incomplete milestones honestly. Do not mark placeholder functionality as complete.


---

# Source: `docs/01_SCOPE_AND_DECISIONS.md`

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


---

# Source: `docs/02_LANGUAGE_SPEC.md`

# 02 - Beaver 0.1 Language Specification

This document is normative. The grammar in `spec/beaver.ebnf` is a parsing aid; this document determines meaning and error behavior.

## 1. Files, layout and keywords

- UTF-8 `.bv` source; LF or CRLF accepted; normalized to LF internally.
- Indentation defines suites, Python-style. A suite begins after `:` followed by a newline and greater indentation. Indents may have 4, 8, 12, etc. spaces; `beaver fmt` normalizes to 4 per level. Reject tabs in leading indentation, mixed/deduced invalid indentation, missing suites, and inconsistent dedents.
- Case-sensitive identifiers: `[A-Za-z_][A-Za-z0-9_]*` in 0.1. Non-ASCII text is allowed in Strings and comments, but not identifiers.
- Comments begin with `#` outside a String and run to end of line. Blank/comment-only lines don't affect indentation.
- Reserved: `fn`, `type`, `pub`, `const`, `let`, `var`, `if`, `elif`, `else`, `while`, `for`, `in`, `break`, `continue`, `scope`, `pass`, `return`, `use`, `test`, `true`, `false`, `and`, `or`, `not`, `view`, `edit`, `take`, `try`. Builtin functions such as `range`, `ok` and `fail` are identifiers, not reserved words.
- No semicolons, braces, variable-length tabs, multiline Strings, tuple syntax, class declarations, user-defined generics or decorators.

## 2. Types and values

| Source type | Semantics | Representation suggestion | Copy? |
|---|---|---|---|
| `Int` | signed 64-bit integer | `int64_t` | yes |
| `Float` | IEEE-754 binary64 | `double` | yes |
| `Bool` | exactly `true`/`false` | `_Bool` | yes |
| `Byte` | unsigned 8-bit integer | `uint8_t` | yes |
| `String` | immutable UTF-8 bytes | ptr+length+ownership tag | no |
| `Void` | no value | C `void` | n/a |
| `type R:` | record with named fields | C struct | iff all fields Copy |
| `List[T]` | growable owning homogeneous sequence | ptr+len+capacity | no |
| `Buffer` | fixed-length owning zero-initialized byte storage | ptr+len | no |
| `Result[T]` | success `T` or error `String` | tagged union | no |

`T` for List and Result may be a named concrete type, built-in type, or a further nesting of built-in containers (subject to ordinary ownership checks). User-defined generic types/functions are NOT in v0.1. Recursive records by value are invalid, even through `List` if their compile-time value representation would be infinite; a `List` breaks value-size recursion because it has fixed-size header storage.

`Void` is allowed only as function return or `Result[Void]`; no `Void` variables, list elements, or record fields.

`String` is an owning value semantically, even if optimized as a static literal. UTF-8 is validated on source input; `len(s)` returns **byte length**, not grapheme count. No direct String indexing.

## 3. Bindings and assignment

```beaver
const LIMIT: Int = 100
let name = "Beaver"
var count: Int = 0
count = count + 1
```

`const` is top-level only, requires explicit type, and accepts compile-time literal or numeric/Boolean constant expressions only; String literal constants are allowed. It is globally immutable. `let` is immutable; `var` is assignable. Local type annotations are optional if inferrable. Shadowing in a nested block is allowed, re-declaration of the same name in a scope is an error. All variables are initialized at declaration. No reinitialization of moved locals in v0.1.

Assignment targets: mutable `var` bindings, mutable record fields accessible through a mutable owner, and checked index targets into mutable lists and buffers. Assigning to a `let`-owned record field is forbidden. A local `let alias = edit x` can mutate the referent with `alias = new_value`, because `let` fixes the reference binding, not the underlying storage; it cannot be rebound to refer to another object.

## 4. Expressions

- Literals: `123`, `-123` (unary operator + integer token), `1.5`, `true`, `false`, `"text"`, and list literals `[1, 2, 3]`.
- String escapes: `\\`, `\"`, `\n`, `\r`, `\t`, `\0`, `\u{HEX}` (valid Unicode scalar value encoded as UTF-8). No interpolation or triple-quoted Strings.
- Integer arithmetic `+ - * / %`, unary `-`; checked at runtime. Division truncates toward zero. Zero divisor and `INT64_MIN / -1` trap. Signed overflow traps instead of wrapping or C undefined behavior. `Byte` uses explicit `Byte(Int)` conversion and comparisons; no implicit Int/Byte arithmetic.
- Float arithmetic `+ - * /`, unary `-`, IEEE-754 semantics. Float division by zero follows IEEE-754 (Inf/NaN). No fast-math assumptions in 0.1.
- `==`, `!=` compare equal-compatible primitive values, Strings by contents, and records recursively only when all fields support equality. `List`, `Buffer`, `Result` equality is not in v0.1.
- `<`, `<=`, `>`, `>=` work for Int/Float/Byte; String ordering is out of scope.
- `and`, `or`, `not` operate on Bool only; `and`/`or` short-circuit left-to-right.
- `String + String` creates a new owned String (may optimize at compile time for literals). No implicit numeric-to-String conversion.
- Field selection `point.x`, indexed access `list[i]`/`buffer[i]`, call `fn(args)`, qualified symbol `module.fn(args)`.
- All expressions evaluated left-to-right; function arguments evaluated in source order.
- No implicit numeric conversions. Builtins `Float(Int)`, `Int(Float)` (finite and range checked), `Byte(Int)` (range checked) implement explicit conversion.
- No assignment expressions, bitwise operators, pointer arithmetic, method calls, operator overloading, ternary expression or increment/decrement operators.

Operator precedence (highest to lowest): grouping/call/field/index; unary `not`, unary `-`, `view`/`edit`/`take`/`try`; `* / %`; `+ -`; comparisons; `and`; `or`. Chained comparisons (`a < b < c`) are forbidden: use `a < b and b < c`. The parser must reject invalid lvalues for `edit` and `take`.

## 5. Functions

```beaver
fn add(a: Int, b: Int) -> Int:
    return a + b

fn say(text: String):
    print(text)

fn change(number: edit Int):
    number = number + 1

fn consume(text: take String):
    print(text)
```

- Functions are top-level declarations, optionally `pub`; all parameters require a type; default return is Void. No overloads, default parameter values, variadic user functions, or nested functions.
- Default parameter passing: Copy types pass by value; non-Copy types pass as read-only borrows. `view T` always means explicit shared immutable borrow, including for Copy T. `edit T` means an exclusive mutable borrow; `take T` moves ownership into callee.
- `f(value)` is valid for ordinary parameters (a read-only borrow is implicit for non-Copy). `f(edit value)` is mandatory for `edit T`; `f(take value)` mandatory for `take T` if value is named owner. Temporaries can be passed to `take` without writing `take`.
- Borrowed parameters cannot be returned, stored in records/lists/Results, or moved. `take` parameters can be transferred or returned with `take`.
- Non-Void functions must return on every reachable path; no implicit final expression return. A freshly constructed non-Copy temporary can be returned directly; returning a named owned non-Copy local requires `return take local`.
- `main` must have signature `fn main():` with no parameters and Void return for binary packages. Exactly one root `main`, not exported.
- Recursion is allowed. Stack exhaustion is a runtime failure and no stack-limit guarantee exists in 0.1.

## 6. Branching and loops

```beaver
if score > 80:
    print("high")
elif score > 50:
    print("medium")
else:
    print("low")

var i = 0
while i < 5:
    i += 1

for n in range(0, 5):
    print(n)
```

Assignment shorthands `+=`, `-=`, `*=`, `/=` are supported for mutable numeric lvalues and checked with the same semantics as expanded operations. `for n in range(start, end)` visits `start...end-1`, step +1; endpoints evaluated exactly once. `for value in list` traverses in order and holds a read borrow of the list for the loop duration. Copy elements bind a copy; non-Copy elements bind a read-only element reference per iteration. Mutating the iterated list during iteration is forbidden. `break`/`continue` require a loop. `scope:` creates an otherwise inert lexical scope (useful for ending borrows).

A moved owner across conditional branches is conservatively rejected if its ownership state is not identical at a control-flow join. Code that would require dynamic partial-move recovery is rejected in 0.1. `return`, `break`, and `continue` must release all still-owned values whose scope is exited.

## 7. Records

```beaver
pub type Point:
    x: Float
    y: Float

fn move(p: edit Point, dx: Float):
    p.x += dx

fn main():
    var p = Point(x=1.0, y=2.0)
    move(edit p, 3.0)
    print(p.x)
```

Records contain fields only. Constructor calls must provide all declared fields exactly once by name, in any order. Expressions evaluated left-to-right, then placed in declared field order. A non-Copy owned local supplied to a constructor must be explicitly moved with `take`. Records recursively own their fields. Borrowing a field locks the entire parent owner for alias analysis in v0.1.

## 8. Lists, buffers and builtins

```beaver
var values: List[Int] = [10, 20, 30]
append(edit values, 40)
print(len(values))
print(values[2])

var bytes = Buffer(size=16)
bytes[0] = Byte(65)
print(bytes[0])
```

- List literals infer `List[T]` from elements; empty `[]` requires an expected type. Elements must have one compatible static type. List owns elements; inserting a named non-Copy element requires `take`.
- `len(value)` returns Int for List/Buffer/String (String length is bytes).
- `append(edit list, element)` mutates a List, growing its capacity as needed; it takes owned non-Copy elements or copies Copy elements. No append on Buffer.
- Buffer is fixed-size, zero-initialized and indexed by checked Int to Byte. `Buffer(size=n)` takes a nonnegative Int, allocates at runtime, and may trap on OOM/oversize.
- `list[index]` yields a copy for Copy elements and a read-only borrow for non-Copy elements in read contexts. `take list[index]` is forbidden. `list[index] = expr` requires mutable list ownership or an exclusive edit borrow, and safely destroys any replaced owned element.
- `clone(value)` returns an owned deep clone for non-Copy types and a copy for Copy types. Clone can allocate and may fail with an OOM trap.
- `print(value)` prints one Int/Float/Byte/Bool/String followed by one newline. For a referenced primitive, it prints the referent. Printing List/Buffer/record/Result values is out of scope.
- `assert(condition)` requires Bool; traps with file/line if false. `assert_eq(a, b)` compares values with supported equality and traps with location if unequal.
- Basic intrinsics above are built into the compiler; there is no automatic import requirement for them.

## 9. Basic recoverable errors

`Result[T]` contains either an owned successful `T` value or an owned String error. It is always non-Copy.

```beaver
fn divide(a: Int, b: Int) -> Result[Int]:
    if b == 0:
        return fail("division by zero")
    return ok(a / b)

fn calculate() -> Result[Int]:
    let n = try divide(10, 2)
    return ok(n + 5)
```

- `ok(value)` constructs success; `ok()` for `Result[Void]`.
- `fail(string)` constructs error. The String is owned, copied by explicit `clone` if necessary, or moved with `take`.
- `try expr` is valid only when expr has `Result[T]` and the enclosing function returns `Result[U]`. On error, it returns the moved String error; on success, it yields the owned T value (or Void).
- `is_ok(result)` reads a Result by borrow and returns Bool.
- `unwrap(take result)` consumes a Result and yields the owned success or traps if error; `unwrap_error(take result)` yields the owned error or traps if success. Temporary results may be passed directly.
- Result is not an exception system. Arithmetic traps, OOM, bounds failures, and assertion failures are not recoverable with Result in 0.1.

## 10. Modules, imports, tests

- `use util` imports an in-package `src/util.bv` module; `use net.http` imports `src/net/http.bv`.
- `use mathkit` imports the entry library module `src/lib.bv` from a direct dependency named `mathkit`.
- `use mathkit.tools` imports that dependency's `src/tools.bv`.
- Calls use namespaces: `util.double(3)` or `mathkit.square(3)`. Re-exporting imported symbols is not supported.
- `pub` exposes top-level declarations to other modules; unmarked declarations remain module-private. Public functions cannot expose private record types in signatures.
- Import cycles are compile errors. Import names cannot conflict with locals or other imports in the same scope.
- Only `tests/*.bv` files may contain top-level `test "label":` blocks. The test runner compiles them into a native executable and reports per-test pass/fail. Tests may import modules and use `assert`/`assert_eq`.

## 11. Runtime observability and failures

- A normal `fn main()` returning completes with exit status 0.
- `assert` failures, failed `unwrap`, checked integer overflow, divide by zero, invalid conversion, out-of-bounds access and unhandled OOM terminate with a nonzero exit status and a readable stderr diagnostic; no undefined behavior.
- No guaranteed stack traces in 0.1; include source path/line if available in generated checks.
- User `print` goes to stdout; compiler diagnostics, package manager errors, and runtime failures go to stderr.

## 12. What the compiler must reject

Type mismatch; unknown symbol; duplicate symbol; uninitialized/moved uses; invalid `take` of borrowed owner; simultaneous `view` and `edit` overlapping the same owner; multiple `edit` aliases; editing an immutable value; returning or persisting borrowed refs; moving out of an indexed list or borrowed field; creating invalid by-value record cycles; unresolved import; `try` in non-Result function; return path missing in non-Void fn; indexing Strings; invalid package visibility; explicit `unsafe`/`share`/raw-pointer syntax as unavailable v0.1 features.


---

# Source: `docs/03_MEMORY_AND_SAFETY.md`

# 03 - Ownership, Borrowing, Destruction, and Memory Safety

## Ownership model

The compiler maintains a state for each owned local `available | moved`. Each non-Copy value has one owner. Assignment from a named owned non-Copy value requires `take`; a temporary is implicitly transferred. Borrowed references never own. Primitive scalars and recursively Copy records can be copied without `take`.

Readable operations:

```beaver
var x = 10
scope:
    let reader = view x
    print(reader)
scope:
    let writer = edit x
    writer = 20
print(x)
```

`view` produces a non-owning shared read reference, `edit` a non-owning unique mutable reference, `take` transfers the owner. Source never needs `*`, `&`, dereference operators, or pointer arithmetic. The C backend may use native pointers internally.

## Borrow timeline

- Local borrow bindings stay alive until their lexical scope closes, regardless of last use.
- A function argument borrow starts when the function call begins and ends at the end of the full call statement/expression evaluation. Nested expression borrows must have consistent ordering and must not alias a simultaneously edited owner.
- `scope:` creates explicit lifetime boundaries.
- Any number of `view` references may overlap. A single `edit` reference requires no active overlapping view/edit reference.
- Borrow conflict checks are conservative at *owner* granularity. `view person.name` conflicts with `edit person.age` when both are bound across a scope.
- A borrowed reference cannot outlive its owner, be returned, be assigned into heap structures or records, be captured (closures do not exist), or be moved.
- Named borrows are transparent for reading; an edit borrow can write to its referent via assignment or field/index lvalues.
- For `for item in list`, the list has a read borrow spanning the loop. Mutable mutation through another alias is a compile-time error.

## Examples: valid

```beaver
fn increment(value: edit Int):
    value += 1

fn main():
    var value = 3
    increment(edit value)
    scope:
        let r = view value
        print(r)
    increment(edit value)
```

```beaver
fn use_text(text: String):
    print(text)

fn main():
    let word = "hello"
    use_text(word)       # read borrow, not an ownership transfer
    use_text(word)       # still valid
```

```beaver
fn consume(text: take String):
    print(text)

fn main():
    let value = "hello"
    consume(take value)
    # value cannot be read or dropped again
```

## Examples: invalid

```beaver
var n = 1
let a = view n
let b = edit n     # error: a's lexical read borrow is still active
```

```beaver
let text = "hi"
let moved = take text
print(text)       # error: use after ownership transfer
```

```beaver
fn bad() -> String:
    let name = "hello"
    let alias = view name
    return alias   # error: borrowed values cannot escape
```

```beaver
var list = [1, 2]
for x in list:
    append(edit list, 3)  # error: active loop read borrow
```

## Deterministic drops

Create explicit `Move`, `BorrowRead`, `BorrowWrite`, `ReleaseBorrow`, and `DropOwned` operations in the typed MIR. Emit drops for active local owners at all scope exits, including `return`, `break`, `continue`, and normal fallthrough. Deconstructing a record recursively drops owned fields in reverse declaration order. Dropping a List recursively drops each live element, then the allocation. Dropping a Result frees exactly its active success or error payload; drop never touches inactive union storage.

After a move, the source's drop is disabled (statically or via generated drop-state bits); the destination becomes the unique owner. The compiler must not double-free conditional moves. In v0.1, reject paths whose post-join ownership states disagree instead of silently inserting untested dynamic partial-move machinery.

## Lifetime/type-state validation algorithm

Proposed initial algorithm (conservative):

1. Type-check all owners and borrowing expressions; assign each borrow to a root owner and a lexical or temporary scope ID.
2. Traverse typed control-flow CFG with states `OwnedAvailable`, `OwnedMoved`, `ReadBorrowed(n)`, `WriteBorrowed`, tracking active borrows scoped to call expressions or lexical blocks.
3. Reject `take` while any borrow is active; reject `edit` unless owner is mutable and has no active borrow; reject a read borrow if an edit exists.
4. Release named borrows at block scope exit and temporary borrows at call evaluation completion.
5. For joins, accept only state combinations that are equal for names live beyond the join; for loops, prevent ownership moves of outer-scope variables where the loop may iterate arbitrarily unless analysis can prove a valid fixed point. Conservative rejection is allowed.
6. Compute drop insertion using the ownership state on outgoing edges. If ownership state differs and no safe static drop plan is available, reject the program rather than generating unsafe code.
7. Reject returning references or indirect storage of borrow-only types.

Tests must verify compile errors AND runtime drops under all statement exits. For v0.1, `take` on an owner that is also aliased in a different record field is forbidden; field-level move is unsupported.

## Runtime representation guidance

- `String`: immutable UTF-8 ptr/len plus either static-data or unique heap-owner tag. All printing must be byte-length-aware rather than using an unbounded `%s` C format, since Strings may contain `\0`. Literal data can point to static program memory; concatenation/clone allocates heap storage. `Drop` frees only heap-owned payloads; integer length arithmetic checked for overflow.
- `List[T]`: `ptr/len/cap`, typed element size/alignment, capacity growth with overflow checks, element clone/drop operations specialized by concrete T. Do not use `memcpy` for non-Copy elements unless the bytewise transfer is a one-way move with old owner invalidated.
- `Buffer`: fixed-length zero-initialized allocation, ptr/len; no negative size; bounds checked reads/writes.
- `Result[T]`: tag with active union payload, owned success or error. `try` transfers active payload without double-drop.
- Records: inline fields, recursive drop/clone. Record with all Copy fields may be trivially assigned.
- Borrowed refs: C pointers/struct pointers, produced only after validation in compiler; no borrowed reference leaves its valid lifetime.

## Memory safety obligations

- **Bounds:** reject negative indices; guard `index < len` before pointer math or loads; compiler can omit a guard only after proof.
- **Arithmetic:** use checked helpers for `Int` add/sub/mul/div/mod/neg (with tests including signed limits); never rely on signed C overflow behavior.
- **Allocation:** detect size/capacity multiplication overflow before allocating. On OOM print a clear runtime error and exit nonzero. Never dereference NULL.
- **Initialization:** no reads of uninitialized memory; Bytes in a new Buffer are zero-filled, List elements initialized before `len` increments.
- **Copies:** no implicit copies of String/List/Buffer/Result/non-Copy records; require `clone` for duplication, `take` for transfer.
- **C backend:** warnings enabled; no UB from invalid shifts (none in 0.1), invalid pointer arithmetic, uninitialized unions, strict aliasing, C evaluation-order differences, or checked int overflow.
- **ABI:** internal representations are implementation details; no stable C FFI/ABI guarantee in 0.1.

## What APLA means for this MVP

No whole-program automatic stack/heap escape analysis is required. Use native locals and inline records where possible, and heap-allocate dynamic String/List/Buffer payloads. Compiler-enforced ownership, borrow validity and deterministic drops deliver the central safety model; sophisticated allocation placement can be added after correctness tests pass.


---

# Source: `docs/04_COMPILER_ARCHITECTURE.md`

# 04 - Compiler Architecture and Native C11 Backend

## Compilation data flow

```
.bv source + beaver.toml + beaver.lock
    |
UTF-8 source manager and indentation lexer
    |
AST parser (spans on every node)
    |
Module/import resolver + symbol tables + visibility check
    |
Type inference/checker + const checking + control-flow validation
    |
Ownership, move and lexical borrowing checker
    |
Typed MIR/CFG (explicit sequencing, borrows, moves, drops)
    |
C11 code generator (one translation unit for v0.1)
    |
Host C compiler + Beaver C runtime
    |
Native executable
```

Make each stage testable independently; no codegen occurs when an earlier stage fails. All errors use structured source spans, diagnostic IDs and notes.

## Lexer

Line-sensitive lexer emits `NEWLINE`, `INDENT`, `DEDENT`, identifiers, types, keywords, number/String tokens, punctuation, operators. Preserve token byte offsets and human-friendly 1-based line/column. Handle blank/comment lines without affecting indent stack. Track escape sequences and reject invalid UTF-8. Precedence parsing via Pratt parser or explicit recursive descent; no parser generator dependency required.

## AST and types

AST nodes include source span + stable node ID. Separate mutable parser AST from resolved typed HIR. Type system uses interned type IDs; avoid embedding symbol names as unvalidated Strings into the C backend. Intern module/function/record symbols, validate duplicates, name visibility and imports. Compile-time constant expressions are type checked. Disallow recursive sized records and illegal generic arities.

## Ownership/borrow checking

Perform after type checking and before lowering to C. Borrow checking is on owned *root place* IDs, not raw source names, to catch `person.name` and `person.age` alias conflicts. Use conservative lexical scope borrow lifetime; materialize `scope:` boundaries. Track `Copy` vs `Move` types and explicit `take`. Control-flow joins must prove consistent ownership for live names. Output typed MIR that explicitly records ownership transfers, temporary-borrow ends, and destructor actions.

## Suggested MIR

```
Program { packages, modules, records, functions, tests }
Function { signature, locals, blocks, entry_block }
BasicBlock { statements[], terminator }
Place { Local(id), Field(place, field_id), Index(place, index_local) }
Rvalue { Const, LoadCopy, Move, BorrowRead, BorrowWrite,
         BinaryChecked, UnaryChecked, ConstructRecord, ConstructList,
         ConstructString, ConstructBuffer, ConstructResult,
         Call, ListGetChecked, BufferGetChecked }
Stmt { Assign(place, rvalue), Drop(place), EndBorrow(id), Assert(...) }
Terminator { Goto, Branch, Return, Trap, Unreachable }
```

This is a model, not a mandatory Rust API. The key is that generated C cannot accidentally change operand evaluation order or omit destructor operations. Lower `and`/`or` to branches for short-circuiting, not eager expressions.

## C11 emission

- Generate build artifacts under `target/{debug|release}/generated/` for inspectability, and produce a native executable at `target/{debug|release}/{package}`.
- Emit one C11 translation unit containing generated type definitions, prototypes, implementations, and main wrapper, using compiler-generated mangled symbol names (`bvr_<package>_<module>_<symbol>`). Names must be collision resistant across modules/packages.
- Inject the bundled runtime headers and compile/link its C source. Compiler-generated C is an internal artifact, not a stable API.
- Evaluate function arguments and binary operands into explicit ordered temporaries before calling C functions, because C does not guarantee left-to-right argument expression evaluation. Preserve borrow/ownership transfers.
- Implement checked Int arithmetic in the runtime using a portable technique (e.g. builtins when available plus a tested C11 fallback, or algorithmically correct overflow checks). `-fwrapv` alone is **not** sufficient.
- Emit bounds checks before address arithmetic on Lists and Buffers.
- For non-Copy values, do not assign C structs by value unless performing a move and permanently invalidating the old location. Generate or reuse specialized move/drop/clone helpers.
- Emit `typedef struct ...` for records and monomorphized built-in `List[T]`/`Result[T]`, with ownership helpers for nested types.
- Generate source-mapped runtime checks (`file`, `line`) where practical.
- Use C11 `int64_t`, `uint8_t`, `double`, `stdbool.h`; guard allocations against overflow. No variable-length C arrays or compiler-specific GNU extensions required.

## C compiler invocation

Selection: `BEAVER_CC` environment variable (validated as an executable path/command), then `CC`, then `clang`, then `cc`. Show a friendly diagnostic if none available. Invoke via `std::process::Command` with separately supplied arguments and never use a shell command String. No arbitrary remote build flags, environment expansion, or user-defined scripts from packages.

Debug flags: `-std=c11 -O0 -g -Wall -Wextra`; release flags: `-std=c11 -O2 -Wall -Wextra`. Treat relevant generated-code warnings as release blockers and do not use optimizations that change Beaver language semantics. Sanitizer-enabled CI should compile fixtures with host-specific ASan/UBSan where available. Link the C runtime and platform libc as needed.

## Runtime API (suggestion)

```
bvr_string_from_literal(ptr, len)
bvr_string_concat(a, b)
bvr_string_clone(a), bvr_string_drop(&a)
bvr_list_<T>_new/append/get/drop/clone
bvr_buffer_new_zeroed(size), bvr_buffer_get/set/drop
bvr_result_<T>_ok/fail/drop
bvr_checked_add_i64/sub_i64/mul_i64/div_i64/rem_i64/neg_i64
bvr_bounds_check(index, len, file, line)
bvr_print_i64/f64/bool/byte/string
bvr_assert(...), bvr_panic(...)
```

Names may change internally, but tests must establish behavioral compatibility. Runtime allocation and cleanup must be instrumentable during tests (e.g., test-build allocation counters) so the suite can assert no leaked owned allocations or double frees.

## Toolchain structure

`beaver check`: stop after type/ownership validation. `beaver build`: lower and compile. `beaver run`: build then invoke binary, propagating its exit code. `beaver test`: compile a native test driver and run it. `beaver fmt`: AST/token-aware formatter; never reformat inside String literals or change semantics. `beaver package`: deterministic archive of allowed package files.

## Standard library v0.1

The prelude is compiler-bundled. Required builtins: `print`, `assert`, `assert_eq`, `len`, `append`, `clone`, `Float`, `Int`, `Byte`, `ok`, `fail`, `is_ok`, `unwrap`, `unwrap_error`, `range` and `Buffer` constructor. Builtins must be typed and lowered through the compiler, not injected by textual transformation. `String` and `List` runtime helpers are bundled. No file I/O, sockets, FFI, or OS APIs are required as Beaver source stdlib features for v0.1.

## Reproducibility notes

The generated C text should be stable for the same AST, module graph and compiler version (sorted modules, stable mangling, normalized paths). Native binary bytes may vary across C compilers and platform SDKs. Archive files and lockfiles should have stricter deterministic requirements.


---

# Source: `docs/05_CLI_PACKAGES_REGISTRY.md`

# 05 - Beaver CLI, Project Format, Dependencies, and Secure Registry Downloads

This is a complete **client-side package workflow**. Hosting a public registry or publishing to it is not a v0.1 deliverable. The CLI must nonetheless be able to fetch packages from a locally hosted contract-compatible static registry and from a configured trusted HTTPS registry.

## 1. CLI contract

| Command | Required behavior |
|---|---|
| `beaver --help`, `beaver --version` | Help text; report language/tool version. |
| `beaver new NAME [--lib]` | Create NAME folder with binary (default) or library skeleton. Refuse overwrite. |
| `beaver init [--lib]` | Initialize current empty/uninitialized directory. Refuse overwrite of existing manifest/source. |
| `beaver check [--offline] [--locked]` | Parse, resolve imports/deps, type-check and borrow-check only. |
| `beaver build [--release] [--offline] [--locked]` | Native codegen + host C compiler; place binary under target profile. |
| `beaver run [--release] [--offline] [--locked]` | Build and execute the binary; propagate nonzero exit code. Valid only for bin packages. |
| `beaver test [--offline] [--locked]` | Compile and run native `tests/*.bv` tests; report totals; nonzero if any fail. |
| `beaver fmt [--check]` | Canonical formatting or verify unchanged. |
| `beaver add NAME@X.Y.Z [--registry-url URL]` | Add exact registry dependency, resolve entire graph, fetch, lock, update manifest. |
| `beaver add --path PATH` | Add local library dependency based on its manifest name. |
| `beaver remove NAME` | Remove direct dependency; update graph/lockfile, preserving unrelated deps. |
| `beaver update NAME@X.Y.Z [--registry-url URL]` | Change direct dependency to a specific exact version; refresh lock. |
| `beaver fetch [--offline] [--locked] [--registry-url URL]` | Resolve/fetch all dependencies without compiling. |
| `beaver package [--output DIR]` | Validate library package and create canonical `.tar.gz` archive. |

`beaver build` on a library package validates and emits a compiled intermediate object/archive only if desired; at minimum it must type-check and produce no misleading executable. `beaver run` on a library is an error. `beaver test` without test blocks succeeds with `0 tests` after successful compilation.

Global help and all command errors must have user-actionable text. `--offline` forbids network access, even metadata refresh. `--locked` requires an existing, fully consistent lockfile and cannot change it. `--locked` is incompatible with `add`/`remove`/`update`.

## 2. Generated project layouts

Binary package:

```
hello/
  beaver.toml
  src/main.bv
  tests/smoke.bv
  .gitignore
```

Library package:

```
mathkit/
  beaver.toml
  src/lib.bv
  tests/smoke.bv
  .gitignore
```

Both ignore `target/` and should commit `beaver.lock` when dependencies exist. Files are UTF-8 text. No network access for `new`, `init`, or local compilation without dependencies.

## 3. Manifest `beaver.toml`

Binary:

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2026"
kind = "bin"
entry = "src/main.bv"

[dependencies]
```

Library with exact and path dependencies:

```toml
[package]
name = "consumer"
version = "0.1.0"
edition = "2026"
kind = "bin"
entry = "src/main.bv"

[dependencies]
mathkit = { version = "1.0.0" }
localutil = { path = "../localutil" }
```

Rules:

- `name`: ASCII `[a-z][a-z0-9_]{0,63}`; case-normalized and unique, no hyphens or slashes. Reject reserved OS/device names, path-like names, duplicate names (including case-folded aliases).
- `version`: exact three-number SemVer `MAJOR.MINOR.PATCH` with nonnegative integers (no prereleases/build suffix in 0.1).
- `edition`: exact string `"2026"`.
- `kind`: `"bin"` or `"lib"`. For bin, entry defaults to `src/main.bv` if omitted; for lib, to `src/lib.bv`.
- `entry`: relative path strictly inside project; cannot contain `..`, absolute prefixes, symlink escapes, or files outside `src/`. `.bv` required.
- Dependencies must be exact versions or local paths. A name may not appear in two forms.
- Library packages may depend on other libraries, never directly on executable packages.
- Registry packages cannot contain `path` dependencies because they cannot be reproduced in external projects. `beaver package` rejects such manifests.
- Unknown top-level keys or conflicting fields are errors (future edition may version them).

## 4. Lockfile `beaver.lock`

Use deterministic TOML (sorted package entries and dependencies):

```toml
lock_version = 1

[[package]]
name = "mathkit"
version = "1.0.0"
source = "registry+https://registry.example.invalid"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
dependencies = []
```

The URL above is a placeholder and **not a real Beaver registry**. `checksum` is SHA-256 hex of the compressed archive bytes. For local path dependencies, use `source = "path+../localutil"` with no checksum; these are intentionally not content-pinned, and must exist locally. The root project itself need not have a lockfile entry. Lock entries record every resolved transitive dependency and its exact direct dependencies (`name@version` strings). Include the registry URL per remote package; one registry per dependency graph in 0.1.

Lockfile behavior:

- `build/check/test/run` use lockfile versions when present; fail if it conflicts with the manifest unless explicitly running a command allowed to rewrite it without `--locked`.
- Prefer not to rewrite a valid lockfile unnecessarily. Use atomic write+rename and preserve existing file on errors.
- `--locked` rejects missing, stale, inconsistent or altered lockfiles.
- A warm offline cache can reproduce lockfile dependencies without visiting the network.
- Recalculate and verify archive SHA-256 on first use after download, and check cached archive integrity before extraction/use.
- Local path dependencies can change without lock hash; this must be explained in the CLI docs.

## 5. Registry configuration

No public default registry exists in 0.1. **Never hardcode an invented public Beaver registry.**

Order of precedence:
1. `--registry-url URL` on remote package commands.
2. `BEAVER_REGISTRY_URL` environment variable.
3. User config (platform config directory, e.g. `~/.config/beaver/config.toml`):

```toml
[registry]
url = "https://registry.example.invalid"
```

4. For an already locked package, use its locked registry URL (subject to security checks).

If a remote package needs to be resolved and no registry is configured/locked, emit a clear configuration error. Registry URLs must be HTTPS with valid TLS certificates, except `http://127.0.0.1:<port>` or `http://[::1]:<port>` when `BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1` is explicitly set for local integration testing. Never allow insecure HTTP to a non-loopback host. Reject redirects for 0.1 to avoid changing trust origins and redirect-based SSRF.

## 6. Registry HTTP protocol version 1

For `name=mathkit`, `version=1.0.0`:

```
GET {base_url}/v1/packages/mathkit/1.0.0/metadata.json
Accept: application/json
```

Successful response (max 128 KiB):

```json
{
  "schema": 1,
  "name": "mathkit",
  "version": "1.0.0",
  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "archive_size": 2048,
  "dependencies": {}
}
```

Then:

```
GET {base_url}/v1/packages/mathkit/1.0.0/archive.tar.gz
Accept: application/gzip
```

Archive response must contain exactly the declared number of bytes (or fail) and match SHA-256. `archive_size` is compressed bytes length. Client streams the download to a temporary file and verifies size and hash **before** entering the content-addressed cache and before extraction. No executable code runs during download or extraction.

The registry metadata dependency graph must match the archive manifest's exact dependencies. Disagreement is an error. Fetch metadata and packages recursively with limited depth; no dynamic SemVer resolution. On missing package/version, use helpful messages rather than falling back to arbitrary versions.

## 7. Dependency resolution

1. Load root manifest. Resolve root path dependencies (validate their manifests) and exact registry-version dependencies.
2. Query/download metadata for each remote dependency or use a lockfile+warm verified cache.
3. Expand transitive exact dependencies (depth-first or breadth-first). Only direct dependencies may be used as `use` imports in a source package.
4. If the same package name resolves to conflicting versions or registries, fail with dependency-chain diagnostics. No parallel versions of one name in v0.1.
5. Reject cycles and excess graph depth (>32) or package count (>128) rather than infinite recursion.
6. For each registry archive, verify metadata name/version and the archive manifest's dependency set before accepting.
7. Serialize a stable sorted lockfile; atomic update of manifest+lockfile for `add/remove/update`, with failure rollback.
8. Compile all source modules in dependency-order in one linking operation. No dependency build scripts.

## 8. Archive packaging and extraction

Format: deterministic `gzip(tar)` with a single root directory `${name}-${version}/`. Allowed contents: `beaver.toml`, `src/**/*.bv`, optional `README.md`, `LICENSE`, and optional test sources (tests are not executed automatically when installed). Exclude `target`, VCS metadata, secrets, generated binaries, `beaver.lock` and unrelated files.

Canonical archive requirements: sort paths lexicographically, normalize uid/gid/mtime/modes, fixed gzip timestamp, stable output for identical inputs. Refuse symlinks/hardlinks/device files and paths resolving outside the package root. `beaver package` prints the archive path and SHA-256.

Extraction requirements: reject absolute paths, `..` path components, Windows drive/UNC paths, symlinks, hardlinks, special files, duplicate paths, case-insensitive collisions, filenames with prohibited components or NUL, and archives with a manifest that disagrees with metadata. Make output directory private and extract atomically after checks. Enforce compressed archive <=20 MiB, total expanded size <=100 MiB, single file <=10 MiB, <=8192 entries, metadata <=128 KiB. Any over-limit condition is a hard error.

## 9. Cache layout

Platform-aware cache root (e.g. `$XDG_CACHE_HOME/beaver`):

```
cache/
  metadata/<registry-hash>/<name>/<version>.json
  archives/<sha256>.tar.gz
  sources/<sha256>/<name>-<version>/...
```

Cache archives by content hash and verify integrity; writes atomic. Remove partial files on errors. Add timeouts (connect <=10s, request <=60s) and an upper bound on streamed response bytes. No credentials or token storage in v0.1. `BEAVER_CACHE_DIR` may override cache location in integration tests.

## 10. Package operations examples

```sh
beaver new app
cd app
beaver add mathkit@1.0.0 --registry-url https://packages.example.invalid
beaver fetch --locked
beaver run --locked
beaver remove mathkit

# local library workflow
beaver new mathkit --lib
beaver new consumer
cd consumer
beaver add --path ../mathkit
beaver check

# archive
cd ../mathkit
beaver package
```

Replace placeholder URLs with a real configured registry. During development use `tools/create_mock_registry.py` to create test artifacts and a loopback HTTP server.

## 11. Security and trust limitations

SHA-256 lockfile checking protects integrity/reproducibility after the first trusted resolution. It does **not** establish publisher identity, guarantee the registry is honest, or prove source code is non-malicious. Beaver 0.1 has no signature verification, account identity, package upload API, or sandbox for a user choosing to build and run a dependency. Disclose this clearly instead of claiming the registry is production-secure by default.


---

# Source: `docs/06_DIAGNOSTICS_TESTS.md`

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


---

# Source: `docs/07_IMPLEMENTATION_PLAN.md`

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


---

# Source: `docs/08_REPOSITORY_AND_ENGINEERING.md`

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


---

# Source: `docs/09_OPEN_QUESTIONS_AND_DEFERRED.md`

# 09 - Deliberate Constraints, Deferred Features and Decision Log

## Frozen v0.1 decisions (do not debate mid-implementation)

- Compiler written in Rust, C11 generated-code backend.
- Simple lexical borrow lifetimes; `scope:` explicitly ends named borrows.
- Copy values passed by value; unadorned non-Copy parameters borrowed read-only; `edit` and `take` explicit.
- String UTF-8 immutable, `len(String)` returns byte length.
- Exactly checked 64-bit signed integer operations, default runtime trap for undefined numeric actions.
- Only data-only records and built-in generic containers List[T]/Result[T], not user-defined generics.
- Result error is String; no exceptions.
- Modules imported with `use`; exports require `pub`.
- Exact version registry dependencies, SHA-256 lockfiles, HTTPS, no public registry service.
- Native host C compiler required for final machine code; `beaver check` does not need one.

## Known conservative restrictions

1. Lexically persistent references cause some otherwise-safe code to require a `scope:` block.
2. Overlapping borrows of disjoint record fields are conservatively rejected.
3. Borrowed return types and persistent references in containers are unsupported.
4. Conditional ownership merges that would require runtime state are conservatively rejected.
5. Partial moves out of record fields or List indices are forbidden.
6. No shared/cyclic heap ownership; express graphs by indices or wait for future `share`/weak references.
7. No FFI or unrestricted raw memory addresses. Beaver 0.1 is not yet suitable for kernel/device-driver programming.
8. No trusted/public registry infrastructure or publisher identity. SHA-256 detects modified artifacts but is not a source attestation.
9. No broad compiler optimization claims. Native code correctness precedes automatic storage-placement sophistication.

## Possible version 0.2 or later

- Non-lexical lifetime analysis and narrower field-borrow granularity.
- Shared ownership, weak links, safe graph APIs, channels and safe concurrency.
- FFI, explicit `unsafe`, bounds-aware low-level pointers, custom allocators and regions.
- Full generics, traits/interfaces, methods, enums, tagged pattern matching, Option[T], richer Result[T,E].
- Borrowed return types and lifetime inference.
- String interpolation, Unicode grapheme iterators, richer collection APIs.
- File and network I/O, async runtime, threads, atomics.
- SemVer ranges, registry publishing, artifact signatures, namespace ownership and supply-chain attestations.
- LLVM/Cranelift native backend, LSP, debugger integration, package workspaces, cross compilation.
- Escape analysis, stack promotion and runtime allocation effects such as `noalloc`.

## If ambiguities arise while coding

Astra should write an ADR in `docs/adr/NNNN-title.md` with `Problem / Options / Choice / Tests / Compatibility`. For any safety ambiguity, choose rejection over unsound acceptance. For any syntax ambiguity, keep the parser and formatter consistent with current fixture programs. For major product-language changes, defer to a later edition rather than silently changing the 0.1 contract.


---

# Source: `docs/10_REFERENCE_MATERIAL.md`

# 10 - Non-Normative Reference Material

These external materials are implementation references, not Beaver specifications.

- Cargo: [Creating a new package](https://doc.rust-lang.org/stable/cargo/guide/creating-a-new-project.html) - generated project structure and executable workflow.
- Cargo: [Dependencies](https://doc.rust-lang.org/cargo/guide/dependencies.html) - manifest declarations and lockfile reasoning.
- Cargo: [Registry index specification](https://doc.rust-lang.org/cargo/reference/registry-index.html) - patterns for checksums, metadata and cache concerns. Beaver's registry protocol is intentionally simpler and NOT wire-compatible.
- [clap derive documentation](https://docs.rs/clap/latest/clap/_derive/) - Rust CLI subcommand parsing.
- [reqwest documentation](https://docs.rs/crate/reqwest/latest) - HTTPS client, streaming and TLS configuration.

Astra should use maintained stable-compatible library versions available at implementation time and commit the generated Rust `Cargo.lock`. This reference list does not require a particular HTTP client or Rust crate version.


---

# Source: `spec/beaver.ebnf`

```ebnf
# Beaver 0.1 surface grammar (informative EBNF)
# NEWLINE / INDENT / DEDENT emitted by indentation-aware lexer.
# Parsing rules must also enforce the semantic restrictions in docs/02_LANGUAGE_SPEC.md.

source         = { NEWLINE | use_decl | const_decl | type_decl | fn_decl | test_decl } EOF ;
visibility     = [ "pub" ] ;
use_decl       = "use" path NEWLINE ;
path           = IDENT { "." IDENT } ;
const_decl     = visibility "const" IDENT ":" type_expr "=" expr NEWLINE ;
type_decl      = visibility "type" IDENT ":" NEWLINE INDENT field_decl { field_decl } DEDENT ;
field_decl     = IDENT ":" type_expr NEWLINE ;
fn_decl        = visibility "fn" IDENT "(" [ param { "," param } ] ")"
                 [ "->" type_expr ] ":" suite ;
param          = IDENT ":" [ "view" | "edit" | "take" ] type_expr ;
test_decl      = "test" STRING ":" suite ;
type_expr      = type_path [ "[" type_expr "]" ] ;
type_path      = path ;
suite          = NEWLINE INDENT { stmt | NEWLINE } DEDENT ;
stmt           = binding | assign | return_stmt | if_stmt | while_stmt
               | for_stmt | loop_ctrl | scope_stmt | "pass" NEWLINE
               | expr NEWLINE ;
binding        = ("let" | "var") IDENT [ ":" type_expr ] "=" expr NEWLINE ;
assign         = place ("=" | "+=" | "-=" | "*=" | "/=") expr NEWLINE ;
return_stmt    = "return" [ expr ] NEWLINE ;
if_stmt        = "if" expr ":" suite { "elif" expr ":" suite }
                 [ "else" ":" suite ] ;
while_stmt     = "while" expr ":" suite ;
for_stmt       = "for" IDENT "in" expr ":" suite ;
loop_ctrl      = ("break" | "continue") NEWLINE ;
scope_stmt     = "scope" ":" suite ;
place          = IDENT { "." IDENT | "[" expr "]" } ;
expr           = or_expr ;
or_expr        = and_expr { "or" and_expr } ;
and_expr       = compare_expr { "and" compare_expr } ;
compare_expr   = additive [ compare_op additive ] ;
compare_op     = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
additive       = multiplicative { ("+" | "-") multiplicative } ;
multiplicative = unary { ("*" | "/" | "%") unary } ;
unary          = ("not" | "-" | "view" | "edit" | "take" | "try") unary
               | postfix ;
postfix        = primary { "." IDENT | "[" expr "]"
                 | "(" [ arg { "," arg } ] ")" } ;
arg            = [ IDENT "=" ] expr ;
primary        = INT | FLOAT | STRING | "true" | "false" | IDENT
               | "(" expr ")" | "[" [ expr { "," expr } ] "]" ;

# Type_expr is restricted to List[T], Result[T], named types, and built-ins;
# IDENT as callee may be a built-in such as ok, fail, Buffer, or Byte.
# "range" is a builtin function, not a general expression iterator type.
# No chained comparisons; lexer handles INT/FLOAT and string escapes.
# Test declarations are valid only under tests/*.bv.

```


---

# Example: `examples/consumer/src/main.bv`

```python
use mathkit

fn main():
    var point = mathkit.Point(x=2, y=7)
    mathkit.translate_x(edit point, 5)
    print(mathkit.square(point.x))

```


---

# Example: `examples/hello/src/main.bv`

```python
fn main():
    print("Hello, Beaver!")

```


---

# Example: `examples/mathkit/src/lib.bv`

```python
pub type Point:
    x: Int
    y: Int

pub fn square(value: Int) -> Int:
    return value * value

pub fn translate_x(point: edit Point, delta: Int):
    point.x += delta

fn hidden_double(value: Int) -> Int:
    return value + value

```
