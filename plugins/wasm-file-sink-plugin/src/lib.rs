use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, write_file,
};

#[derive(Default)]
struct WasmFileSinkPlugin {
    output: Option<String>,
}

impl Plugin for WasmFileSinkPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        match request.message.payload {
            Payload::Control(Control::Start { args }) => {
                if args.len() > 1 {
                    anyhow::bail!("file sink accepts at most 1 arg");
                }
                let output = args
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("file sink needs output path arg 0"))?;
                validate_text(output, "file sink output path")?;
                self.output = Some(output.clone());
                Ok(PluginResponse::default())
            }
            Payload::Text(text) => self.write(request.plugin, text.into_bytes()),
            Payload::Bytes(bytes) => self.write(request.plugin, bytes),
            _ => Ok(PluginResponse::default()),
        }
    }
}

impl WasmFileSinkPlugin {
    fn write(&self, plugin: String, bytes: Vec<u8>) -> anyhow::Result<PluginResponse> {
        let output = self
            .output
            .clone()
            .ok_or_else(|| anyhow::anyhow!("file sink was not started"))?;
        write_file(output.clone(), bytes)?;
        Ok(PluginResponse {
            messages: vec![Message::new(plugin, "file.written", Payload::Text(output))],
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

cubex_wasm_plugin_sdk::export_plugin!(WasmFileSinkPlugin::default());
