use cubex_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct RegisterBankPlugin {
    registers: BTreeMap<u16, u16>,
}

impl Plugin for RegisterBankPlugin {
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

impl RegisterBankPlugin {
    fn write(
        &mut self,
        source: String,
        fields: BTreeMap<String, Value>,
    ) -> anyhow::Result<PluginResponse> {
        let address = read_u16(&fields, "address")?;
        let value = read_u16(&fields, "value")?;
        self.registers.insert(address, value);

        let mut record = BTreeMap::new();
        record.insert("address".into(), Value::U64(address.into()));
        record.insert("value".into(), Value::U64(value.into()));
        Ok(PluginResponse {
            messages: vec![Message::new(
                source,
                "register.written",
                Payload::Record(record),
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

        let mut record = BTreeMap::new();
        record.insert("address".into(), Value::U64(address.into()));
        record.insert("value".into(), Value::U64(value.into()));
        Ok(PluginResponse {
            messages: vec![Message::new(
                source,
                "register.value",
                Payload::Record(record),
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

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(RegisterBankPlugin::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_written_register_value() {
        let mut plugin = RegisterBankPlugin::default();
        let response = plugin
            .write("bank".into(), record([("address", 7), ("value", 42)]))
            .unwrap();
        assert_eq!(response.messages[0].topic, "register.written");

        let response = plugin
            .read("bank".into(), record([("address", 7)]))
            .unwrap();
        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(fields["value"], Value::U64(42));
    }

    #[test]
    fn reads_unwritten_register_as_zero() {
        let plugin = RegisterBankPlugin::default();
        let response = plugin
            .read("bank".into(), record([("address", 9)]))
            .unwrap();
        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };

        assert_eq!(response.messages[0].topic, "register.value");
        assert_eq!(fields["address"], Value::U64(9));
        assert_eq!(fields["value"], Value::U64(0));
    }

    #[test]
    fn rejects_non_numeric_register_fields() {
        let fields = BTreeMap::from([("address".into(), Value::String("7".into()))]);

        let error = read_u16(&fields, "address").unwrap_err().to_string();

        assert_eq!(error, "register field `address` must be numeric");
    }

    #[test]
    fn rejects_out_of_range_register_fields() {
        for value in [Value::U64(70_000), Value::I64(-1)] {
            let fields = BTreeMap::from([("address".into(), value)]);
            let error = read_u16(&fields, "address").unwrap_err().to_string();

            assert_eq!(error, "register field `address` must be unsigned 16-bit");
        }
    }

    fn record<const N: usize>(fields: [(&str, u16); N]) -> BTreeMap<String, Value> {
        fields
            .into_iter()
            .map(|(key, value)| (key.to_string(), Value::U64(value.into())))
            .collect()
    }
}
