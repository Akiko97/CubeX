# CubeX Plugin Guide

CubeX plugins are separate processes that exchange length-prefixed binary frames with the host.
This keeps plugins isolated without loading arbitrary shared libraries into the runtime.

Implement a plugin by depending on `cubex-plugin-sdk`, implementing `Plugin`, and calling
`cubex_plugin_sdk::run_stdio`.

Minimal project:

```text
plugins/my-plugin/
  Cargo.toml
  src/main.rs
```

A runnable version of this shape lives in `examples/plugin-project`.

`plugins/my-plugin/Cargo.toml`:

```toml
[package]
name = "cubex-my-plugin"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
anyhow.workspace = true
cubex-plugin-sdk = { path = "../../crates/cubex-plugin-sdk" }
```

Add it to the workspace `Cargo.toml`:

```toml
[workspace]
members = [
    "plugins/my-plugin",
]
```

`plugins/my-plugin/src/main.rs`:

```rust
use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct MyPlugin;

impl Plugin for MyPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 1 {
            anyhow::bail!("my-plugin accepts at most 1 arg");
        }
        let text = args.first().cloned().unwrap_or_else(|| "ok".into());
        if text.trim().is_empty() {
            anyhow::bail!("my-plugin text must not be empty");
        }
        if text.trim() != text {
            anyhow::bail!("my-plugin text must not be padded");
        }

        Ok(PluginResponse {
            messages: vec![Message::new(request.plugin, "my.topic", Payload::Text(text))],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(MyPlugin)
}
```

Register it in `cubex.toml`:

```toml
[[plugins]]
name = "my-plugin"
command = "./target/debug/cubex-my-plugin"
args = ["hello from my plugin"]
autostart = true
```

Route its output:

```toml
[[plugins]]
name = "print"
command = "./target/debug/cubex-print-plugin"

[[routes]]
name = "my-plugin-to-print"
source = "my-plugin"
topic = "my.topic"
payload = "text"
to = ["print"]
```

Build and check:

```sh
cargo build --workspace
cargo run -p cubex-cli -- check -c cubex.toml
cargo run -p cubex-cli -- run --strict -c cubex.toml
```

Run the included plugin project example:

```sh
cargo run -p cubex-cli -- run --strict -c examples/plugin-project/cubex.toml
```

## Wasm plugins

Wasm plugins depend on `cubex-wasm-plugin-sdk`, build as `cdylib`, implement the
same `Plugin` shape, and export the ABI with `export_plugin!`.

`plugins/wasm-my-plugin/Cargo.toml`:

```toml
[package]
name = "cubex-wasm-my-plugin"
edition.workspace = true
license.workspace = true
version.workspace = true

[lib]
crate-type = ["cdylib"]

[dependencies]
anyhow.workspace = true
cubex-wasm-plugin-sdk = { path = "../../crates/cubex-wasm-plugin-sdk" }
```

`plugins/wasm-my-plugin/src/lib.rs`:

```rust
use cubex_wasm_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct WasmMyPlugin;

impl Plugin for WasmMyPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        Ok(PluginResponse {
            messages: vec![Message::new(request.plugin, "wasm.ready", Payload::Text("ok".into()))],
            logs: Vec::new(),
            error: None,
        })
    }
}

cubex_wasm_plugin_sdk::export_plugin!(WasmMyPlugin);
```

Build and run a wasm example:

```sh
cargo build --target wasm32-unknown-unknown -p cubex-wasm-hello-plugin -p cubex-wasm-echo-plugin -p cubex-wasm-print-plugin
cargo run -p cubex-cli -- run --strict -c examples/wasm-hello/cubex.toml
```

Register a wasm plugin with `wasm`; do not set `command` on the same plugin:

```toml
[[plugins]]
name = "wasm-my-plugin"
wasm = "./target/wasm32-unknown-unknown/debug/cubex_wasm_my_plugin.wasm"
```

Wasm plugins that need host IO use one imported ABI function:
`cubex.host_call(ptr, len) -> packed(ptr, len)`. The SDK wraps this with
helpers such as `read_file`, `write_file`, `tcp_request`, `tcp_echo`, `sleep_ms`,
and `record_*`. Each call is checked against the plugin's configured
capabilities before the host performs the operation.

Plugin `args` come from the `system.start` control message. The host does not pass
them as process argv. Keep stdout for binary protocol frames; return user-visible
logs through `PluginResponse.logs`. The host also sends `system.stop` during
shutdown and reports any logs from that response.
`system.stop` responses must not emit messages because shutdown messages cannot
be routed.
If `Plugin::handle` returns an error, the SDK sends that error back in the next
binary response and the host fails the run with the plugin name and error text.
When a plugin returns a manual `PluginResponse.error`, the SDK trims the error
text and drops any messages from that error response.
Plugin error text must be non-empty and must not have leading or trailing whitespace.
Error responses must not emit messages.
The runtime stamps emitted message `source` values with the configured plugin
name, so routing identity comes from the host configuration. Emitted message
IDs must not be nil. Emitted message topics must be non-empty and must not have
leading or trailing whitespace.
Plugins must not emit `Payload::Control`; `system.start` and `system.stop` are
host-owned control messages.
