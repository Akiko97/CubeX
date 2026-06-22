# CubeX

CubeX is a Rust workspace for a message-driven runtime with TOML configuration,
binary message frames, route-based dispatch, and isolated process plugins.

Run the full local check:

```sh
bash scripts/smoke.sh
```

Run the hello example:

```sh
cargo build --workspace
cargo run -p cubex-cli -- check -c examples/hello/cubex.toml
cargo run -p cubex-cli -- check --strict -c examples/hello/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/hello/cubex.toml
```

Run the record store example:

```sh
cargo run -p cubex-cli -- run --strict -c examples/record-store/cubex.toml
cargo run -p cubex-cli -- events examples/record-store/events.bin
cargo run -p cubex-cli -- records examples/record-store/records.bin
cargo run -p cubex-cli -- run --strict -c examples/record-store/get.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/list.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/delete.toml
cargo run -p cubex-cli -- run --strict -c examples/record-store/replay.toml
```

Run file and timer examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/file-flow/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/bytes-flow/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/timer/cubex.toml
```

Run network and crypto examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/network/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/crypto/cubex.toml
```

Run the standalone plugin project example:

```sh
cargo run -p cubex-cli -- run --strict -c examples/plugin-project/cubex.toml
```

Run access-control and register examples:

```sh
cargo run -p cubex-cli -- run --strict -c examples/access-control/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/record-route/cubex.toml
cargo run -p cubex-cli -- run --strict -c examples/register-bank/cubex.toml
```

Read more:

- [Architecture](docs/architecture.md)
- [Configuration](docs/configuration.md)
- [Plugin guide](docs/plugin-guide.md)

## Disclaimer

This project is a clumsy imitation of
[cubeos-1.5](https://gitee.com/biparadox/cubeos-1.5), a long-running work built
over many years. CubeX only borrows a few surface ideas and tries to reinterpret
them in Rust; it does not claim to reproduce the depth, history, or hard-won
engineering experience of the original system. Any rough edges, omissions, or
misunderstandings here are entirely mine.
