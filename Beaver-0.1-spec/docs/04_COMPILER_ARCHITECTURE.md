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
