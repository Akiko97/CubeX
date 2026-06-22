use cubex_wasm_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct WasmEchoPlugin;

impl Plugin for WasmEchoPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let payload = match request.message.payload {
            Payload::Text(text) => Payload::Text(text),
            Payload::Bytes(bytes) => Payload::Bytes(bytes),
            _ => return Ok(PluginResponse::default()),
        };
        Ok(PluginResponse {
            messages: vec![Message::new(request.plugin, "echo.reply", payload)],
            logs: Vec::new(),
            error: None,
        })
    }
}

cubex_wasm_plugin_sdk::export_plugin!(WasmEchoPlugin);
