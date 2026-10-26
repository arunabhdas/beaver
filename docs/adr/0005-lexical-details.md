# ADR 0005: Lexical details

## Problem
Several surface details are not fixed by the spec.

## Choice
* **Indentation:** each line's indentation must be a multiple of 4 spaces (B1003). A block may
  indent by any multiple of 4; `beaver fmt` normalizes to 4 per level. Tabs in leading whitespace
  are rejected (B1002). Comment-only and blank lines never affect indentation.
* **No line continuation:** every logical line is one physical line; newlines inside brackets are
  not joined. This keeps the formatter purely line-based and lossless.
* **Numbers:** decimal integers (`0`..`9223372036854775807`) and decimal floats `D.D`. No
  exponents, hex, underscores, or forms like `1.`/`.5`. `-x` is unary minus on a literal, so the
  minimum Int is written `-9223372036854775807 - 1`.
* **Precedence:** exactly as the spec lists it, so `not a == b` parses as `(not a) == b`.
* **Lone `\r`** (not part of CRLF) is rejected (B1016).

## Tests
`crates/beaver-syntax/src/lexer.rs` and `parser.rs` unit tests, `tests/robustness.rs`.
