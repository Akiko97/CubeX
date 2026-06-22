# CubeX Configuration

An instance is described by one TOML file.

```toml
[engine]
name = "hello-example"
max_messages = 32

[[plugins]]
name = "hello"
command = "../../target/debug/cubex-hello-plugin"
working_dir = "."
args = ["CubeX"]
autostart = true

[[plugins]]
name = "print"
command = "../../target/debug/cubex-print-plugin"

[[plugins]]
name = "wasm-print"
wasm = "../../target/wasm32-unknown-unknown/debug/cubex_wasm_print_plugin.wasm"

[[routes]]
name = "greeting-to-print"
source = "hello"
topic = "hello.greeting"
payload = "text"
to = ["print"]

[[routes]]
name = "alice-records-to-print"
topic = "record.put"
payload = "record"
record = { user = "alice", priority = 7, active = true }
to = ["print"]
```

Plugins use either `command` for a process plugin or `wasm` for a WebAssembly
plugin. The fields are mutually exclusive. Both paths are resolved relative to
the config file. This keeps instances portable and avoids process-wide
environment setup.

`engine.name`, plugin names, route names, plugin commands, and plugin wasm paths
must be non-empty. Plugin commands and wasm paths must not be only whitespace.
String identifiers such as engine, plugin, route, source, topic, and target
names must not be only whitespace or have leading/trailing whitespace.
Plugin names must not reuse `engine.name`.
Plugin and route names must be unique.
`engine.max_messages` must be positive.

`working_dir` is optional. When present, it must be non-empty and non-blank. It
is also resolved relative to the config file and becomes the child plugin process
working directory.

`[store]` is optional. Setting non-empty, non-blank `store.path` enables an
append-only binary message log for emitted messages. `replay_on_start = true`
requires `store.path` and requeues stored messages at startup without appending
them again. Replayed messages must have non-nil IDs, non-empty unpadded `source`
and `topic` fields, must not use host-owned control payloads, and must come from
the engine or a configured plugin.

Route fields are optional except `name` and `to`. Omitting a match field makes it
a wildcard for that field. Route targets must be non-empty and unique within the
route. When `topic` is present, it must be non-empty. When `source` is present,
it must name a configured plugin or the engine. `record` is an optional exact
match for record payload fields; supported config values are strings, signed
integers, and booleans. When `record` is present, `payload` must be omitted or
set to `record`. Record match keys must not be empty or padded.

Plugins receive their `args` in the `system.start` control message. Autostart
plugins receive it at startup; lazy plugins receive it before their first routed
message. The host does not pass args as process argv.

Autostart plugins run in the same order they appear in the configuration. This
matters for flows such as starting a local TCP endpoint before a client connects.
It also lets policy-style plugins load rules before source plugins emit records.

Validate a config without starting plugins:

```sh
cargo run -p cubex-cli -- check -c examples/hello/cubex.toml
cargo run -p cubex-cli -- check --strict -c examples/hello/cubex.toml
```

Use `--strict` after building plugins to also verify configured plugin commands
exist and are executable, configured wasm files exist, any `working_dir` entries
are directories, and `store.path` does not point at a directory or through a file parent. If
`store.path` already exists, strict checks also verify that it is a readable
CubeX event log.
`run --strict` applies the same file checks before starting plugins.
