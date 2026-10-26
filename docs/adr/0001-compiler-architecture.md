# ADR 0001: Crate layout and a single checked compilation path

## Problem
The handoff requires distinct lexer, parser/AST, diagnostics, module resolution, type checking,
ownership checking, MIR, C backend, runtime, package manager and CLI components, and that code
generation can never run on a program that failed an earlier stage.

## Options
1. One crate with modules.
2. A crate per stage, with checked artifacts passed between them.

## Choice
Option 2, seven crates: `beaver-source` (spans, diagnostics), `beaver-syntax` (lexer, parser,
AST, formatter), `beaver-sema` (module loading/import resolution in `load.rs`, type checking in
`typeck.rs`, ownership/borrow checking in `ownership.rs`), `beaver-mir`, `beaver-codegen-c` (C11
emitter + host compiler driver, bundles `runtime/`), `beaver-package`, `beaver-cli`.

`beaver_sema::analyze` is the only producer of `hir::Program`, and it only returns one when loading,
type checking *and* ownership checking produced no errors. `beaver_mir::lower` only accepts a
`hir::Program`, and the C backend only accepts MIR. No stage translates source text to C by string
substitution: the backend sees typed MIR with explicit moves, borrows and drops.

## Tests
`crates/beaver-cli/tests/conformance.rs` asserts that every compile-fail fixture also fails
`beaver build` and produces no executable. `crates/beaver-codegen-c/tests/pipeline_fuzz.rs` runs
mutated programs through every stage.

## Compatibility
A future LLVM/Cranelift backend can consume `beaver_mir::MirProgram` unchanged.
