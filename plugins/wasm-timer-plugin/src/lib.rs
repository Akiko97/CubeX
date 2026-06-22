use cubex_wasm_plugin_sdk::{
    Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value, sleep_ms,
};
use std::collections::BTreeMap;

const MAX_TICKS: u64 = 1024;

#[derive(Default)]
struct WasmTimerPlugin;

impl Plugin for WasmTimerPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 3 {
            anyhow::bail!("timer accepts at most 3 args");
        }
        let count = parse_arg(&args, 0, 1)?;
        if count == 0 {
            anyhow::bail!("timer count must be positive");
        }
        if count > MAX_TICKS {
            anyhow::bail!("timer count must be at most {MAX_TICKS}");
        }
        let interval_ms = parse_arg(&args, 1, 0)?;
        let topic = args.get(2).cloned().unwrap_or_else(|| "timer.tick".into());
        validate_text(&topic, "timer topic")?;

        let mut messages = Vec::new();
        for index in 0..count {
            if interval_ms > 0 && index > 0 {
                sleep_ms(interval_ms)?;
            }
            messages.push(Message::new(
                request.plugin.clone(),
                topic.clone(),
                Payload::Record(BTreeMap::from([
                    ("index".into(), Value::U64(index)),
                    ("count".into(), Value::U64(count)),
                ])),
            ));
        }
        Ok(PluginResponse {
            messages,
            logs: Vec::new(),
            error: None,
        })
    }
}

fn parse_arg(args: &[String], index: usize, default: u64) -> anyhow::Result<u64> {
    let Some(value) = args.get(index) else {
        return Ok(default);
    };
    validate_text(value, "timer numeric args")?;
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("timer numeric args must be unsigned integers"))
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

cubex_wasm_plugin_sdk::export_plugin!(WasmTimerPlugin);
