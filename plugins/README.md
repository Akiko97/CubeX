# Example Plugins

These plugins are normal Rust binaries using `cubex-plugin-sdk`.

- `cubex-hello-plugin`: emits a greeting on `system.start`; arg 0 is an optional name.
- `cubex-echo-plugin`: echoes text and bytes messages.
- `cubex-file-source-plugin`: reads a file into text, bytes, record-key, or `key=value` record messages; `record-typed` also parses booleans and integers.
- `cubex-file-sink-plugin`: writes text or binary messages to a file; arg 0 is the output path.
- `cubex-print-plugin`: records text messages as host-visible logs.
- `cubex-record-source-plugin`: emits a structured record; args are optional key, message, and user.
- `cubex-record-store-plugin`: stores, loads, lists, and deletes structured records in a binary file; get/list/delete accept text or record payloads; arg 0 is the optional store path.
- `cubex-access-policy-plugin`: allows or denies records by user/topic rules.
- `cubex-register-client-plugin`: emits a register write followed by a read; args are optional address and value.
- `cubex-register-bank-plugin`: keeps an in-memory 16-bit register map.
- `cubex-sha256-plugin`: hashes text or bytes with SHA-256.
- `cubex-tcp-client-plugin`: sends bytes to a TCP endpoint and emits the response; args are address, optional text, optional timeout milliseconds.
- `cubex-tcp-echo-plugin`: starts a tiny TCP echo endpoint for local network flows; args are optional bind address and max connections.
- `cubex-timer-plugin`: emits up to 1024 configured timer tick records; args are optional count, interval milliseconds, and topic.

Build them with:

```sh
cargo build --workspace
```

Run all plugin examples with:

```sh
bash scripts/smoke.sh
```

Run the example instance with:

```sh
cargo run -p cubex-cli -- run --strict -c examples/hello/cubex.toml
```

Run the record store example with:

```sh
cargo run -p cubex-cli -- run --strict -c examples/record-store/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/get.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/list.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/delete.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/replay.toml
cargo run -p cubex-cli -- run --strict -c examples/file-flow/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/bytes-flow/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/timer/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/network/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/crypto/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/access-control/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/register-bank/cubex.toml
```
