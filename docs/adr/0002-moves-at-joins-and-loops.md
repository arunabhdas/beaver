# ADR 0002: Ownership states at control-flow joins and in loops

## Problem
The spec says a moved owner "is conservatively rejected if its ownership state is not identical at
a control-flow join", but also "accept only state combinations that are equal for names live
beyond the join" and "if ownership state differs and no safe static drop plan is available, reject".
The runtime fixture `try_failure.bv` moves `result` in the `else` branch only, and must compile.
The compile-fail fixture `conditional_move_join.bv` moves in one branch and *uses the value after*
the join, and must fail.

## Options
1. Reject any differing state (would reject the `try_failure` runtime fixture).
2. Dynamic drop flags (explicitly not wanted for 0.1).
3. Accept a differing state only for values that are never mentioned again in their scope, and drop
   them statically on the paths where they are still owned.

## Choice
Option 3. Liveness is purely syntactic and conservative: a local is "used later" if any later
statement in its scope mentions it, and, inside a loop body, if any statement of the body or the
loop condition mentions it (a later iteration). Locals declared inside the loop body are excluded
from that set, because they are fresh in every iteration. The MIR builder emits `Drop` at the end of
each incoming path that still owns the value, so each value is dropped exactly once and no drop flag
exists at runtime.

Loops: on every back edge (end of body, `continue`) no local declared outside the loop may be in a
moved state, otherwise the next iteration could use or drop it again (B3010). A move followed by
`break` or `return` is fine. The loop exit join (`break` paths plus the normal exit) follows the same
liveness rule. A move inside a `while` condition is a back-edge violation. A move inside the right
operand of `and`/`or` is rejected (B3015) because that operand runs conditionally inside an
expression.

## Tests
`tests/conformance/compile_pass/conditional_move_unused.bv`, `move_then_break.bv`,
`compile_fail/move_in_loop.bv`, `moved_in_while_condition.bv`, `move_in_and_operand.bv`, the spec
fixtures above, `crates/beaver-sema/tests/sema.rs::joins_*`, `loops_*`, and
`crates/beaver-mir/tests/drops.rs::branch_only_moves_get_a_fixup_drop_on_the_other_path`.

## Compatibility
Non-lexical liveness can relax this later without breaking accepted programs.
