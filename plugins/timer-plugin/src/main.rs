use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::BTreeMap;
use std::time::Duration;

// ponytail: plugin response is one Vec; make this configurable if real timers need more.
const MAX_TICKS: u64 = 1024;

#[derive(Default)]
struct TimerPlugin;

impl Plugin for TimerPlugin {
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
        if topic.trim().is_empty() {
            anyhow::bail!("timer topic must not be empty");
        }
        if topic.trim() != topic {
            anyhow::bail!("timer topic must not be padded");
        }
        let mut messages = Vec::new();

        for index in 0..count {
            if interval_ms > 0 && index > 0 {
                std::thread::sleep(Duration::from_millis(interval_ms));
            }
            let mut fields = BTreeMap::new();
            fields.insert("index".into(), Value::U64(index));
            fields.insert("count".into(), Value::U64(count));
            messages.push(Message::new(
                request.plugin.clone(),
                topic.clone(),
                Payload::Record(fields),
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
    if value.trim().is_empty() || value.trim() != value {
        anyhow::bail!("timer numeric args must not be empty or padded");
    }
    value
        .parse()
        .map_err(|_| anyhow::anyhow!("timer numeric args must be unsigned integers"))
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(TimerPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_configured_ticks() {
        let response = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["2".into(), "0".into(), "timer.tick".into()],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages.len(), 2);
        assert_eq!(response.messages[0].topic, "timer.tick");
        let Payload::Record(fields) = &response.messages[1].payload else {
            panic!("expected record");
        };
        assert_eq!(fields["index"], Value::U64(1));
        assert_eq!(fields["count"], Value::U64(2));
    }

    #[test]
    fn rejects_zero_count() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["0".into(), "0".into(), "timer.tick".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer count must be positive");
    }

    #[test]
    fn rejects_extra_args() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            "1".into(),
                            "0".into(),
                            "timer.tick".into(),
                            "ignored".into(),
                        ],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer accepts at most 3 args");
    }

    #[test]
    fn rejects_excessive_count() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["1025".into(), "0".into(), "timer.tick".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer count must be at most 1024");
    }

    #[test]
    fn rejects_blank_numeric_args() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["1".into(), " 0".into(), "timer.tick".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer numeric args must not be empty or padded");
    }

    #[test]
    fn rejects_invalid_numeric_args() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["three".into(), "0".into(), "timer.tick".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer numeric args must be unsigned integers");
    }

    #[test]
    fn rejects_empty_topic() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["1".into(), "0".into(), " ".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer topic must not be empty");
    }

    #[test]
    fn rejects_padded_topic() {
        let error = TimerPlugin
            .handle(PluginRequest {
                plugin: "timer".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["1".into(), "0".into(), " timer.tick".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "timer topic must not be padded");
    }
}
