use cubex_wasm_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct WasmRegisterBankPlugin {
    registers: BTreeMap<u16, u16>,
}

impl Plugin for WasmRegisterBankPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Record(fields) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        match request.message.topic.as_str() {
            "register.write" => self.write(request.plugin, fields),
            "register.read" => self.read(request.plugin, fields),
            _ => Ok(PluginResponse::default()),
        }
    }
}

impl WasmRegisterBankPlugin {
    fn write(
        &mut self,
        source: String,
        fields: BTreeMap<String, Value>,
    ) -> anyhow::Result<PluginResponse> {
        let address = read_u16(&fields, "address")?;
        let value = read_u16(&fields, "value")?;
        self.registers.insert(address, value);
        Ok(PluginResponse {
            messages: vec![Message::new(
                source,
                "register.written",
                register(address, value),
            )],
            logs: Vec::new(),
            error: None,
        })
    }

    fn read(
        &self,
        source: String,
        fields: BTreeMap<String, Value>,
    ) -> anyhow::Result<PluginResponse> {
        let address = read_u16(&fields, "address")?;
        let value = self.registers.get(&address).copied().unwrap_or_default();
        Ok(PluginResponse {
            messages: vec![Message::new(
                source,
                "register.value",
                register(address, value),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn read_u16(fields: &BTreeMap<String, Value>, key: &str) -> anyhow::Result<u16> {
    match fields.get(key) {
        Some(Value::U64(value)) => u16::try_from(*value)
            .map_err(|_| anyhow::anyhow!("register field `{key}` must be unsigned 16-bit")),
        Some(Value::I64(value)) => u16::try_from(*value)
            .map_err(|_| anyhow::anyhow!("register field `{key}` must be unsigned 16-bit")),
        Some(_) => anyhow::bail!("register field `{key}` must be numeric"),
        None => anyhow::bail!("missing numeric field `{key}`"),
    }
}

fn register(address: u16, value: u16) -> Payload {
    Payload::Record(BTreeMap::from([
        ("address".into(), Value::U64(address.into())),
        ("value".into(), Value::U64(value.into())),
    ]))
}

cubex_wasm_plugin_sdk::export_plugin!(WasmRegisterBankPlugin::default());
