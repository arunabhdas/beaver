# Beaver Examples

These `.bv` programs specify behavior; they are not currently runnable without a compiler.

- `hello/`: no dependencies. `beaver run` must print `Hello, Beaver!` followed by a newline.
- `mathkit/`: example public Beaver library to publish into the mock static registry. Its `hidden_double` must not be visible to consumer packages.
- `consumer/`: adds exact remote `mathkit@1.0.0` and prints `49` after mutating the Point through an exclusive borrow.

After writing the compiler, create `localconsumer/` in an integration temp directory by replacing `[dependencies] mathkit = { version = "1.0.0" }` with `mathkit = { path = "../mathkit" }` and verify it builds without the network.

Use `tools/create_mock_registry.py` followed by a local HTTP server to exercise remote download. See CLI protocol documentation.
