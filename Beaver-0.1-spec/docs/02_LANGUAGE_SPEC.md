# 02 - Beaver 0.1 Language Specification

This document is normative. The grammar in `spec/beaver.ebnf` is a parsing aid; this document determines meaning and error behavior.

## 1. Files, layout and keywords

- UTF-8 `.bv` source; LF or CRLF accepted; normalized to LF internally.
- Indentation defines suites, Python-style. A suite begins after `:` followed by a newline and greater indentation. Indents may have 4, 8, 12, etc. spaces; `beaver fmt` normalizes to 4 per level. Reject tabs in leading indentation, mixed/deduced invalid indentation, missing suites, and inconsistent dedents.
- Case-sensitive identifiers: `[A-Za-z_][A-Za-z0-9_]*` in 0.1. Non-ASCII text is allowed in Strings and comments, but not identifiers.
- Comments begin with `#` outside a String and run to end of line. Blank/comment-only lines don't affect indentation.
- Reserved: `fn`, `type`, `pub`, `const`, `let`, `var`, `if`, `elif`, `else`, `while`, `for`, `in`, `break`, `continue`, `scope`, `pass`, `return`, `use`, `test`, `true`, `false`, `and`, `or`, `not`, `view`, `edit`, `take`, `try`. Builtin functions such as `range`, `ok` and `fail` are identifiers, not reserved words.
- No semicolons, braces, variable-length tabs, multiline Strings, tuple syntax, class declarations, user-defined generics or decorators.

## 2. Types and values

| Source type | Semantics | Representation suggestion | Copy? |
|---|---|---|---|
| `Int` | signed 64-bit integer | `int64_t` | yes |
| `Float` | IEEE-754 binary64 | `double` | yes |
| `Bool` | exactly `true`/`false` | `_Bool` | yes |
| `Byte` | unsigned 8-bit integer | `uint8_t` | yes |
| `String` | immutable UTF-8 bytes | ptr+length+ownership tag | no |
| `Void` | no value | C `void` | n/a |
| `type R:` | record with named fields | C struct | iff all fields Copy |
| `List[T]` | growable owning homogeneous sequence | ptr+len+capacity | no |
| `Buffer` | fixed-length owning zero-initialized byte storage | ptr+len | no |
| `Result[T]` | success `T` or error `String` | tagged union | no |

`T` for List and Result may be a named concrete type, built-in type, or a further nesting of built-in containers (subject to ordinary ownership checks). User-defined generic types/functions are NOT in v0.1. Recursive records by value are invalid, even through `List` if their compile-time value representation would be infinite; a `List` breaks value-size recursion because it has fixed-size header storage.

`Void` is allowed only as function return or `Result[Void]`; no `Void` variables, list elements, or record fields.

`String` is an owning value semantically, even if optimized as a static literal. UTF-8 is validated on source input; `len(s)` returns **byte length**, not grapheme count. No direct String indexing.

## 3. Bindings and assignment

```beaver
const LIMIT: Int = 100
let name = "Beaver"
var count: Int = 0
count = count + 1
```

`const` is top-level only, requires explicit type, and accepts compile-time literal or numeric/Boolean constant expressions only; String literal constants are allowed. It is globally immutable. `let` is immutable; `var` is assignable. Local type annotations are optional if inferrable. Shadowing in a nested block is allowed, re-declaration of the same name in a scope is an error. All variables are initialized at declaration. No reinitialization of moved locals in v0.1.

Assignment targets: mutable `var` bindings, mutable record fields accessible through a mutable owner, and checked index targets into mutable lists and buffers. Assigning to a `let`-owned record field is forbidden. A local `let alias = edit x` can mutate the referent with `alias = new_value`, because `let` fixes the reference binding, not the underlying storage; it cannot be rebound to refer to another object.

## 4. Expressions

