# Beaver 0.1 diagnostic codes

Every compiler, package-manager and runtime error carries a stable code. Compiler errors also show
`file:line:column`, the offending source line with a caret, and usually a `help:` line.

```
error[B3002]: cannot edit `value` while it is borrowed
 --> src/main.bv:4:18
  |
4 |     let writer = edit value
  |                  ^^^^^^^^^^ conflicting exclusive borrow
  |
note: `value` already has an active `view` borrow from line 3; borrows of any field borrow the whole value
note: the earlier borrow remains active until its lexical scope ends
help: wrap `reader` in `scope:` to shorten the borrow lifetime
```

## B1xxx: lexing, indentation, parsing

| Code | Meaning |
|---|---|
| B1001 | Source file is not valid UTF-8 |
| B1002 | Tab character in leading indentation |
| B1003 | Indentation is not a multiple of 4 spaces |
| B1004 | Dedent does not match any enclosing indentation level |
| B1005 | Unexpected indentation (no preceding `:`) |
| B1006 | Missing indented block after `:` |
| B1007 | Unterminated string literal |
| B1008 | Invalid escape sequence (`\q`, bad `\u{...}`) |
| B1009 | Unexpected character (`&`, `{`, `;`, `!`, `|`, non-ASCII identifier, ...) |
| B1010 | Integer literal does not fit in Int |
| B1011 | Invalid numeric literal (`12abc`, `1.5.3`, out-of-range float) |
| B1012 | General syntax error (unexpected token) |
| B1013 | Chained comparison (`a < b < c`) |
| B1014 | Invalid assignment target |
| B1015 | `view`/`edit`/`take` applied to something that is not a variable, field or element |
| B1016 | Stray carriage return |
| B1017 | Unsupported feature keyword (`class`, `share`, `unsafe`, `match`, ...) |
| B1099 | Internal formatter safety check (formatting would change tokens; file left untouched) |

## B2xxx: names, types, modules, return analysis

| Code | Meaning |
|---|---|
| B2001 | Unknown name / function / module item |
| B2002 | Duplicate definition (item, local in the same scope, parameter, field, test name) |
| B2003 | Type mismatch |
| B2004 | `if`/`elif`/`while` condition is not Bool |
| B2005 | Operator or builtin applied to unsupported types |
| B2006 | Non-Void function may finish without returning |
| B2007 | Assignment to an immutable variable or parameter |
| B2008 | `try` outside a Result-returning function, or on a non-Result value |
| B2009 | Unknown or malformed type |
| B2010 | Wrong number or kind (named/positional) of arguments |
| B2011 | Record field errors (unknown, missing, duplicate, positional constructor) |
| B2012 | Calling something that is not a function, or using a function/type as a value |
| B2013 | Access to a non-`pub` item from another module |
| B2014 | Module not found / ambiguous import / invalid entry path |
| B2015 | Import cycle |
| B2016 | Index is not Int, or value cannot be indexed |
| B2017 | String indexing (not supported in 0.1) |
| B2018 | Type cannot be inferred (`[]`, `fail(...)` without context) |
| B2019 | Invalid use of `Void` |
| B2020 | Missing or malformed `fn main():` in a binary package |
| B2021 | Record contains itself by value (infinite size) |
| B2022 | `break`/`continue` outside a loop |
| B2023 | Invalid constant (non-constant expression, overflow, cycle, unsupported type) |
| B2024 | `test` block outside `tests/*.bv` |
| B2025 | Equality on a type that does not support `==` |
| B2026 | Public declaration exposes a private record type |
| B2027 | Name conflicts with an imported module name |
| B2028 | Redefinition of a builtin name, or `range(...)` outside a `for` loop |
| B2029 | `return` value mismatch (value in a Void function or missing value) |

## B3xxx: ownership, borrowing, lifetimes

