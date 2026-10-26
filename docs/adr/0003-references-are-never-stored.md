# ADR 0003: Reference bindings, Copy referents and escapes

## Problem
The fixture `reference_inside_list.bv` (`let alias = view n` with `n: Int`, then `[alias]`) must be
rejected with B3, although copying an Int out of a reference would be harmless. The spec also says
borrowed parameters "cannot be returned, stored in records/lists/Results, or moved". It does not say
what `var r = edit x` means: rebinding or writing.

## Choice
* A **bare reference name** (a `view`/`edit` local, a non-Copy default parameter, a `view`/`edit`
  parameter, a non-Copy `for` element) in a *persisting* position (list literal element, `append`
  element, record field initializer, `ok(...)`/`fail(...)` argument, `return` value) is rejected
  with B3008, **even when the referent is Copy**. Write `clone(r)` (a plain copy for Copy types) or
  any expression (`r + 0`) to store a value. Reads through a reference elsewhere (`let y = r`,
  arithmetic, `print(r)`, passing to a by-value parameter) copy Copy referents as usual.
* Non-Copy values reachable only through a reference can never be moved (B3005), and when returned
  are reported as escapes (B3008).
* References must be bound with `let`; `var r = view/edit x` is rejected (B3013). Assignments through
  `let w = edit x` write the referent (`w = 5`).
* `view`/`edit` expressions are only allowed as `let` initializers and call arguments (B3016).

## Tests
`compile_fail/return_view_param.bv`, `reference_in_record.bv`, `var_reference.bv`,
`borrow_in_list_literal.bv`, `sema.rs::references_never_escape`.

## Compatibility
Restrictive by design. It may be relaxed for Copy referents in a later edition.
