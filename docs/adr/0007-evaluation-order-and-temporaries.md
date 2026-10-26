# ADR 0007: Evaluation order, temporaries and statement-scoped borrows

## Choice
* Expressions evaluate left to right. The MIR evaluates every operand into an explicit temporary, so
  C's unspecified argument order never matters. Copy values are snapshotted at the point they are
  read: in `x + inc(edit x)` the left operand is the old `x`.
* In `target = value`, the right-hand side is evaluated first, then the index expressions of the
  target, then the old value is dropped (with a bounds check) and the new one stored. In
  `target op= value`, the target is read first.
* Temporaries (call results, literals passed by reference, concatenations, ...) are dropped at the
  end of their statement unless consumed. Inside the right operand of `and`/`or`, temporaries are
  dropped before the operand completes.
* Borrows created for call arguments and builtin reads last until the end of the statement. They
  are released *before* the store of an assignment, which is sound because no borrowed value can
  escape a call in 0.1. Conflicts within one statement are therefore rejected, e.g.
  `two(edit n, edit n)` or `append(edit xs, len(xs))`.
* `for x in <temporary list>` keeps the temporary alive for the whole loop.
* `try expr` on the failure path moves the error String into the function's return value, then drops
  all live temporaries and owned locals.

## Tests
`compile_pass/evaluation_order.bv`, `short_circuit_temps.bv`, `for_over_temporaries.bv`,
`crates/beaver-mir/tests/drops.rs`.
