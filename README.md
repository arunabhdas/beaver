# Beaver 0.1

Beaver is a statically typed, indentation-based, ahead-of-time compiled language with explicit
ownership (`view`, `edit`, `take`), lexically checked borrows and deterministic cleanup, with no
garbage collector and no pointer syntax. This repository contains the Beaver 0.1 compiler and
toolchain, written in Rust. Beaver source is parsed, type-checked and ownership-checked, then
lowered to a typed MIR with explicit moves, borrows and drops. The MIR is emitted as C11 and
compiled with the host C compiler into a native executable.

The binding specification is in [`Beaver-0.1-spec/`](Beaver-0.1-spec/README.md). Decisions that
the spec left open are recorded in [`docs/adr/`](docs/adr). Error codes are listed in
[`docs/DIAGNOSTICS.md`](docs/DIAGNOSTICS.md), and progress with real test results is in
[`IMPLEMENTATION_STATUS.md`](IMPLEMENTATION_STATUS.md).

```beaver
type Point:
    x: Int
    y: Int

fn translate(p: edit Point, dx: Int):
    p.x += dx

fn main():
    var p = Point(x=1, y=2)
    translate(edit p, 4)
    print(p.x)                 # 5
```

## Building

Prerequisites:

* a stable Rust toolchain (tested with Rust 1.98.1);
* a C11 compiler: `clang`, `cc` or `gcc`, or any compiler named by `BEAVER_CC`/`CC`. `beaver check`
  works without one.

Supported hosts: macOS and Linux. The test suite has been run on macOS 26 (arm64) with Apple
clang 21. CI is configured for Linux x86_64/aarch64 (gcc and clang) and macOS arm64/x86_64.
Windows is not supported in 0.1.

```sh
cargo build --release          # produces target/release/beaver
cargo test --workspace         # unit, conformance, sanitizer, registry and CLI tests
```

## Quick start

```sh
beaver new hello && cd hello
beaver run                     # Hello, Beaver!
beaver test                    # runs tests/*.bv
beaver build --release         # target/release/hello (native executable)
beaver fmt                     # canonical formatting
```

A binary package looks like this:

```
hello/
  beaver.toml
  src/main.bv
  tests/smoke.bv
  .gitignore
```

## The language

### Types and values

| Type | Meaning | Copy |
|---|---|---|
| `Int` | signed 64-bit; overflow traps | yes |
| `Float` | IEEE-754 binary64 | yes |
| `Bool` | `true` / `false` | yes |
| `Byte` | unsigned 8-bit, made with `Byte(n)` | yes |
| `String` | immutable UTF-8; `len` counts bytes | no |
| `List[T]` | growable, owns its elements | no |
| `Buffer` | fixed-size, zero-initialized bytes | no |
| `Result[T]` | success `T` or `String` error | no |
| `type R:` | record; Copy iff all fields are Copy | depends |
| `Void` | only as a return type or in `Result[Void]` | — |

There are no implicit numeric conversions. Use `Float(i)`, `Int(f)` (checked), `Byte(i)`
(checked) and `Int(b)`.

### Bindings, functions and control flow

```beaver
const LIMIT: Int = 10          # top level, explicit type, constant expression

fn classify(n: Int) -> String:
    if n < 0:
        return "negative"
    elif n == 0:
        return "zero"
    else:
        return "positive"

fn main():
    let name = "Beaver"        # immutable, type inferred
    var total = 0              # assignable
    for i in range(0, LIMIT):  # 0..LIMIT-1, bounds evaluated once
        if i == 3:
            continue
        total += i
    while total > 40:
        total -= 1
    print(classify(total))
```

`print` writes one Int, Float, Bool, Byte or String plus a newline. Integer `/` and `%`
truncate toward zero. Overflow, division by zero, out-of-bounds indexing and invalid
conversions stop the program with a `runtime error[B5xxx]` and exit status 101.
Comparisons cannot be chained, `and`/`or` short-circuit, and `not`, `-` and `view`/`edit`/
`take`/`try` bind tighter than the arithmetic operators.

