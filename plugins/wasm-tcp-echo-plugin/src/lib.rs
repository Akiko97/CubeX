use cubex_wasm_plugin_sdk::{Control, Payload, Plugin, PluginRequest, PluginResponse, tcp_echo};

#[derive(Default)]
struct WasmTcpEchoPlugin {
    started: bool,
}

impl Plugin for WasmTcpEchoPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if self.started {
            return Ok(PluginResponse::default());
        }
        if args.len() > 2 {
            anyhow::bail!("tcp echo accepts at most 2 args");
        }
        let addr = args
            .first()
            .cloned()
            .unwrap_or_else(|| "127.0.0.1:4010".into());
        validate_text(&addr, "tcp echo address")?;
        let max_connections = parse_max_connections(args.get(1))?;
        let local_addr = tcp_echo(addr, max_connections)?;
        self.started = true;
        Ok(PluginResponse {
            messages: Vec::new(),
            logs: vec![format!("tcp echo listening on {local_addr}")],
            error: None,
        })
    }
}

fn parse_max_connections(value: Option<&String>) -> anyhow::Result<u64> {
    let Some(value) = value else {
        return Ok(1);
    };
    validate_text(value, "tcp echo max connections")?;
    let max_connections: u64 = value
        .parse()
        .map_err(|_| anyhow::anyhow!("tcp echo max connections must be an unsigned integer"))?;
    if max_connections == 0 {
        anyhow::bail!("tcp echo max connections must be positive");
    }
    Ok(max_connections)
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

cubex_wasm_plugin_sdk::export_plugin!(WasmTcpEchoPlugin::default());
