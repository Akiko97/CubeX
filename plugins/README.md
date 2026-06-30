# Example Plugins

These plugins are normal Rust binaries using `cubex-plugin-sdk`, plus Wasm
plugins using `cubex-wasm-plugin-sdk`.

- `cubex-hello-plugin`: emits a greeting on `system.start`; arg 0 is an optional name.
- `cubex-echo-plugin`: echoes text and bytes messages.
- `cubex-file-source-plugin`: reads a file into text, bytes, record-key, or `key=value` record messages; `record-typed` also parses booleans and integers.
- `cubex-file-sink-plugin`: writes text or binary messages to a file; arg 0 is the output path.
- `cubex-print-plugin`: records text messages as host-visible logs.
- `cubex-record-source-plugin`: emits a structured record; args are optional key, message, and user.
- `cubex-record-store-plugin`: stores, loads, lists, and deletes structured records in a binary file; get/list/delete accept text or record payloads; arg 0 is the optional store path.
- `cubex-access-policy-plugin`: allows or denies records by user/topic rules.
- `cubex-blp-policy-plugin`: applies Bell-LaPadula read/write checks to labeled record payloads.
- `cubex-register-client-plugin`: emits a register write followed by a read; args are optional address and value.
- `cubex-register-bank-plugin`: keeps an in-memory 16-bit register map.
- `cubex-sha256-plugin`: hashes text or bytes with SHA-256.
- `cubex-tcp-client-plugin`: sends bytes to a TCP endpoint and emits the response; args are address, optional text, optional timeout milliseconds.
- `cubex-tcp-echo-plugin`: starts a tiny TCP echo endpoint for local network flows; args are optional bind address and max connections.
- `cubex-timer-plugin`: emits up to 1024 configured timer tick records; args are optional count, interval milliseconds, and topic.
- `cubex-wasm-hello-plugin`, `cubex-wasm-echo-plugin`, `cubex-wasm-print-plugin`: Wasm variants for basic text flows.
- `cubex-wasm-file-source-plugin`, `cubex-wasm-file-sink-plugin`: Wasm file source/sink plugins using file capabilities.
- `cubex-wasm-record-source-plugin`, `cubex-wasm-access-policy-plugin`: Wasm variants for record policy flows.
- `cubex-wasm-record-store-plugin`: Wasm durable record store using a record-store capability.
- `cubex-wasm-register-client-plugin`, `cubex-wasm-register-bank-plugin`: Wasm variants for in-memory register flows.
- `cubex-wasm-sha256-plugin`: Wasm SHA-256 plugin.
- `cubex-wasm-random-plugin`: Wasm random byte plugin using the host random ABI.
- `cubex-wasm-tcp-client-plugin`, `cubex-wasm-tcp-echo-plugin`: Wasm TCP client and echo plugins using TCP capabilities.
- `cubex-wasm-timer-plugin`: Wasm timer plugin using the timer capability.

Build them with:

```sh
cargo build --workspace
cargo build --target wasm32-unknown-unknown \
  -p cubex-wasm-access-policy-plugin \
  -p cubex-wasm-echo-plugin \
  -p cubex-wasm-file-sink-plugin \
  -p cubex-wasm-file-source-plugin \
  -p cubex-wasm-hello-plugin \
  -p cubex-wasm-print-plugin \
  -p cubex-wasm-random-plugin \
  -p cubex-wasm-record-source-plugin \
  -p cubex-wasm-record-store-plugin \
  -p cubex-wasm-register-bank-plugin \
  -p cubex-wasm-register-client-plugin \
  -p cubex-wasm-sha256-plugin \
  -p cubex-wasm-tcp-client-plugin \
  -p cubex-wasm-tcp-echo-plugin \
  -p cubex-wasm-timer-plugin
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
cargo run -p cubex-cli -- run --strict -c examples/blp/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/alice-bob/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/register-bank/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-hello/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-crypto/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-access-control/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-register-bank/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-random/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-file-flow/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-bytes-flow/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-network/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-timer/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/wasm-record-store/cubex.toml
```