### Ownership: `view`, `edit`, `take`

* Every non-Copy value has exactly one owner and is freed when that owner's scope ends, on
  every exit path: fallthrough, `return`, `break`, `continue`, and `try` failures.
* `take x` transfers ownership. Afterwards `x` cannot be used (there is no re-initialization in
  0.1). Named non-Copy values are never moved implicitly: write `take x`, or `clone(x)` to
  duplicate.
* `view x` is a shared read-only borrow and `edit x` an exclusive mutable borrow. Borrows bound
  with `let` last until the end of their lexical scope. Use `scope:` to end them early. Borrows
  created for call arguments end with the statement.
* Borrowing a field or element borrows the whole owner. References cannot be returned, stored
  in lists, records or Results, or moved.

```beaver
fn consume(text: take String):
    print(text)

fn append_twice(xs: edit List[Int], v: Int):
    append(edit xs, v)
    append(edit xs, v)

fn main():
    var xs = [1, 2]
    scope:
        let first = view xs    # shared borrow until the end of this scope
        print(len(first))
    append_twice(edit xs, 7)   # allowed: the view ended
    let word = "hi"
    consume(take word)         # `word` is moved; using it now is error B3001
```

Parameters:

| Parameter | Meaning | Call with |
|---|---|---|
| `x: Int` (Copy type) | copy | any value |
| `x: String` (non-Copy) | read-only borrow | `s`, `view s` or a temporary |
| `x: view T` | read-only borrow | same |
| `x: edit T` | exclusive mutable borrow | `edit s` |
| `x: take T` | ownership moves into the callee | `take s` or a temporary |

To return a named non-Copy local, write `return take local`. Freshly built values can be
returned directly.

### Records, lists and buffers

```beaver
type Person:
    name: String
    tags: List[String]

fn main():
    var people: List[Person] = []
    append(edit people, Person(name="Ada", tags=[]))
    append(edit people[0].tags, "math")
    people[0].name = "Ada L."            # the old String is freed
    for p in people:                     # `p` is a read-only element reference
        print(p.name)
    var buf = Buffer(size=4)
    buf[0] = Byte(65)
    print(buf[0])                        # 65
    let copy = clone(people)             # deep copy
    print(len(copy))
```

Constructors take every field exactly once, by name. Records compare with `==` when all of
their fields support equality. Lists, Buffers and Results do not.

### Errors: `Result[T]` and `try`

```beaver
fn divide(a: Int, b: Int) -> Result[Int]:
    if b == 0:
        return fail("division by zero")
    return ok(a / b)

fn compute() -> Result[Int]:
    let n = try divide(10, 2)    # on failure, returns the error to the caller
    return ok(n + 5)

fn main():
    let r = compute()
    if is_ok(r):
        print(unwrap(take r))
    else:
        print(unwrap_error(take r))
```

### Builtins

`print`, `assert`, `assert_eq`, `len`, `append`, `clone`, `ok`, `fail`, `is_ok`, `unwrap`,
`unwrap_error`, `range` (only in `for`), `Buffer(size=n)`, `Int(...)`, `Float(...)`,
`Byte(...)`.

### Modules and tests

* `use util` imports `src/util.bv` as `util`, and `use geo.shapes` imports `src/geo/shapes.bv`
  as `shapes`.
* `use mathkit` imports a direct dependency's entry module, and `use mathkit.tools` imports
  that dependency's `src/tools.bv`.
* Only `pub` declarations are visible to other modules. Public signatures cannot expose
  private types. Import cycles are errors.
* `test "label":` blocks are allowed in `tests/*.bv` only. `beaver test` compiles them into one
  native driver and runs each test in its own process, so one crashing test does not hide the
  others.

## Commands

