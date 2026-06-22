use cubex_wasm_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Default)]
struct WasmSha256Plugin;

impl Plugin for WasmSha256Plugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let bytes = match request.message.payload {
            Payload::Text(text) => text.into_bytes(),
            Payload::Bytes(bytes) => bytes,
            _ => return Ok(PluginResponse::default()),
        };
        let digest = Sha256::digest(&bytes);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let fields = BTreeMap::from([
            ("algorithm".into(), Value::String("sha256".into())),
            ("hex".into(), Value::String(hex)),
            ("size".into(), Value::U64(bytes.len() as u64)),
        ]);
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "crypto.sha256",
                Payload::Record(fields),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

cubex_wasm_plugin_sdk::export_plugin!(WasmSha256Plugin);
