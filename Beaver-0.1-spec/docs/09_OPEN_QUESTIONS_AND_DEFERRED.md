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