| Command | |
|---|---|
| `beaver new NAME [--lib]` / `beaver init [--lib]` | create a package (never overwrites) |
| `beaver check [--offline] [--locked]` | parse, type-check and ownership-check the entry module graph only (test files are checked by `beaver test`) |
| `beaver build [--release] [--offline] [--locked]` | native executable at `target/{debug,release}/NAME`; libraries are compiled to an object file for validation |
| `beaver run [--release] [--offline] [--locked]` | build and run, propagating the exit status |
| `beaver test [--release] [--offline] [--locked]` | run `tests/*.bv`; nonzero if any test fails |
| `beaver fmt [--check]` | format `src/` and `tests/` (idempotent, keeps comments) |
| `beaver add NAME@X.Y.Z [--registry-url URL]` | add an exact registry dependency, resolve, fetch and lock |
| `beaver add --path PATH` | add a local library |
| `beaver remove NAME` / `beaver update NAME@X.Y.Z` | change dependencies and the lockfile |
| `beaver fetch [--offline] [--locked] [--registry-url URL]` | resolve and download without compiling |
| `beaver package [--output DIR]` | deterministic `NAME-VERSION.tar.gz` of a library, printed with its SHA-256 |

`--offline` never touches the network. `--locked` requires an up-to-date `beaver.lock` and
never modifies it.

Generated C is kept in `target/{debug,release}/generated/` for inspection. Debug builds use
`-std=c11 -O0 -g -Wall -Wextra` and release builds use `-std=c11 -O2 -Wall -Wextra`.

## Packages and the registry

```toml
[package]
name = "consumer"
version = "0.1.0"
edition = "2026"
kind = "bin"            # or "lib"
entry = "src/main.bv"   # optional

[dependencies]
mathkit = { version = "1.0.0" }        # exact version from the registry
localutil = { path = "../localutil" }  # local library
```

There is **no default public registry**. A registry is chosen from `--registry-url`, then
`BEAVER_REGISTRY_URL`, then `~/.config/beaver/config.toml`:

```toml
[registry]
url = "https://registry.example.invalid"
```

The client implements the static registry protocol v1 (`/v1/packages/NAME/VERSION/metadata.json`
and `archive.tar.gz`). It only uses HTTPS, refuses redirects, enforces size limits, verifies size
and SHA-256 before anything enters the cache or is extracted, validates every archive entry, and
never runs code from a package. `beaver.lock` pins each registry package's SHA-256. Path
dependencies are not content-pinned.

To try the mock registry from the handoff locally, start the server in one terminal:

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory Beaver-0.1-spec/fixtures/registry
```

Then, in another terminal:

```sh
export BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1   # plain HTTP is allowed only for 127.0.0.1/[::1] with this opt-in
export BEAVER_REGISTRY_URL=http://127.0.0.1:8765
beaver new app && cd app
beaver add mathkit@1.0.0
beaver fetch --offline --locked                 # served from the verified cache
```

The automated tests run their own in-process loopback server, so `cargo test` needs no external
service.

Security limits: SHA-256 detects artifacts that changed after they were locked. It does not
authenticate publishers or make dependency code safe to run. Beaver 0.1 has no signatures and no
sandbox.

## Environment variables

| Variable | Effect |
|---|---|
| `BEAVER_CC`, `CC` | C compiler to use (a path or a name on `PATH`, no arguments) |
| `BEAVER_REGISTRY_URL` | registry base URL |
| `BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1` | allow `http://127.0.0.1` / `http://[::1]` registries |
| `BEAVER_CACHE_DIR` | package cache location (default `~/Library/Caches/beaver` or `$XDG_CACHE_HOME/beaver`) |
| `BEAVER_RT_LEAK_CHECK=1` | programs verify at exit that every owned allocation was released |
| `BEAVER_SANITIZE=1` | build with `-fsanitize=address,undefined` |
| `BEAVER_WERROR=1` | treat C compiler warnings on generated code as errors |

## How it works

