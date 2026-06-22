use cubex_wasm_plugin_sdk::{Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct WasmPrintPlugin;

impl Plugin for WasmPrintPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let text = match request.message.payload {
            Payload::Control(_) => return Ok(PluginResponse::default()),
            Payload::Text(text) => text,
            payload => format!("{payload:?}"),
        };
        Ok(PluginResponse {
            messages: Vec::new(),
            logs: vec![format!("{}: {text}", request.plugin)],
            error: None,
        })
    }
}

cubex_wasm_plugin_sdk::export_plugin!(WasmPrintPlugin);
