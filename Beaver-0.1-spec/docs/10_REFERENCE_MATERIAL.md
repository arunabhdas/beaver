# 10 - Non-Normative Reference Material

These external materials are implementation references, not Beaver specifications.

- Cargo: [Creating a new package](https://doc.rust-lang.org/stable/cargo/guide/creating-a-new-project.html) - generated project structure and executable workflow.
- Cargo: [Dependencies](https://doc.rust-lang.org/cargo/guide/dependencies.html) - manifest declarations and lockfile reasoning.
- Cargo: [Registry index specification](https://doc.rust-lang.org/cargo/reference/registry-index.html) - patterns for checksums, metadata and cache concerns. Beaver's registry protocol is intentionally simpler and NOT wire-compatible.
- [clap derive documentation](https://docs.rs/clap/latest/clap/_derive/) - Rust CLI subcommand parsing.
- [reqwest documentation](https://docs.rs/crate/reqwest/latest) - HTTPS client, streaming and TLS configuration.

Astra should use maintained stable-compatible library versions available at implementation time and commit the generated Rust `Cargo.lock`. This reference list does not require a particular HTTP client or Rust crate version.