| Code | Meaning |
|---|---|
| B3001 | Use, borrow, move or assignment of a moved value |
| B3002 | Conflicting borrow (`edit` while borrowed, `view` while edited, two `edit`s) |
| B3003 | Reading a value while it is borrowed by `edit` |
| B3004 | `edit` of immutable storage or through a read-only reference |
| B3005 | Moving out of a reference |
| B3006 | Partial move out of a record field or list element |
| B3007 | Implicit move of a non-Copy value (needs `take` or `clone`) |
| B3008 | Reference escapes (returned, or stored in a list/record/Result) |
| B3009 | Ownership state differs at a control-flow join and the value is used afterwards |
| B3010 | Value declared outside a loop is moved inside it (next iteration would use it) |
| B3011 | Assignment through a read-only reference |
| B3012 | `take` while the value is borrowed |
| B3013 | Reference bound with `var` |
| B3014 | Assignment while borrowed |
| B3015 | Move inside the conditionally evaluated operand of `and`/`or` |
| B3016 | `view`/`edit` outside a `let` binding or call argument |
| B3017 | Argument passing mode does not match the parameter (`edit`/`view`/`take`/by value) |

## B4xxx: code generation and host toolchain

| Code | Meaning |
|---|---|
| B4001 | No usable C compiler (`BEAVER_CC`, `CC`, `clang`, `cc`, `gcc`) |
| B4002 | The C compiler rejected generated code (a compiler bug; please report) |
| B4003 | I/O error writing generated files |
| B4004 | Warning: the C compiler printed warnings for generated code |

## B5xxx: runtime checks (process exits with status 101)

| Code | Meaning |
|---|---|
| B5000 | Internal error: unreachable code reached |
| B5001 | Integer overflow (`+ - * /`, negation, `INT64_MIN / -1`) |
| B5002 | Division or remainder by zero |
| B5003 | Index out of bounds (List or Buffer, including negative indices) |
| B5004 | Invalid conversion (`Byte(n)` out of 0..255, `Int(f)` of NaN/infinite/out-of-range Float) |
| B5005 | `assert` / `assert_eq` failed |
| B5006 | `unwrap` on a failed Result, `unwrap_error` on a successful one |
| B5007 | Out of memory / allocation size overflow |
| B5008 | Negative `Buffer(size=...)` |
| B5098 | Internal error: an owned allocation was released twice (never expected) |
| B5099 | With `BEAVER_RT_LEAK_CHECK=1`: owned allocations not released at exit (exit status 102) |

## B6xxx: packages, registry, archives, lockfiles

| Code | Meaning |
|---|---|
| B6001 | Invalid or missing manifest, name or version |
| B6002 | File-system error in a CLI command |
| B6003 | `new`/`init` would overwrite existing files |
| B6004 | `beaver run` on a library package |
| B6005 | `beaver add` of an existing dependency |
| B6006 | `remove`/`update` of something that is not a direct (registry) dependency |
| B6010 | Invalid or unreadable `beaver.lock` |
| B6011 | `--locked` violations, or a stale lockfile in build commands |
| B6012 | Registry checksum or dependencies disagree with `beaver.lock` |
| B6013 | Needed artifact is not cached and `--offline` forbids network access |
| B6014 | No registry configured |
| B6015 | Path dependency not found |
| B6016 | Path dependency name does not match the package's manifest |
| B6020 | Registry URL rejected by the security policy |
| B6021 | HTTP request failed or non-200 status |
| B6022 | Registry responded with a redirect (never followed) |
| B6023 | Package/version not found in the registry |
| B6024 | Response larger than the allowed limit |
| B6025 | Cannot write a download |
| B6026 | Invalid registry metadata |
| B6030 | Unsafe or invalid archive (traversal, links, special files, limits, disallowed files, corrupt gzip/tar) |
| B6031 | Package manifest inside an archive is invalid, mismatched, or not a library |
| B6032 | Registry package declares a path dependency |
| B6033 | Registry metadata and archive manifest disagree about dependencies |
| B6034 | Archive size differs from the declared size |
| B6035 | Archive SHA-256 mismatch |
| B6040 | Dependency cycle |
| B6041 | Conflicting versions/sources of one package |
| B6042 | Dependency graph deeper than 32 |
| B6043 | More than 128 packages in a graph |
| B6050 | Package cache I/O error |
| B6060 | `beaver package` on a binary package |
| B6061 | `beaver package` with path dependencies |