- Literals: `123`, `-123` (unary operator + integer token), `1.5`, `true`, `false`, `"text"`, and list literals `[1, 2, 3]`.
- String escapes: `\\`, `\"`, `\n`, `\r`, `\t`, `\0`, `\u{HEX}` (valid Unicode scalar value encoded as UTF-8). No interpolation or triple-quoted Strings.
- Integer arithmetic `+ - * / %`, unary `-`; checked at runtime. Division truncates toward zero. Zero divisor and `INT64_MIN / -1` trap. Signed overflow traps instead of wrapping or C undefined behavior. `Byte` uses explicit `Byte(Int)` conversion and comparisons; no implicit Int/Byte arithmetic.
- Float arithmetic `+ - * /`, unary `-`, IEEE-754 semantics. Float division by zero follows IEEE-754 (Inf/NaN). No fast-math assumptions in 0.1.
- `==`, `!=` compare equal-compatible primitive values, Strings by contents, and records recursively only when all fields support equality. `List`, `Buffer`, `Result` equality is not in v0.1.
- `<`, `<=`, `>`, `>=` work for Int/Float/Byte; String ordering is out of scope.
- `and`, `or`, `not` operate on Bool only; `and`/`or` short-circuit left-to-right.
- `String + String` creates a new owned String (may optimize at compile time for literals). No implicit numeric-to-String conversion.
- Field selection `point.x`, indexed access `list[i]`/`buffer[i]`, call `fn(args)`, qualified symbol `module.fn(args)`.
- All expressions evaluated left-to-right; function arguments evaluated in source order.
- No implicit numeric conversions. Builtins `Float(Int)`, `Int(Float)` (finite and range checked), `Byte(Int)` (range checked) implement explicit conversion.
- No assignment expressions, bitwise operators, pointer arithmetic, method calls, operator overloading, ternary expression or increment/decrement operators.

Operator precedence (highest to lowest): grouping/call/field/index; unary `not`, unary `-`, `view`/`edit`/`take`/`try`; `* / %`; `+ -`; comparisons; `and`; `or`. Chained comparisons (`a < b < c`) are forbidden: use `a < b and b < c`. The parser must reject invalid lvalues for `edit` and `take`.

## 5. Functions

```beaver
fn add(a: Int, b: Int) -> Int:
    return a + b

fn say(text: String):
    print(text)

fn change(number: edit Int):
    number = number + 1

fn consume(text: take String):
    print(text)
```

- Functions are top-level declarations, optionally `pub`; all parameters require a type; default return is Void. No overloads, default parameter values, variadic user functions, or nested functions.
- Default parameter passing: Copy types pass by value; non-Copy types pass as read-only borrows. `view T` always means explicit shared immutable borrow, including for Copy T. `edit T` means an exclusive mutable borrow; `take T` moves ownership into callee.
- `f(value)` is valid for ordinary parameters (a read-only borrow is implicit for non-Copy). `f(edit value)` is mandatory for `edit T`; `f(take value)` mandatory for `take T` if value is named owner. Temporaries can be passed to `take` without writing `take`.
- Borrowed parameters cannot be returned, stored in records/lists/Results, or moved. `take` parameters can be transferred or returned with `take`.
- Non-Void functions must return on every reachable path; no implicit final expression return. A freshly constructed non-Copy temporary can be returned directly; returning a named owned non-Copy local requires `return take local`.
- `main` must have signature `fn main():` with no parameters and Void return for binary packages. Exactly one root `main`, not exported.
- Recursion is allowed. Stack exhaustion is a runtime failure and no stack-limit guarantee exists in 0.1.

## 6. Branching and loops

```beaver
if score > 80:
    print("high")
elif score > 50:
    print("medium")
else:
    print("low")

var i = 0
while i < 5:
    i += 1

for n in range(0, 5):
    print(n)
```

Assignment shorthands `+=`, `-=`, `*=`, `/=` are supported for mutable numeric lvalues and checked with the same semantics as expanded operations. `for n in range(start, end)` visits `start...end-1`, step +1; endpoints evaluated exactly once. `for value in list` traverses in order and holds a read borrow of the list for the loop duration. Copy elements bind a copy; non-Copy elements bind a read-only element reference per iteration. Mutating the iterated list during iteration is forbidden. `break`/`continue` require a loop. `scope:` creates an otherwise inert lexical scope (useful for ending borrows).

A moved owner across conditional branches is conservatively rejected if its ownership state is not identical at a control-flow join. Code that would require dynamic partial-move recovery is rejected in 0.1. `return`, `break`, and `continue` must release all still-owned values whose scope is exited.

## 7. Records

```beaver
pub type Point:
    x: Float
    y: Float

fn move(p: edit Point, dx: Float):
    p.x += dx

fn main():
    var p = Point(x=1.0, y=2.0)
    move(edit p, 3.0)
    print(p.x)
```

Records contain fields only. Constructor calls must provide all declared fields exactly once by name, in any order. Expressions evaluated left-to-right, then placed in declared field order. A non-Copy owned local supplied to a constructor must be explicitly moved with `take`. Records recursively own their fields. Borrowing a field locks the entire parent owner for alias analysis in v0.1.

## 8. Lists, buffers and builtins

