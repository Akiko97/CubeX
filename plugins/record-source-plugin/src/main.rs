use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::BTreeMap;

#[derive(Default)]
struct RecordSourcePlugin;

impl Plugin for RecordSourcePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };

        if args.len() > 3 {
            anyhow::bail!("record source accepts at most 3 args");
        }
        let key = args.first().cloned().unwrap_or_else(|| "sample".into());
        validate_key(&key)?;
        let value = args.get(1).cloned().unwrap_or_else(|| "ready".into());
        let mut fields = BTreeMap::new();
        fields.insert("key".into(), Value::String(key));
        fields.insert("message".into(), Value::String(value));
        if let Some(user) = args.get(2) {
            validate_user(user)?;
            fields.insert("user".into(), Value::String(user.clone()));
        }
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "record.put",
                Payload::Record(fields),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn validate_key(key: &str) -> anyhow::Result<()> {
    if key.trim().is_empty() {
        anyhow::bail!("record source key must not be empty");
    }
    if key.trim() != key {
        anyhow::bail!("record source key must not be padded");
    }
    Ok(())
}

fn validate_user(user: &str) -> anyhow::Result<()> {
    if user.trim().is_empty() {
        anyhow::bail!("record source user must not be empty");
    }
    if user.trim() != user {
        anyhow::bail!("record source user must not be padded");
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(RecordSourcePlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_default_record() {
        let response = RecordSourcePlugin
            .handle(PluginRequest {
                plugin: "record-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start { args: Vec::new() }),
                ),
            })
            .unwrap();

        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(fields["key"], Value::String("sample".into()));
    }

    #[test]
    fn emits_configured_record_with_user() {
        let response = RecordSourcePlugin
            .handle(PluginRequest {
                plugin: "record-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["demo".into(), "ready".into(), "alice".into()],
                    }),
                ),
            })
            .unwrap();

        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(response.messages[0].topic, "record.put");
        assert_eq!(fields["key"], Value::String("demo".into()));
        assert_eq!(fields["message"], Value::String("ready".into()));
        assert_eq!(fields["user"], Value::String("alice".into()));
    }

    #[test]
    fn rejects_extra_args() {
        let error = RecordSourcePlugin
            .handle(PluginRequest {
                plugin: "record-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            "key".into(),
                            "value".into(),
                            "alice".into(),
                            "ignored".into(),
                        ],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "record source accepts at most 3 args");
    }

    #[test]
    fn rejects_invalid_keys() {
        for (key, message) in [
            ("", "record source key must not be empty"),
            (" demo", "record source key must not be padded"),
        ] {
            let error = RecordSourcePlugin
                .handle(PluginRequest {
                    plugin: "record-source".into(),
                    message: Message::new(
                        "engine",
                        "system.start",
                        Payload::Control(Control::Start {
                            args: vec![key.into()],
                        }),
                    ),
                })
                .unwrap_err()
                .to_string();

            assert_eq!(error, message);
        }
    }

    #[test]
    fn rejects_invalid_users() {
        for (user, message) in [
            ("", "record source user must not be empty"),
            (" alice", "record source user must not be padded"),
        ] {
            let error = RecordSourcePlugin
                .handle(PluginRequest {
                    plugin: "record-source".into(),
                    message: Message::new(
                        "engine",
                        "system.start",
                        Payload::Control(Control::Start {
                            args: vec!["key".into(), "value".into(), user.into()],
                        }),
                    ),
                })
                .unwrap_err()
                .to_string();

            assert_eq!(error, message);
        }
    }
}
