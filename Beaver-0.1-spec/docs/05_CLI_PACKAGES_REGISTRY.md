# 05 - Beaver CLI, Project Format, Dependencies, and Secure Registry Downloads

This is a complete **client-side package workflow**. Hosting a public registry or publishing to it is not a v0.1 deliverable. The CLI must nonetheless be able to fetch packages from a locally hosted contract-compatible static registry and from a configured trusted HTTPS registry.

## 1. CLI contract

| Command | Required behavior |
|---|---|
| `beaver --help`, `beaver --version` | Help text; report language/tool version. |
| `beaver new NAME [--lib]` | Create NAME folder with binary (default) or library skeleton. Refuse overwrite. |
| `beaver init [--lib]` | Initialize current empty/uninitialized directory. Refuse overwrite of existing manifest/source. |
| `beaver check [--offline] [--locked]` | Parse, resolve imports/deps, type-check and borrow-check only. |
| `beaver build [--release] [--offline] [--locked]` | Native codegen + host C compiler; place binary under target profile. |
| `beaver run [--release] [--offline] [--locked]` | Build and execute the binary; propagate nonzero exit code. Valid only for bin packages. |
| `beaver test [--offline] [--locked]` | Compile and run native `tests/*.bv` tests; report totals; nonzero if any fail. |
| `beaver fmt [--check]` | Canonical formatting or verify unchanged. |
| `beaver add NAME@X.Y.Z [--registry-url URL]` | Add exact registry dependency, resolve entire graph, fetch, lock, update manifest. |
| `beaver add --path PATH` | Add local library dependency based on its manifest name. |
| `beaver remove NAME` | Remove direct dependency; update graph/lockfile, preserving unrelated deps. |
| `beaver update NAME@X.Y.Z [--registry-url URL]` | Change direct dependency to a specific exact version; refresh lock. |
| `beaver fetch [--offline] [--locked] [--registry-url URL]` | Resolve/fetch all dependencies without compiling. |
| `beaver package [--output DIR]` | Validate library package and create canonical `.tar.gz` archive. |

`beaver build` on a library package validates and emits a compiled intermediate object/archive only if desired; at minimum it must type-check and produce no misleading executable. `beaver run` on a library is an error. `beaver test` without test blocks succeeds with `0 tests` after successful compilation.

Global help and all command errors must have user-actionable text. `--offline` forbids network access, even metadata refresh. `--locked` requires an existing, fully consistent lockfile and cannot change it. `--locked` is incompatible with `add`/`remove`/`update`.

## 2. Generated project layouts

Binary package:

```
hello/
  beaver.toml
  src/main.bv
  tests/smoke.bv
  .gitignore
```

Library package:

```
mathkit/
  beaver.toml
  src/lib.bv
  tests/smoke.bv
  .gitignore
```

Both ignore `target/` and should commit `beaver.lock` when dependencies exist. Files are UTF-8 text. No network access for `new`, `init`, or local compilation without dependencies.

## 3. Manifest `beaver.toml`

Binary:

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2026"
kind = "bin"
entry = "src/main.bv"

[dependencies]
```

Library with exact and path dependencies:

```toml
[package]
name = "consumer"
version = "0.1.0"
edition = "2026"
kind = "bin"
entry = "src/main.bv"

[dependencies]
mathkit = { version = "1.0.0" }
localutil = { path = "../localutil" }
```

Rules:

- `name`: ASCII `[a-z][a-z0-9_]{0,63}`; case-normalized and unique, no hyphens or slashes. Reject reserved OS/device names, path-like names, duplicate names (including case-folded aliases).
- `version`: exact three-number SemVer `MAJOR.MINOR.PATCH` with nonnegative integers (no prereleases/build suffix in 0.1).
- `edition`: exact string `"2026"`.
- `kind`: `"bin"` or `"lib"`. For bin, entry defaults to `src/main.bv` if omitted; for lib, to `src/lib.bv`.
- `entry`: relative path strictly inside project; cannot contain `..`, absolute prefixes, symlink escapes, or files outside `src/`. `.bv` required.
- Dependencies must be exact versions or local paths. A name may not appear in two forms.
- Library packages may depend on other libraries, never directly on executable packages.
- Registry packages cannot contain `path` dependencies because they cannot be reproduced in external projects. `beaver package` rejects such manifests.
- Unknown top-level keys or conflicting fields are errors (future edition may version them).

## 4. Lockfile `beaver.lock`

Use deterministic TOML (sorted package entries and dependencies):

```toml
lock_version = 1

