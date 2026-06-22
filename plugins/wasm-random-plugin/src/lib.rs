use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, random_bytes,
};

#[derive(Default)]
struct WasmRandomPlugin;

impl Plugin for WasmRandomPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 2 {
            anyhow::bail!("random accepts at most 2 args");
        }
        let len = parse_len(args.first())?;
        let topic = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "random.bytes".into());
        validate_text(&topic, "random topic")?;
        let bytes = random_bytes(len)?;

        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                topic,
                Payload::Text(format!("random bytes: {}", hex(&bytes))),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_len(value: Option<&String>) -> anyhow::Result<u32> {
    let Some(value) = value else {
        return Ok(16);
    };
    validate_text(value, "random length")?;
    let len = value
        .parse()
        .map_err(|_| anyhow::anyhow!("random length must be an unsigned integer"))?;
    if len == 0 {
        anyhow::bail!("random length must be positive");
    }
    Ok(len)
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

cubex_wasm_plugin_sdk::export_plugin!(WasmRandomPlugin);
