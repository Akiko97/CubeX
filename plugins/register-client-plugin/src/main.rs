use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct RegisterClientPlugin;

impl Plugin for RegisterClientPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };

        if args.len() > 2 {
            anyhow::bail!("register client accepts at most 2 args");
        }
        let address = parse_u16(args.first(), 7)?;
        let value = parse_u16(args.get(1), 42)?;

        let write = Message::new(
            request.plugin.clone(),
            "register.write",
            Payload::Record(record([("address", address), ("value", value)])),
        );
        let read = Message::new(
            request.plugin,
            "register.read",
            Payload::Record(record([("address", address)])),
        );

        Ok(PluginResponse {
            messages: vec![write, read],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn parse_u16(input: Option<&String>, fallback: u16) -> anyhow::Result<u16> {
    let Some(value) = input else {
        return Ok(fallback);
    };
    if value.trim().is_empty() || value.trim() != value {
        anyhow::bail!("register client numeric args must not be empty or padded");
    }
    value.parse().map_err(|_| {
        anyhow::anyhow!("register client numeric args must be unsigned 16-bit integers")
    })
}

fn record<const N: usize>(fields: [(&str, u16); N]) -> BTreeMap<String, Value> {
    fields
        .into_iter()
        .map(|(key, value)| (key.to_string(), Value::U64(value.into())))
        .collect()
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(RegisterClientPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_default_write_and_read() {
        let response = RegisterClientPlugin
            .handle(PluginRequest {
                plugin: "register-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start { args: Vec::new() }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "register.write");
        assert_eq!(response.messages[1].topic, "register.read");
    }

    #[test]
    fn emits_configured_register_payloads() {
        let response = RegisterClientPlugin
            .handle(PluginRequest {
                plugin: "register-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["12".into(), "34".into()],
                    }),
                ),
            })
            .unwrap();

        let Payload::Record(write_fields) = &response.messages[0].payload else {
            panic!("expected write record");
        };
        let Payload::Record(read_fields) = &response.messages[1].payload else {
            panic!("expected read record");
        };
        assert_eq!(write_fields["address"], Value::U64(12));
        assert_eq!(write_fields["value"], Value::U64(34));
        assert_eq!(read_fields["address"], Value::U64(12));
    }

    #[test]
    fn rejects_extra_args() {
        let error = RegisterClientPlugin
            .handle(PluginRequest {
                plugin: "register-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["12".into(), "34".into(), "ignored".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "register client accepts at most 2 args");
    }

    #[test]
    fn rejects_blank_numeric_args() {
        let error = RegisterClientPlugin
            .handle(PluginRequest {
                plugin: "register-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![" ".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "register client numeric args must not be empty or padded"
        );
    }

    #[test]
    fn rejects_invalid_numeric_args() {
        let error = RegisterClientPlugin
            .handle(PluginRequest {
                plugin: "register-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["70000".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            "register client numeric args must be unsigned 16-bit integers"
        );
    }
}
