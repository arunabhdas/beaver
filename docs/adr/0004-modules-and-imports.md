# ADR 0004: Import names and module resolution

## Problem
The spec gives `use util` → `util.double(3)` and `use mathkit.tools` but does not say what name
`use net.http` binds, nor what happens when a dependency and a local module share a first segment.
The `mathkit` example's test uses `use lib` to reach its own `src/lib.bv`.

## Choice
* `use a.b.c` binds the **last segment** (`c`). Two imports with the same last segment in one module
  are an error (B2027), as are top-level items and locals named like an import.
* The first segment is looked up among the package's **direct** dependencies first. `use dep` imports
  the dependency's entry module; `use dep.x.y` imports `src/x/y.bv` of that dependency. Otherwise the
  path is a module of the current package (`src/a/b/c.bv`). A dependency name that is also a local
  module (`src/<name>.bv` or `src/<name>/`) is an ambiguity error (B2014).
* Test files (`tests/*.bv`) resolve imports like any module of the package, so `use lib` inside a
  library's tests reaches `src/lib.bv`. Test files cannot be imported.
* Module files must be regular files inside the package (symlinks and canonical-path escapes are
  rejected). Import cycles are reported with the full chain (B2015).
* C symbols are mangled with length-prefixed segments: `bvr_<len><pkg><len><seg>..._<len><name>`,
  with `bvt_` for test modules, so they cannot collide across packages or modules.

## Tests
`crates/beaver-cli/tests/workflow.rs::multi_file_application_with_local_module_and_library_package`,
`visibility_imports_and_cycles_are_enforced`.
