use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value,
};
use std::collections::BTreeMap;

#[derive(Default)]
struct WasmRecordSourcePlugin;

impl Plugin for WasmRecordSourcePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 3 {
            anyhow::bail!("record source accepts at most 3 args");
        }
        let key = args.first().cloned().unwrap_or_else(|| "sample".into());
        validate_text(&key, "record source key")?;
        let value = args.get(1).cloned().unwrap_or_else(|| "ready".into());
        let mut fields = BTreeMap::new();
        fields.insert("key".into(), Value::String(key));
        fields.insert("message".into(), Value::String(value));
        if let Some(user) = args.get(2) {
            validate_text(user, "record source user")?;
            fields.insert("user".into(), Value::String(user.clone()));
        }
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "record.put",
                Payload::Record(fields),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn validate_text(value: &str, label: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{label} must not be empty");
    }
    if value.trim() != value {
        anyhow::bail!("{label} must not be padded");
    }
    Ok(())
}

cubex_wasm_plugin_sdk::export_plugin!(WasmRecordSourcePlugin);