[[package]]
name = "mathkit"
version = "1.0.0"
source = "registry+https://registry.example.invalid"
checksum = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
dependencies = []
```

The URL above is a placeholder and **not a real Beaver registry**. `checksum` is SHA-256 hex of the compressed archive bytes. For local path dependencies, use `source = "path+../localutil"` with no checksum; these are intentionally not content-pinned, and must exist locally. The root project itself need not have a lockfile entry. Lock entries record every resolved transitive dependency and its exact direct dependencies (`name@version` strings). Include the registry URL per remote package; one registry per dependency graph in 0.1.

Lockfile behavior:

- `build/check/test/run` use lockfile versions when present; fail if it conflicts with the manifest unless explicitly running a command allowed to rewrite it without `--locked`.
- Prefer not to rewrite a valid lockfile unnecessarily. Use atomic write+rename and preserve existing file on errors.
- `--locked` rejects missing, stale, inconsistent or altered lockfiles.
- A warm offline cache can reproduce lockfile dependencies without visiting the network.
- Recalculate and verify archive SHA-256 on first use after download, and check cached archive integrity before extraction/use.
- Local path dependencies can change without lock hash; this must be explained in the CLI docs.

## 5. Registry configuration

No public default registry exists in 0.1. **Never hardcode an invented public Beaver registry.**

Order of precedence:
1. `--registry-url URL` on remote package commands.
2. `BEAVER_REGISTRY_URL` environment variable.
3. User config (platform config directory, e.g. `~/.config/beaver/config.toml`):

```toml
[registry]
url = "https://registry.example.invalid"
```

4. For an already locked package, use its locked registry URL (subject to security checks).

If a remote package needs to be resolved and no registry is configured/locked, emit a clear configuration error. Registry URLs must be HTTPS with valid TLS certificates, except `http://127.0.0.1:<port>` or `http://[::1]:<port>` when `BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1` is explicitly set for local integration testing. Never allow insecure HTTP to a non-loopback host. Reject redirects for 0.1 to avoid changing trust origins and redirect-based SSRF.

## 6. Registry HTTP protocol version 1

For `name=mathkit`, `version=1.0.0`:

```
GET {base_url}/v1/packages/mathkit/1.0.0/metadata.json
Accept: application/json
```

Successful response (max 128 KiB):

```json
{
  "schema": 1,
  "name": "mathkit",
  "version": "1.0.0",
  "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
  "archive_size": 2048,
  "dependencies": {}
}
```

Then:

```
GET {base_url}/v1/packages/mathkit/1.0.0/archive.tar.gz
Accept: application/gzip
```

Archive response must contain exactly the declared number of bytes (or fail) and match SHA-256. `archive_size` is compressed bytes length. Client streams the download to a temporary file and verifies size and hash **before** entering the content-addressed cache and before extraction. No executable code runs during download or extraction.

The registry metadata dependency graph must match the archive manifest's exact dependencies. Disagreement is an error. Fetch metadata and packages recursively with limited depth; no dynamic SemVer resolution. On missing package/version, use helpful messages rather than falling back to arbitrary versions.

## 7. Dependency resolution

1. Load root manifest. Resolve root path dependencies (validate their manifests) and exact registry-version dependencies.
2. Query/download metadata for each remote dependency or use a lockfile+warm verified cache.
3. Expand transitive exact dependencies (depth-first or breadth-first). Only direct dependencies may be used as `use` imports in a source package.
4. If the same package name resolves to conflicting versions or registries, fail with dependency-chain diagnostics. No parallel versions of one name in v0.1.
5. Reject cycles and excess graph depth (>32) or package count (>128) rather than infinite recursion.
6. For each registry archive, verify metadata name/version and the archive manifest's dependency set before accepting.
7. Serialize a stable sorted lockfile; atomic update of manifest+lockfile for `add/remove/update`, with failure rollback.
8. Compile all source modules in dependency-order in one linking operation. No dependency build scripts.

## 8. Archive packaging and extraction

Format: deterministic `gzip(tar)` with a single root directory `${name}-${version}/`. Allowed contents: `beaver.toml`, `src/**/*.bv`, optional `README.md`, `LICENSE`, and optional test sources (tests are not executed automatically when installed). Exclude `target`, VCS metadata, secrets, generated binaries, `beaver.lock` and unrelated files.

Canonical archive requirements: sort paths lexicographically, normalize uid/gid/mtime/modes, fixed gzip timestamp, stable output for identical inputs. Refuse symlinks/hardlinks/device files and paths resolving outside the package root. `beaver package` prints the archive path and SHA-256.

Extraction requirements: reject absolute paths, `..` path components, Windows drive/UNC paths, symlinks, hardlinks, special files, duplicate paths, case-insensitive collisions, filenames with prohibited components or NUL, and archives with a manifest that disagrees with metadata. Make output directory private and extract atomically after checks. Enforce compressed archive <=20 MiB, total expanded size <=100 MiB, single file <=10 MiB, <=8192 entries, metadata <=128 KiB. Any over-limit condition is a hard error.

## 9. Cache layout

Platform-aware cache root (e.g. `$XDG_CACHE_HOME/beaver`):

```
cache/
  metadata/<registry-hash>/<name>/<version>.json
  archives/<sha256>.tar.gz
  sources/<sha256>/<name>-<version>/...
```

Cache archives by content hash and verify integrity; writes atomic. Remove partial files on errors. Add timeouts (connect <=10s, request <=60s) and an upper bound on streamed response bytes. No credentials or token storage in v0.1. `BEAVER_CACHE_DIR` may override cache location in integration tests.

## 10. Package operations examples

```sh
beaver new app
cd app
beaver add mathkit@1.0.0 --registry-url https://packages.example.invalid
beaver fetch --locked
beaver run --locked
beaver remove mathkit

# local library workflow
beaver new mathkit --lib
beaver new consumer
cd consumer
beaver add --path ../mathkit
beaver check

# archive
cd ../mathkit
beaver package
```

Replace placeholder URLs with a real configured registry. During development use `tools/create_mock_registry.py` to create test artifacts and a loopback HTTP server.

## 11. Security and trust limitations

SHA-256 lockfile checking protects integrity/reproducibility after the first trusted resolution. It does **not** establish publisher identity, guarantee the registry is honest, or prove source code is non-malicious. Beaver 0.1 has no signature verification, account identity, package upload API, or sandbox for a user choosing to build and run a dependency. Disclose this clearly instead of claiming the registry is production-secure by default.
