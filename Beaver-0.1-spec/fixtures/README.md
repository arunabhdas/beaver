# Beaver Conformance Fixtures

These fixtures are **specification inputs** for Astra's compiler tests. No Beaver compiler is included in this package.

- `compile_pass/*.bv`: each is a standalone valid `src/main.bv` in a generated temporary binary package.
- `compile_fail/*.bv`: each must fail `beaver check` with diagnostic prefix in its neighboring `.expect` file. Each is likewise a temporary package `src/main.bv`.
- `runtime/*.bv`: compile to native code and run; `.expect` has `exit=0`/`exit=nonzero`, optional `stdout=...`, or diagnostic `stderr=B5`. `stdout=` represents the line WITHOUT the trailing newline.
- `registry/v1/packages/*/`: generated static HTTP registry fixtures. `mathkit` should install and run. `badchecksum`, `pathtrap`, `linktrap`, `badgzip` must fail safely. `cycle_a`/`cycle_b` must fail due to cycle. `conflict_root` must fail due to versions of `mathkit` differing through two dependency paths.

## Regenerate all fixtures

```sh
python3 tools/create_fixtures.py
python3 tools/create_mock_registry.py
```

## Serve local test registry

```sh
python3 -m http.server 8765 --bind 127.0.0.1 --directory fixtures/registry
```

In a second terminal:

```sh
export BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1
export BEAVER_REGISTRY_URL=http://127.0.0.1:8765
cd examples/consumer
beaver fetch
beaver run
```

`beaver fetch` / `beaver run` will work **only after Astra has implemented the toolchain**. Until then the fixture server can be inspected through a browser or HTTP client.

## Safety note

The fixture registry includes deliberately malicious tar members to verify path traversal and symlink rejection. Do not use a generic unvalidated tar extraction command against `pathtrap`/`linktrap`. The Beaver installer must examine metadata and entry types first, reject invalid archives and never extract outside its designated directory.
