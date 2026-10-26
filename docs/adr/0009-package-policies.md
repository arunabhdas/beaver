# ADR 0009: Lockfile, registry and archive policies

## Choice
* **Lockfile in build commands.** `check`/`build`/`run`/`test`/`package` create `beaver.lock` when it
  is missing and dependencies exist. They refuse to *change* an existing lockfile (B6011, "out of
  date; run `beaver fetch`"). `add`/`remove`/`update`/`fetch` may rewrite it. `--locked` requires an
  existing, identical lockfile and never writes one. `--locked` is rejected by
  `add`/`remove`/`update`. Projects without dependencies get no lockfile.
* **Atomicity.** `add`/`remove`/`update` resolve and download the whole graph first. Only then is
  the lockfile written (temp file + rename), followed by the manifest. If the manifest write fails,
  the previous lockfile is restored. Any resolution error leaves both files untouched.
* **Registry selection:** `--registry-url`, then `BEAVER_REGISTRY_URL`, then
  `$XDG_CONFIG_HOME/beaver/config.toml` (default `~/.config/beaver/config.toml`), then the URL
  recorded in the lockfile. A locked URL is re-validated by the same policy. There is no default
  registry. One registry per graph.
* **URL policy:** `https://` only. `http://` only for the literal hosts `127.0.0.1` / `[::1]` and
  only with `BEAVER_ALLOW_INSECURE_LOCAL_REGISTRY=1` (`localhost` is not accepted). No credentials,
  queries or fragments. Redirects are never followed (B6022). Archive URLs are always built from the
  base URL; metadata with unknown fields (e.g. an `archive_url`) is rejected.
* **Offline.** With `--offline` no HTTP client is ever constructed. A lockfile entry plus a cached
  archive whose SHA-256 still matches is enough to build; cached metadata is used for unlocked
  resolution.
* **Cache integrity.** Archives are stored by SHA-256 and re-hashed every time they are used. A
  mismatching cache entry and its extracted sources are deleted and fetched again (or reported
  offline).
* **Archives.** Extraction accepts only regular files and directories at
  `<name>-<version>/{beaver.toml, README.md, LICENSE, src/**/*.bv, tests/*.bv}` with conservative
  file-name characters. It rejects absolute/drive/UNC paths, `..`, `.`, empty components,
  backslashes, NUL, symlinks, hard links, device files, duplicate or case-colliding paths, files over
  10 MiB, over 100 MiB in total, over 8192 entries, and compressed archives over 20 MiB. Every check
  runs before a file is written, and files are written only into a fresh private staging directory
  that is renamed into place once the whole archive validates. `beaver package` writes canonical
  archives (sorted, ustar, uid/gid/mtime 0, mode 0644, gzip mtime 0) and re-validates them with the
  extractor.
* **Path dependencies** are recorded as `path+<relative path>` without a checksum and are not
  content-pinned. Their manifests are re-read on every build.

## Tests
`crates/beaver-cli/tests/registry.rs`, `workflow.rs::package_archives_are_deterministic_and_validated`,
`crates/beaver-package` unit tests.

## Limitations
SHA-256 pins artifacts after the first trusted resolution. It does not authenticate publishers, and
it does not make dependency code trustworthy. Beaver 0.1 has no signatures and no sandbox.
