use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value,
};
use std::collections::BTreeMap;

#[derive(Default)]
struct WasmRegisterClientPlugin;

impl Plugin for WasmRegisterClientPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 2 {
            anyhow::bail!("register client accepts at most 2 args");
        }
        let address = parse_u16(args.first(), 7)?;
        let value = parse_u16(args.get(1), 42)?;
        Ok(PluginResponse {
            messages: vec![
                Message::new(
                    request.plugin.clone(),
                    "register.write",
                    register(address, value),
                ),
                Message::new(request.plugin, "register.read", read_register(address)),
            ],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn parse_u16(value: Option<&String>, fallback: u16) -> anyhow::Result<u16> {
    let Some(value) = value else {
        return Ok(fallback);
    };
    if value.trim().is_empty() || value.trim() != value {
        anyhow::bail!("register client numeric args must not be empty or padded");
    }
    value.parse().map_err(|_| {
        anyhow::anyhow!("register client numeric args must be unsigned 16-bit integers")
    })
}

fn register(address: u16, value: u16) -> Payload {
    Payload::Record(BTreeMap::from([
        ("address".into(), Value::U64(address.into())),
        ("value".into(), Value::U64(value.into())),
    ]))
}

fn read_register(address: u16) -> Payload {
    Payload::Record(BTreeMap::from([(
        "address".into(),
        Value::U64(address.into()),
    )]))
}

cubex_wasm_plugin_sdk::export_plugin!(WasmRegisterClientPlugin);
