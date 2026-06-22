use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value, record_delete,
    record_get, record_list, record_put,
};

#[derive(Default)]
struct WasmRecordStorePlugin {
    path: Option<String>,
}

impl Plugin for WasmRecordStorePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let message = request.message;
        if let Payload::Control(Control::Start { args }) = message.payload {
            if args.len() > 1 {
                anyhow::bail!("record store accepts at most 1 arg");
            }
            let path = args
                .first()
                .cloned()
                .unwrap_or_else(|| "cubex-records.bin".into());
            validate_text(&path, "record store path")?;
            self.path = Some(path);
            return Ok(PluginResponse::default());
        }

        match message.topic.as_str() {
            "record.put" if matches!(&message.payload, Payload::Record(_)) => {
                let key = record_key(&message)?.unwrap_or_else(|| message.id.to_string());
                record_put(self.path()?, key.clone(), message)?;
                Ok(text_response(request.plugin, "record.stored", key))
            }
            "record.get" if key_payload(&message.payload) => {
                let key = message_key(&message, "lookup")?;
                let response = match record_get(self.path()?, key.clone())? {
                    Some(message) => format!("record {key} found from {}", message.source),
                    None => format!("record {key} not found"),
                };
                Ok(text_response(request.plugin, "record.loaded", response))
            }
            "record.delete" if key_payload(&message.payload) => {
                let key = message_key(&message, "delete")?;
                let response = if record_delete(self.path()?, key.clone())? {
                    format!("record {key} deleted")
                } else {
                    format!("record {key} not found")
                };
                Ok(text_response(request.plugin, "record.deleted", response))
            }
            "record.list" if key_payload(&message.payload) => {
                let keys = record_list(self.path()?)?.join(", ");
                let response = if keys.is_empty() {
                    "records: <empty>".into()
                } else {
                    format!("records: {keys}")
                };
                Ok(text_response(request.plugin, "record.listed", response))
            }
            _ => Ok(PluginResponse::default()),
        }
    }
}

impl WasmRecordStorePlugin {
    fn path(&self) -> anyhow::Result<String> {
        self.path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("record store was not started"))
    }
}

fn text_response(plugin: String, topic: &str, text: String) -> PluginResponse {
    PluginResponse {
        messages: vec![Message::new(plugin, topic, Payload::Text(text))],
        logs: Vec::new(),
        error: None,
    }
}

fn key_payload(payload: &Payload) -> bool {
    matches!(payload, Payload::Text(_) | Payload::Record(_))
}

fn message_key(message: &Message, operation: &str) -> anyhow::Result<String> {
    match &message.payload {
        Payload::Text(key) => text_key(key, operation),
        Payload::Record(_) => record_key(message)?
            .ok_or_else(|| anyhow::anyhow!("record {operation} key is required")),
        _ => unreachable!("message_key is only called for key payloads"),
    }
}

fn text_key(key: &str, operation: &str) -> anyhow::Result<String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        anyhow::bail!("record {operation} key must not be empty");
    }
    Ok(key)
}

fn record_key(message: &Message) -> anyhow::Result<Option<String>> {
    let Payload::Record(fields) = &message.payload else {
        return Ok(None);
    };
    let Some(value) = fields.get("key") else {
        return Ok(None);
    };
    let Value::String(key) = value else {
        anyhow::bail!("record key must be a string");
    };
    validate_text(key, "record key")?;
    Ok(Some(key.clone()))
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

cubex_wasm_plugin_sdk::export_plugin!(WasmRecordStorePlugin::default());