* **Borrow checking** (`crates/beaver-sema/src/ownership.rs`) walks each function in evaluation
  order. It tracks the set of moved locals and a list of active borrows, each tied to the *root*
  local of the borrowed place, so a field borrow locks the whole owner. Named borrows end with
  their lexical scope and argument borrows at the end of the statement. Each new borrow, read,
  write and `take` is checked against the active borrows. At `if`/loop joins, differing move states
  are accepted only for values never mentioned again. Loop back edges must not have moved outer
  values. References may not appear in returning or storing positions.
* **MIR and drops** (`crates/beaver-mir`): lowering produces basic blocks in which every operand is
  a constant or a local, so the evaluation order is explicit. Ownership transfers are
  `Operand::Move`, borrows are `Stmt::BindRef`/`EndBorrow`, and destruction is `Stmt::Drop`. The
  builder tracks moved locals per path and emits drops for owned locals on every scope exit (normal
  end, `return`, `break`, `continue`, the error path of `try`), for unconsumed statement temporaries,
  and on the still-owning side of any join. Each value is therefore dropped exactly once with no
  runtime drop flags. The C backend generates per-type drop and clone functions: recursive for
  records and lists, and touching only the active payload of a Result.
* **Checked arithmetic and bounds** (`runtime/src/beaver_rt.c`): every Int `+ - * / %` and negation
  goes through runtime helpers that use `__builtin_*_overflow` when available, with a portable
  C11 fallback. Both are tested on boundary values against Rust's `checked_*`. Division by zero,
  `INT64_MIN / -1`, invalid conversions, negative or out-of-range indices and allocation size
  overflow all trap before any undefined operation can happen.
* **Package integrity** (`crates/beaver-package`): downloads stream into a temporary file while
  being hashed. Size and SHA-256 are compared with the metadata (and the lockfile) before the
  archive is stored by hash. Every tar entry's type, path, size and name is validated before it
  is written into a private staging directory, which is renamed into place only when the whole
  archive is valid. The archive's manifest must match the metadata's name, version and
  dependencies. Resolution is exact-version with cycle, conflict, depth and size limits.
  `add`/`remove`/`update` change files only after the whole graph resolves.

## Repository layout

```
crates/beaver-source      spans, source map, diagnostics
crates/beaver-syntax      lexer, parser, AST, formatter
crates/beaver-sema        module loader, type checker, ownership checker, typed HIR
crates/beaver-mir         MIR and lowering with drop elaboration
crates/beaver-codegen-c   C11 emitter, C compiler driver (embeds runtime/)
crates/beaver-package     manifest, lockfile, registry client, cache, archives, resolver
crates/beaver-cli         the `beaver` binary and end-to-end tests
runtime/                  C runtime (checked arithmetic, strings, buffers, traps, test driver)
tests/conformance/        this repository's compile-pass / compile-fail / runtime programs
Beaver-0.1-spec/          the handoff: specification, examples, fixtures, mock registry
docs/                     ADRs and diagnostic catalogue
```

## Limitations of 0.1

These are deliberate (see `Beaver-0.1-spec/docs/09_OPEN_QUESTIONS_AND_DEFERRED.md`):

* Borrows are lexical, not non-lexical, and field borrows lock the whole owner.
* There are no partial moves, no re-initialization after `take`, and conditional moves are
  accepted only when the value is never used again.
* There are no user generics, methods, enums, pattern matching, closures, shared ownership, FFI,
  `unsafe`, concurrency, file or network I/O, String indexing, interpolation, line continuation,
  float exponents or bitwise operators.
* Dependencies use exact versions only. There is no publishing, no signatures and no public
  registry.
* There is no LLVM backend, debugger or LSP. Windows is not supported.
* Recursion depth is limited by the native stack, with no guaranteed diagnostic on overflow.
* Program arguments are passed through `beaver run -- ...`, but Beaver 0.1 has no API to read
  them.
