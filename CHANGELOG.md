# Changelog

## 0.1.0 (unreleased)

First implementation of the Beaver 0.1 handoff.

* Compiler: indentation-aware lexer, recursive-descent parser, module loader with `use`/`pub`,
  type checker, lexical ownership/borrow checker (`view`/`edit`/`take`), typed MIR with explicit
  moves, borrows and drops, and a C11 backend driving the host C compiler.
* Runtime: checked Int arithmetic, bounds checks, UTF-8 Strings, Lists, Buffers, Results,
  allocation accounting (`BEAVER_RT_LEAK_CHECK=1`), traps with source locations, native test
  driver.
* CLI: `new`, `init`, `check`, `build`, `run`, `test`, `fmt`, `add`, `remove`, `update`, `fetch`,
  `package`, plus `--offline`, `--locked`, `--release` and `--registry-url`.
* Packages: `beaver.toml` validation, deterministic `beaver.lock`, registry protocol v1 client
  (HTTPS; loopback HTTP only with explicit opt-in; no redirects), content-addressed cache,
  SHA-256 verification, hardened archive extraction, deterministic `.tar.gz` packaging.
