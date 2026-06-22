pub use cubex_protocol::{Control, Message, Payload, PluginRequest, PluginResponse, Value};
use std::io::{Read, Write};

pub trait Plugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse>;
}

pub fn run_stdio(mut plugin: impl Plugin) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    run_io(&mut plugin, &mut reader, &mut writer)
}

fn run_io(
    plugin: &mut impl Plugin,
    mut reader: impl Read,
    mut writer: impl Write,
) -> anyhow::Result<()> {
    while let Some(request) = cubex_protocol::read_frame::<_, PluginRequest>(&mut reader)? {
        let mut response = match plugin.handle(request) {
            Ok(response) => response,
            Err(err) => PluginResponse {
                error: Some(plugin_error_text(err)),
                ..PluginResponse::default()
            },
        };
        normalize_response_error(&mut response);
        cubex_protocol::write_frame(&mut writer, &response)?;
    }
    Ok(())
}

fn plugin_error_text(err: anyhow::Error) -> String {
    normalize_error_text(err.to_string())
}

fn normalize_response_error(response: &mut PluginResponse) {
    if let Some(error) = response.error.take() {
        response.error = Some(normalize_error_text(error));
        response.messages.clear();
    }
}

fn normalize_error_text(text: String) -> String {
    let text = text.trim();
    if text.is_empty() {
        "plugin error".into()
    } else {
        text.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReadyPlugin;

    impl Plugin for ReadyPlugin {
        fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
            Ok(PluginResponse {
                messages: vec![Message::new(
                    request.plugin,
                    "ready",
                    Payload::Text("ok".into()),
                )],
                logs: vec!["handled".into()],
                error: None,
            })
        }
    }

    struct FailingPlugin(&'static str);

    impl Plugin for FailingPlugin {
        fn handle(&mut self, _request: PluginRequest) -> anyhow::Result<PluginResponse> {
            anyhow::bail!("{}", self.0)
        }
    }

    struct ManualErrorPlugin;

    impl Plugin for ManualErrorPlugin {
        fn handle(&mut self, _request: PluginRequest) -> anyhow::Result<PluginResponse> {
            Ok(PluginResponse {
                error: Some(" boom ".into()),
                ..PluginResponse::default()
            })
        }
    }

    struct ManualErrorWithMessagePlugin;

    impl Plugin for ManualErrorWithMessagePlugin {
        fn handle(&mut self, _request: PluginRequest) -> anyhow::Result<PluginResponse> {
            Ok(PluginResponse {
                messages: vec![Message::new("plugin", "late", Payload::Text("bad".into()))],
                error: Some("boom".into()),
                ..PluginResponse::default()
            })
        }
    }

    #[test]
    fn run_io_handles_framed_requests() {
        let mut input = Vec::new();
        cubex_protocol::write_frame(
            &mut input,
            &PluginRequest {
                plugin: "example".into(),
                message: Message::new("engine", "system.start", Payload::Control(Control::Stop)),
            },
        )
        .unwrap();
        let mut output = Vec::new();

        run_io(&mut ReadyPlugin, input.as_slice(), &mut output).unwrap();

        let response: PluginResponse = cubex_protocol::read_frame(&mut output.as_slice())
            .unwrap()
            .unwrap();
        assert_eq!(response.messages[0].topic, "ready");
        assert_eq!(response.logs, vec!["handled"]);
        assert_eq!(response.error, None);
    }

    #[test]
    fn run_io_returns_plugin_errors_as_responses() {
        let mut input = Vec::new();
        cubex_protocol::write_frame(
            &mut input,
            &PluginRequest {
                plugin: "example".into(),
                message: Message::new("engine", "system.start", Payload::Control(Control::Stop)),
            },
        )
        .unwrap();
        let mut output = Vec::new();

        run_io(&mut FailingPlugin("boom"), input.as_slice(), &mut output).unwrap();

        let response: PluginResponse = cubex_protocol::read_frame(&mut output.as_slice())
            .unwrap()
            .unwrap();
        assert_eq!(response.error.as_deref(), Some("boom"));
    }

    #[test]
    fn plugin_error_text_is_normalized() {
        assert_eq!(plugin_error_text(anyhow::anyhow!(" boom ")), "boom");
        assert_eq!(plugin_error_text(anyhow::anyhow!("")), "plugin error");
    }

    #[test]
    fn run_io_normalizes_manual_error_responses() {
        let mut input = Vec::new();
        cubex_protocol::write_frame(
            &mut input,
            &PluginRequest {
                plugin: "example".into(),
                message: Message::new("engine", "system.start", Payload::Control(Control::Stop)),
            },
        )
        .unwrap();
        let mut output = Vec::new();

        run_io(&mut ManualErrorPlugin, input.as_slice(), &mut output).unwrap();

        let response: PluginResponse = cubex_protocol::read_frame(&mut output.as_slice())
            .unwrap()
            .unwrap();
        assert_eq!(response.error.as_deref(), Some("boom"));
    }

    #[test]
    fn run_io_drops_messages_from_error_responses() {
        let mut input = Vec::new();
        cubex_protocol::write_frame(
            &mut input,
            &PluginRequest {
                plugin: "example".into(),
                message: Message::new("engine", "system.start", Payload::Control(Control::Stop)),
            },
        )
        .unwrap();
        let mut output = Vec::new();

        run_io(
            &mut ManualErrorWithMessagePlugin,
            input.as_slice(),
            &mut output,
        )
        .unwrap();

        let response: PluginResponse = cubex_protocol::read_frame(&mut output.as_slice())
            .unwrap()
            .unwrap();
        assert_eq!(response.error.as_deref(), Some("boom"));
        assert!(response.messages.is_empty());
    }
}
