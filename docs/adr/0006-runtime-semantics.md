# ADR 0006: Runtime traps, conversions and printing

## Choice
* Every runtime check failure prints `runtime error[B5xxx]: message` plus `--> file:line:col` to
  stderr (stdout is flushed first) and exits with status **101**. `beaver run` propagates it.
* `INT64_MIN % -1` evaluates to `0` (its mathematical value) instead of trapping; only division
  `INT64_MIN / -1` overflows, as the spec requires. A remainder by zero traps (B5002).
* Checked arithmetic uses `__builtin_*_overflow` when the C compiler has them and a portable C11
  fallback otherwise (`-DBVR_NO_BUILTINS` forces it). Both are tested against Rust's `checked_*`
  operations on boundary values.
* Conversions: `Float(Int)`, `Int(Float)` (truncates; traps on NaN/±inf/out of range), `Byte(Int)`
  (0..255), and additionally `Int(Byte)`. Without it, Bytes could not take part in arithmetic at
  all. It cannot fail.
* `print(Float)` prints the shortest decimal that round-trips, always with `.` or an exponent:
  `3.0`, `0.30000000000000004`, `inf`, `nan`.
* String literals are static, never-freed data. `clone`, concatenation and other derived strings
  are heap-owned. Dropping a static string is a no-op.
* `BEAVER_RT_LEAK_CHECK=1` makes the runtime count live owned allocations and fail with B5099
  (exit 102) if any remain at exit. The test suite enables it for every program.

## Tests
`tests/conformance/runtime/*`, `crates/beaver-codegen-c/tests/runtime_arith.rs`.
