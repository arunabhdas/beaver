# ADR 0008: Argument forms

## Choice
| Parameter | Accepted arguments |
|---|---|
| `x: T` with Copy `T` | any value expression (copied); `take place` (moves) |
| `x: T` with non-Copy `T` | a place (implicit read borrow), `view place`, or a temporary (borrowed for the statement) |
| `x: view T` | same as above |
| `x: edit T` | `edit place` only |
| `x: take T` | a temporary, `take place`, or a Copy place |

Passing `edit` to a read-only parameter, `view`/`edit` to a by-value or `take` parameter, or `take`
to a borrowing parameter is rejected (B3017), so the call site shows exactly what happens. `take`
parameters and by-value Copy parameters are immutable bindings in the callee; copy into a `var`
to mutate.

`append(edit list, element)` requires `edit`. Read-only builtins (`print`, `len`, `clone`, `is_ok`,
`assert_eq`) accept a place, `view place`, or a temporary. `unwrap`/`unwrap_error` consume their
argument (`take r` or a temporary).

## Tests
`crates/beaver-sema/tests/sema.rs::argument_modes_must_match_parameters`,
`compile_fail/missing_edit_keyword.bv`, `edit_and_read_same_call.bv`.
