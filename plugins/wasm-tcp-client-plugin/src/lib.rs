use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, tcp_request,
};

#[derive(Default)]
struct WasmTcpClientPlugin;

impl Plugin for WasmTcpClientPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 3 {
            anyhow::bail!("tcp client accepts at most 3 args");
        }
        let addr = args
            .first()
            .ok_or_else(|| anyhow::anyhow!("tcp client needs address as arg 0"))?;
        validate_text(addr, "tcp client address")?;
        let text = args.get(1).cloned().unwrap_or_else(|| "ping".into());
        let timeout_ms = parse_timeout(args.get(2))?;
        let response = tcp_request(addr.clone(), text.into_bytes(), timeout_ms)?;
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "tcp.response",
                Payload::Text(String::from_utf8(response)?),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn parse_timeout(value: Option<&String>) -> anyhow::Result<u64> {
    let Some(value) = value else {
        return Ok(2000);
    };
    validate_text(value, "tcp client timeout")?;
    let millis: u64 = value
        .parse()
        .map_err(|_| anyhow::anyhow!("tcp client timeout must be an unsigned integer"))?;
    if millis == 0 {
        anyhow::bail!("tcp client timeout must be positive");
    }
    Ok(millis)
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

cubex_wasm_plugin_sdk::export_plugin!(WasmTcpClientPlugin);
