# CubeX

CubeX is a Rust workspace for a message-driven runtime with a strongly
functional `.cx` strategy language, binary message frames, route-based dispatch,
process plugins, and Wasm sandbox plugins.

Strategy files are compiled to the same typed runtime configuration used by the
host. TOML configuration is still supported as the low-level format and as a
compiler output target.

Run the full local check:

```sh
bash scripts/smoke.sh
```

Run the strategy hello example:

```sh
cargo build --workspace
cargo run -p cubex-cli -- check -c examples/hello/cubex.cx
cargo run -p cubex-cli -- check --strict -c examples/hello/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/hello/cubex.cx
```

Inspect the TOML generated from a strategy:

```sh
cargo run -p cubex-cli -- compile examples/hello/cubex.cx
cargo run -p cubex-cli -- compile examples/hello/cubex.cx -o generated.toml
```

Run the Wasm hello example:

```sh
cargo build --target wasm32-unknown-unknown -p cubex-wasm-hello-plugin -p cubex-wasm-echo-plugin -p cubex-wasm-print-plugin
cargo run -p cubex-cli -- run --strict -c examples/wasm-hello/cubex.cx
```

Run the record store example:

```sh
cargo run -p cubex-cli -- run --strict -c examples/record-store/cubex.cx
cargo run -p cubex-cli -- events examples/record-store/events.bin
cargo run -p cubex-cli -- records examples/record-store/records.bin
cargo run -p cubex-cli -- run --strict -c examples/record-store/get.cx
cargo run -p cubex-cli -- run --strict -c examples/record-store/list.cx
cargo run -p cubex-cli -- run --strict -c examples/record-store/delete.cx
cargo run -p cubex-cli -- run --strict -c examples/record-store/replay.cx
```

Run file and timer examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/file-flow/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/bytes-flow/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/timer/cubex.cx
```

Run network and crypto examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/network/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/crypto/cubex.cx
```

Run the standalone plugin project example:

```sh
cargo run -p cubex-cli -- run --strict -c examples/plugin-project/cubex.cx
```

Run access-control and register examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/access-control/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/record-route/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/register-bank/cubex.cx
```

Run BLP and Alice/Bob examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/blp/cubex.cx
cargo run -p cubex-cli -- run --strict -c examples/alice-bob/cubex.cx
```

Run an existing TOML config directly when needed:

```sh
cargo run -p cubex-cli -- run --strict -c examples/hello/cubex.toml
```

Read more:

- [Architecture](docs/architecture.md)
- [Configuration](docs/configuration.md)
- [Strategy language](docs/strategy-language.md)
- [Plugin guide](docs/plugin-guide.md)

## Disclaimer

This project is a clumsy imitation of
[cubeos-1.5](https://gitee.com/biparadox/cubeos-1.5), a long-running work built
over many years. CubeX only borrows a few surface ideas and tries to reinterpret
them in Rust; it does not claim to reproduce the depth, history, or hard-won
engineering experience of the original system. Any rough edges, omissions, or
misunderstandings here are entirely mine.
