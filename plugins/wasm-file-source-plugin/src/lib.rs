use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value, read_file,
};
use std::collections::BTreeMap;

#[derive(Default)]
struct WasmFileSourcePlugin;

impl Plugin for WasmFileSourcePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 3 {
            anyhow::bail!("file source accepts at most 3 args");
        }
        let path = args
            .first()
            .ok_or_else(|| anyhow::anyhow!("file source needs input path as arg 0"))?;
        validate_text(path, "file source input path")?;
        let topic = args.get(1).cloned().unwrap_or_else(|| "file.read".into());
        validate_text(&topic, "file source topic")?;
        let mode = args.get(2).map(String::as_str).unwrap_or("text");
        let bytes = read_file(path.clone())?;
        let payload = match mode {
            "text" => Payload::Text(String::from_utf8(bytes)?),
            "bytes" => Payload::Bytes(bytes),
            "record-key" => Payload::Record(BTreeMap::from([(
                "key".into(),
                Value::String(read_key(bytes)?),
            )])),
            "record" => Payload::Record(read_record(bytes, false)?),
            "record-typed" => Payload::Record(read_record(bytes, true)?),
            other => anyhow::bail!(
                "file source mode must be `text`, `bytes`, `record-key`, `record`, or `record-typed`, got `{other}`"
            ),
        };
        Ok(PluginResponse {
            messages: vec![Message::new(request.plugin, topic, payload)],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn read_key(bytes: Vec<u8>) -> anyhow::Result<String> {
    let key = String::from_utf8(bytes)?.trim().to_string();
    if key.is_empty() {
        anyhow::bail!("file source record key must not be empty");
    }
    Ok(key)
}

fn read_record(bytes: Vec<u8>, typed: bool) -> anyhow::Result<BTreeMap<String, Value>> {
    let text = String::from_utf8(bytes)?;
    let mut record = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            anyhow::bail!("file source record line {} must use key=value", index + 1);
        };
        validate_text(key, "file source record key")?;
        if record
            .insert(key.into(), record_value(value, typed))
            .is_some()
        {
            anyhow::bail!("file source record key `{key}` is duplicated");
        }
    }
    if record.is_empty() {
        anyhow::bail!("file source record must not be empty");
    }
    Ok(record)
}

fn record_value(value: &str, typed: bool) -> Value {
    if !typed {
        return Value::String(value.into());
    }
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => value
            .parse::<i64>()
            .map(Value::I64)
            .unwrap_or_else(|_| Value::String(value.into())),
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

cubex_wasm_plugin_sdk::export_plugin!(WasmFileSourcePlugin);
