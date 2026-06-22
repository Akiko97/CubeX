use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct HelloPlugin;

impl Plugin for HelloPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };

        if args.len() > 1 {
            anyhow::bail!("hello accepts at most 1 arg");
        }
        let name = args.first().cloned().unwrap_or_else(|| "world".into());
        if name.trim().is_empty() {
            anyhow::bail!("hello name must not be empty");
        }
        if name.trim() != name {
            anyhow::bail!("hello name must not be padded");
        }
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "hello.greeting",
                Payload::Text(format!("Hello, {name}!")),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(HelloPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_default_greeting() {
        let response = HelloPlugin
            .handle(PluginRequest {
                plugin: "hello".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start { args: Vec::new() }),
                ),
            })
            .unwrap();

        assert_eq!(
            response.messages[0].payload,
            Payload::Text("Hello, world!".into())
        );
    }

    #[test]
    fn rejects_extra_args() {
        let error = HelloPlugin
            .handle(PluginRequest {
                plugin: "hello".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["CubeX".into(), "ignored".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "hello accepts at most 1 arg");
    }

    #[test]
    fn rejects_invalid_names() {
        for (name, message) in [
            ("", "hello name must not be empty"),
            (" CubeX", "hello name must not be padded"),
        ] {
            let error = HelloPlugin
                .handle(PluginRequest {
                    plugin: "hello".into(),
                    message: Message::new(
                        "engine",
                        "system.start",
                        Payload::Control(Control::Start {
                            args: vec![name.into()],
                        }),
                    ),
                })
                .unwrap_err()
                .to_string();

            assert_eq!(error, message);
        }
    }
}
