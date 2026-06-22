use cubex_wasm_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct WasmHelloPlugin;

impl Plugin for WasmHelloPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 1 {
            anyhow::bail!("hello accepts at most 1 arg");
        }
        let name = args.first().cloned().unwrap_or_else(|| "world".into());
        if name.trim().is_empty() {
            anyhow::bail!("hello name must not be empty");
        }
        if name.trim() != name {
            anyhow::bail!("hello name must not be padded");
        }
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "hello.greeting",
                Payload::Text(format!("Hello, {name}!")),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

cubex_wasm_plugin_sdk::export_plugin!(WasmHelloPlugin);
