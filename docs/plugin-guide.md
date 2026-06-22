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
