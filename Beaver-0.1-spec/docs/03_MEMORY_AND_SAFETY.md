# 03 - Ownership, Borrowing, Destruction, and Memory Safety

## Ownership model

The compiler maintains a state for each owned local `available | moved`. Each non-Copy value has one owner. Assignment from a named owned non-Copy value requires `take`; a temporary is implicitly transferred. Borrowed references never own. Primitive scalars and recursively Copy records can be copied without `take`.

Readable operations:

```beaver
var x = 10
scope:
    let reader = view x
    print(reader)
scope:
    let writer = edit x
    writer = 20
print(x)
```

`view` produces a non-owning shared read reference, `edit` a non-owning unique mutable reference, `take` transfers the owner. Source never needs `*`, `&`, dereference operators, or pointer arithmetic. The C backend may use native pointers internally.

## Borrow timeline

- Local borrow bindings stay alive until their lexical scope closes, regardless of last use.
- A function argument borrow starts when the function call begins and ends at the end of the full call statement/expression evaluation. Nested expression borrows must have consistent ordering and must not alias a simultaneously edited owner.
- `scope:` creates explicit lifetime boundaries.
- Any number of `view` references may overlap. A single `edit` reference requires no active overlapping view/edit reference.
- Borrow conflict checks are conservative at *owner* granularity. `view person.name` conflicts with `edit person.age` when both are bound across a scope.
- A borrowed reference cannot outlive its owner, be returned, be assigned into heap structures or records, be captured (closures do not exist), or be moved.
- Named borrows are transparent for reading; an edit borrow can write to its referent via assignment or field/index lvalues.
- For `for item in list`, the list has a read borrow spanning the loop. Mutable mutation through another alias is a compile-time error.

## Examples: valid

```beaver
fn increment(value: edit Int):
    value += 1

fn main():
    var value = 3
    increment(edit value)
    scope:
        let r = view value
        print(r)
    increment(edit value)
```

```beaver
fn use_text(text: String):
    print(text)

fn main():
    let word = "hello"
    use_text(word)       # read borrow, not an ownership transfer
    use_text(word)       # still valid
```

```beaver
fn consume(text: take String):
    print(text)

fn main():
    let value = "hello"
    consume(take value)
    # value cannot be read or dropped again
```

## Examples: invalid

```beaver
var n = 1
let a = view n
let b = edit n     # error: a's lexical read borrow is still active
```

```beaver
let text = "hi"
let moved = take text
print(text)       # error: use after ownership transfer
```

```beaver
fn bad() -> String:
    let name = "hello"
    let alias = view name
    return alias   # error: borrowed values cannot escape
```

```beaver
var list = [1, 2]
for x in list:
    append(edit list, 3)  # error: active loop read borrow
```

## Deterministic drops

Create explicit `Move`, `BorrowRead`, `BorrowWrite`, `ReleaseBorrow`, and `DropOwned` operations in the typed MIR. Emit drops for active local owners at all scope exits, including `return`, `break`, `continue`, and normal fallthrough. Deconstructing a record recursively drops owned fields in reverse declaration order. Dropping a List recursively drops each live element, then the allocation. Dropping a Result frees exactly its active success or error payload; drop never touches inactive union storage.

After a move, the source's drop is disabled (statically or via generated drop-state bits); the destination becomes the unique owner. The compiler must not double-free conditional moves. In v0.1, reject paths whose post-join ownership states disagree instead of silently inserting untested dynamic partial-move machinery.

## Lifetime/type-state validation algorithm

Proposed initial algorithm (conservative):

1. Type-check all owners and borrowing expressions; assign each borrow to a root owner and a lexical or temporary scope ID.
2. Traverse typed control-flow CFG with states `OwnedAvailable`, `OwnedMoved`, `ReadBorrowed(n)`, `WriteBorrowed`, tracking active borrows scoped to call expressions or lexical blocks.
3. Reject `take` while any borrow is active; reject `edit` unless owner is mutable and has no active borrow; reject a read borrow if an edit exists.
4. Release named borrows at block scope exit and temporary borrows at call evaluation completion.
5. For joins, accept only state combinations that are equal for names live beyond the join; for loops, prevent ownership moves of outer-scope variables where the loop may iterate arbitrarily unless analysis can prove a valid fixed point. Conservative rejection is allowed.
6. Compute drop insertion using the ownership state on outgoing edges. If ownership state differs and no safe static drop plan is available, reject the program rather than generating unsafe code.
7. Reject returning references or indirect storage of borrow-only types.

Tests must verify compile errors AND runtime drops under all statement exits. For v0.1, `take` on an owner that is also aliased in a different record field is forbidden; field-level move is unsupported.

## Runtime representation guidance

- `String`: immutable UTF-8 ptr/len plus either static-data or unique heap-owner tag. All printing must be byte-length-aware rather than using an unbounded `%s` C format, since Strings may contain `\0`. Literal data can point to static program memory; concatenation/clone allocates heap storage. `Drop` frees only heap-owned payloads; integer length arithmetic checked for overflow.
- `List[T]`: `ptr/len/cap`, typed element size/alignment, capacity growth with overflow checks, element clone/drop operations specialized by concrete T. Do not use `memcpy` for non-Copy elements unless the bytewise transfer is a one-way move with old owner invalidated.
- `Buffer`: fixed-length zero-initialized allocation, ptr/len; no negative size; bounds checked reads/writes.
- `Result[T]`: tag with active union payload, owned success or error. `try` transfers active payload without double-drop.
- Records: inline fields, recursive drop/clone. Record with all Copy fields may be trivially assigned.
- Borrowed refs: C pointers/struct pointers, produced only after validation in compiler; no borrowed reference leaves its valid lifetime.

## Memory safety obligations

- **Bounds:** reject negative indices; guard `index < len` before pointer math or loads; compiler can omit a guard only after proof.
- **Arithmetic:** use checked helpers for `Int` add/sub/mul/div/mod/neg (with tests including signed limits); never rely on signed C overflow behavior.
- **Allocation:** detect size/capacity multiplication overflow before allocating. On OOM print a clear runtime error and exit nonzero. Never dereference NULL.
- **Initialization:** no reads of uninitialized memory; Bytes in a new Buffer are zero-filled, List elements initialized before `len` increments.
- **Copies:** no implicit copies of String/List/Buffer/Result/non-Copy records; require `clone` for duplication, `take` for transfer.
- **C backend:** warnings enabled; no UB from invalid shifts (none in 0.1), invalid pointer arithmetic, uninitialized unions, strict aliasing, C evaluation-order differences, or checked int overflow.
- **ABI:** internal representations are implementation details; no stable C FFI/ABI guarantee in 0.1.

## What APLA means for this MVP

No whole-program automatic stack/heap escape analysis is required. Use native locals and inline records where possible, and heap-allocate dynamic String/List/Buffer payloads. Compiler-enforced ownership, borrow validity and deterministic drops deliver the central safety model; sophisticated allocation placement can be added after correctness tests pass.
