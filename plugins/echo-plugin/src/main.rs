use cubex_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse};

#[derive(Default)]
struct EchoPlugin;

impl Plugin for EchoPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let payload = match request.message.payload {
            Payload::Text(text) => Payload::Text(text),
            Payload::Bytes(bytes) => Payload::Bytes(bytes),
            _ => return Ok(PluginResponse::default()),
        };

        Ok(PluginResponse {
            messages: vec![Message::new(request.plugin, "echo.reply", payload)],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(EchoPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoes_text_payload() {
        let response = EchoPlugin
            .handle(PluginRequest {
                plugin: "echo".into(),
                message: Message::new("source", "input", Payload::Text("hello".into())),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "echo.reply");
        assert_eq!(response.messages[0].payload, Payload::Text("hello".into()));
    }

    #[test]
    fn echoes_bytes_payload() {
        let response = EchoPlugin
            .handle(PluginRequest {
                plugin: "echo".into(),
                message: Message::new("source", "input", Payload::Bytes(vec![1, 2, 3])),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "echo.reply");
        assert_eq!(response.messages[0].payload, Payload::Bytes(vec![1, 2, 3]));
    }

    #[test]
    fn ignores_non_echoable_payload() {
        let response = EchoPlugin
            .handle(PluginRequest {
                plugin: "echo".into(),
                message: Message::new("source", "input", Payload::Record(Default::default())),
            })
            .unwrap();

        assert!(response.messages.is_empty());
    }
}