```beaver
var values: List[Int] = [10, 20, 30]
append(edit values, 40)
print(len(values))
print(values[2])

var bytes = Buffer(size=16)
bytes[0] = Byte(65)
print(bytes[0])
```

- List literals infer `List[T]` from elements; empty `[]` requires an expected type. Elements must have one compatible static type. List owns elements; inserting a named non-Copy element requires `take`.
- `len(value)` returns Int for List/Buffer/String (String length is bytes).
- `append(edit list, element)` mutates a List, growing its capacity as needed; it takes owned non-Copy elements or copies Copy elements. No append on Buffer.
- Buffer is fixed-size, zero-initialized and indexed by checked Int to Byte. `Buffer(size=n)` takes a nonnegative Int, allocates at runtime, and may trap on OOM/oversize.
- `list[index]` yields a copy for Copy elements and a read-only borrow for non-Copy elements in read contexts. `take list[index]` is forbidden. `list[index] = expr` requires mutable list ownership or an exclusive edit borrow, and safely destroys any replaced owned element.
- `clone(value)` returns an owned deep clone for non-Copy types and a copy for Copy types. Clone can allocate and may fail with an OOM trap.
- `print(value)` prints one Int/Float/Byte/Bool/String followed by one newline. For a referenced primitive, it prints the referent. Printing List/Buffer/record/Result values is out of scope.
- `assert(condition)` requires Bool; traps with file/line if false. `assert_eq(a, b)` compares values with supported equality and traps with location if unequal.
- Basic intrinsics above are built into the compiler; there is no automatic import requirement for them.

## 9. Basic recoverable errors

`Result[T]` contains either an owned successful `T` value or an owned String error. It is always non-Copy.

```beaver
fn divide(a: Int, b: Int) -> Result[Int]:
    if b == 0:
        return fail("division by zero")
    return ok(a / b)

fn calculate() -> Result[Int]:
    let n = try divide(10, 2)
    return ok(n + 5)
```

- `ok(value)` constructs success; `ok()` for `Result[Void]`.
- `fail(string)` constructs error. The String is owned, copied by explicit `clone` if necessary, or moved with `take`.
- `try expr` is valid only when expr has `Result[T]` and the enclosing function returns `Result[U]`. On error, it returns the moved String error; on success, it yields the owned T value (or Void).
- `is_ok(result)` reads a Result by borrow and returns Bool.
- `unwrap(take result)` consumes a Result and yields the owned success or traps if error; `unwrap_error(take result)` yields the owned error or traps if success. Temporary results may be passed directly.
- Result is not an exception system. Arithmetic traps, OOM, bounds failures, and assertion failures are not recoverable with Result in 0.1.

## 10. Modules, imports, tests

- `use util` imports an in-package `src/util.bv` module; `use net.http` imports `src/net/http.bv`.
- `use mathkit` imports the entry library module `src/lib.bv` from a direct dependency named `mathkit`.
- `use mathkit.tools` imports that dependency's `src/tools.bv`.
- Calls use namespaces: `util.double(3)` or `mathkit.square(3)`. Re-exporting imported symbols is not supported.
- `pub` exposes top-level declarations to other modules; unmarked declarations remain module-private. Public functions cannot expose private record types in signatures.
- Import cycles are compile errors. Import names cannot conflict with locals or other imports in the same scope.
- Only `tests/*.bv` files may contain top-level `test "label":` blocks. The test runner compiles them into a native executable and reports per-test pass/fail. Tests may import modules and use `assert`/`assert_eq`.

## 11. Runtime observability and failures

- A normal `fn main()` returning completes with exit status 0.
- `assert` failures, failed `unwrap`, checked integer overflow, divide by zero, invalid conversion, out-of-bounds access and unhandled OOM terminate with a nonzero exit status and a readable stderr diagnostic; no undefined behavior.
- No guaranteed stack traces in 0.1; include source path/line if available in generated checks.
- User `print` goes to stdout; compiler diagnostics, package manager errors, and runtime failures go to stderr.

## 12. What the compiler must reject

Type mismatch; unknown symbol; duplicate symbol; uninitialized/moved uses; invalid `take` of borrowed owner; simultaneous `view` and `edit` overlapping the same owner; multiple `edit` aliases; editing an immutable value; returning or persisting borrowed refs; moving out of an indexed list or borrowed field; creating invalid by-value record cycles; unresolved import; `try` in non-Result function; return path missing in non-Void fn; indexing Strings; invalid package visibility; explicit `unsafe`/`share`/raw-pointer syntax as unavailable v0.1 features.
