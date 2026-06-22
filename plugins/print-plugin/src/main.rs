use cubex_plugin_sdk::{Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct PrintPlugin;

impl Plugin for PrintPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let text = match request.message.payload {
            Payload::Control(_) => return Ok(PluginResponse::default()),
            Payload::Text(text) => text,
            other => format!("{other:?}"),
        };

        Ok(PluginResponse {
            messages: Vec::new(),
            logs: vec![format!("{}: {text}", request.plugin)],
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(PrintPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubex_plugin_sdk::{Control, Message};

    #[test]
    fn records_text_as_log() {
        let response = PrintPlugin
            .handle(PluginRequest {
                plugin: "print".into(),
                message: Message::new("source", "topic", Payload::Text("hello".into())),
            })
            .unwrap();

        assert!(response.messages.is_empty());
        assert_eq!(response.logs, vec!["print: hello"]);
    }

    #[test]
    fn ignores_control_payloads() {
        let response = PrintPlugin
            .handle(PluginRequest {
                plugin: "print".into(),
                message: Message::new("engine", "system.stop", Payload::Control(Control::Stop)),
            })
            .unwrap();

        assert!(response.messages.is_empty());
        assert!(response.logs.is_empty());
    }
}
