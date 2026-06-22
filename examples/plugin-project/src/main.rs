use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct ExamplePlugin;

impl Plugin for ExamplePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if args.len() > 1 {
            anyhow::bail!("example plugin accepts at most 1 arg");
        }
        let text = args
            .first()
            .cloned()
            .unwrap_or_else(|| "example ready".into());
        if text.trim().is_empty() {
            anyhow::bail!("example plugin text must not be empty");
        }
        if text.trim() != text {
            anyhow::bail!("example plugin text must not be padded");
        }

        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "example.ready",
                Payload::Text(text),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(ExamplePlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_start_text() {
        let response = ExamplePlugin
            .handle(PluginRequest {
                plugin: "example".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["hello".into()],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "example.ready");
        assert_eq!(response.messages[0].payload, Payload::Text("hello".into()));
    }

    #[test]
    fn rejects_bad_args() {
        for (args, message) in [
            (
                vec!["hello".into(), "ignored".into()],
                "example plugin accepts at most 1 arg",
            ),
            (vec![String::new()], "example plugin text must not be empty"),
            (
                vec![" hello".into()],
                "example plugin text must not be padded",
            ),
        ] {
            let error = ExamplePlugin
                .handle(PluginRequest {
                    plugin: "example".into(),
                    message: Message::new(
                        "engine",
                        "system.start",
                        Payload::Control(Control::Start { args }),
                    ),
                })
                .unwrap_err()
                .to_string();

            assert_eq!(error, message);
        }
    }
}
