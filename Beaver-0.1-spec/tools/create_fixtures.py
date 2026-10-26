#!/usr/bin/env python3
"""Materialize proposed Beaver language conformance fixtures (no Beaver compiler needed)."""
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / 'fixtures'

BAD = {
    'use_after_take': ('B3', '''fn main():
    let name = "hello"
    let another = take name
    print(name)
'''),
    'overlapping_borrow': ('B3', '''fn main():
    var number = 1
    let reader = view number
    let writer = edit number
    print(reader)
'''),
    'borrowed_return': ('B3', '''fn bad() -> String:
    let value = "hello"
    let alias = view value
    return alias

fn main():
    print("test")
'''),
    'mutate_immutable': ('B2', '''fn main():
    let value = 3
    value = 4
'''),
    'type_mismatch': ('B2', '''fn main():
    let value: Int = "hello"
'''),
    'borrow_different_fields': ('B3', '''type Pair:
    x: Int
    y: Int

fn main():
    var pair = Pair(x=1, y=2)
    let left = view pair.x
    let right = edit pair.y
    print(left)
'''),
    'mutate_during_iteration': ('B3', '''fn main():
    var values = [1, 2]
    for value in values:
        append(edit values, 3)
'''),
    'take_indexed_value': ('B3', '''fn main():
    var names = ["a", "b"]
    let one = take names[0]
'''),
    'conditional_move_join': ('B3', '''fn main():
    let name = "hello"
    if true:
        let another = take name
    print(name)
'''),
    'invalid_if_condition': ('B2', '''fn main():
    if 1:
        print("invalid")
'''),
    'missing_return_path': ('B2', '''fn maybe(flag: Bool) -> Int:
    if flag:
        return 1

fn main():
    print(0)
'''),
    'indentation': ('B1', '''fn main():
   print("bad indentation")
     print("bad indent level")
'''),
    'invalid_index_type': ('B2', '''fn main():
    let values = [1, 2]
    print(values[1.5])
'''),
    'try_without_result': ('B2', '''fn helper() -> Result[Int]:
    return ok(1)

fn main():
    let n = try helper()
    print(n)
'''),
    'reference_inside_list': ('B3', '''fn main():
    let n = 1
    let alias = view n
    let values = [alias]
'''),
    'edit_immutable': ('B3', '''fn main():
    let n = 1
    let writer = edit n
    writer = 2
'''),
    'duplicate_names': ('B2', '''fn main():
    let n = 1
    let n = 2
'''),
    'out_of_scope': ('B2', '''fn main():
    scope:
        let inner = 7
    print(inner)
'''),
}

GOOD = {
    'lexical_borrow': '''fn main():
    var n = 10
    scope:
        let read = view n
        print(read)
    scope:
        let write = edit n
        write += 10
    print(n)
''',
    'recursion': '''fn factorial(n: Int) -> Int:
    if n <= 1:
        return 1
    return n * factorial(n - 1)

fn main():
    print(factorial(5))
''',
    'lists_of_string': '''fn main():
    var names: List[String] = ["Alice", "Bob"]
    let other = "Charlie"
    append(edit names, take other)
    for name in names:
        print(name)
''',
    'record_constructor': '''type Person:
    name: String
    age: Int

fn main():
    let person = Person(name="Ada", age=37)
    print(person.name)
''',
    'record_mutation': '''type Point:
    x: Int
    y: Int

fn translate(p: edit Point, dx: Int):
    p.x += dx

fn main():
    var p = Point(x=1, y=2)
    translate(edit p, 4)
    print(p.x)
''',
    'try_result': '''fn divide(x: Int, y: Int) -> Result[Int]:
    if y == 0:
        return fail("division by zero")
    return ok(x / y)

fn compute() -> Result[Int]:
    let x = try divide(20, 4)
    return ok(x + 1)

fn main():
    print(unwrap(compute()))
''',
    'loops': '''fn main():
    var total = 0
    for n in range(0, 5):
        if n == 3:
            continue
        total += n
    print(total)
''',
    'list_buffer': '''fn main():
    var data = Buffer(size=8)
    data[2] = Byte(42)
    print(data[2])
    var values: List[Int] = []
    append(edit values, 5)
    print(values[0])
''',
}

RUNTIME = {
    'arithmetic': ('exit=0\nstdout=8\n', '''fn main():
    print(5 + 3)
'''),
    'overflow': ('exit=nonzero\nstderr=B5\n', '''fn main():
    let max: Int = 9223372036854775807
    print(max + 1)
'''),
    'negative_overflow': ('exit=nonzero\nstderr=B5\n', '''fn main():
    let min: Int = -9223372036854775807 - 1
    print(-min)
'''),
    'bounds': ('exit=nonzero\nstderr=B5\n', '''fn main():
    let values = [1, 2]
    print(values[9])
'''),
    'division_zero': ('exit=nonzero\nstderr=B5\n', '''fn main():
    let divisor = 0
    print(10 / divisor)
'''),
    'string_clone': ('exit=0\nstdout=hellohello\n', '''fn main():
    let a = "hello"
    let b = clone(a)
    print(a + b)
'''),
    'try_failure': ('exit=0\nstdout=bad input\n', '''fn inner() -> Result[Int]:
    return fail("bad input")

fn outer() -> Result[Int]:
    let n = try inner()
    return ok(n)

fn main():
    let result = outer()
    if is_ok(result):
        print("unexpected")
    else:
        let message = unwrap_error(take result)
        print(message)
'''),
    'buffer_byte': ('exit=0\nstdout=65\n', '''fn main():
    var b = Buffer(size=4)
    b[0] = Byte(65)
    print(b[0])
'''),
    'short_circuit': ('exit=0\nstdout=safe\n', '''fn panic_if_called() -> Bool:
    assert(false)
    return true

fn main():
    if false and panic_if_called():
        print("bad")
    else:
        print("safe")
'''),
    'clone_owned_list': ('exit=0\nstdout=hello\n', '''fn main():
    var names = ["hello", "world"]
    let other = clone(names)
    print(other[0])
'''),
}

for name, (error, contents) in BAD.items():
    base = ROOT / 'compile_fail' / name
    base.with_suffix('.bv').write_text(contents)
    base.with_suffix('.expect').write_text(error + '\n')
for name, contents in GOOD.items():
    (ROOT / 'compile_pass' / (name + '.bv')).write_text(contents)
for name, (expected, contents) in RUNTIME.items():
    base = ROOT / 'runtime' / name
    base.with_suffix('.bv').write_text(contents)
    base.with_suffix('.expect').write_text(expected)
print(f'Wrote {len(BAD)} compile-fail, {len(GOOD)} compile-pass, and {len(RUNTIME)} runtime fixtures.')
