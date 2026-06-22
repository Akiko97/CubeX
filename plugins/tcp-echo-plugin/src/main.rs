use cubex_plugin_sdk::{Control, Payload, Plugin, PluginRequest, PluginResponse};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

#[derive(Default)]
struct TcpEchoPlugin {
    started: bool,
}

impl Plugin for TcpEchoPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };
        if self.started {
            return Ok(PluginResponse::default());
        }

        if args.len() > 2 {
            anyhow::bail!("tcp echo accepts at most 2 args");
        }
        let addr = args
            .first()
            .cloned()
            .unwrap_or_else(|| "127.0.0.1:4010".into());
        if addr.trim().is_empty() || addr.trim() != addr {
            anyhow::bail!("tcp echo address must not be empty or padded");
        }
        let max_connections = parse_max_connections(args.get(1))?;
        let listener = TcpListener::bind(&addr)?;
        let local_addr = listener.local_addr()?;
        self.started = true;

        thread::spawn(move || {
            for stream in listener.incoming().take(max_connections) {
                let Ok(mut stream) = stream else {
                    break;
                };
                let mut buf = Vec::new();
                if stream.read_to_end(&mut buf).is_ok() {
                    let _ = stream.write_all(&buf);
                }
            }
        });

        Ok(PluginResponse {
            messages: Vec::new(),
            logs: vec![format!("tcp echo listening on {local_addr}")],
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(TcpEchoPlugin::default())
}

fn parse_max_connections(value: Option<&String>) -> anyhow::Result<usize> {
    let Some(value) = value else {
        return Ok(1);
    };
    if value.trim().is_empty() || value.trim() != value {
        anyhow::bail!("tcp echo max connections must not be empty or padded");
    }
    let max_connections: usize = value
        .parse()
        .map_err(|_| anyhow::anyhow!("tcp echo max connections must be an unsigned integer"))?;
    if max_connections == 0 {
        anyhow::bail!("tcp echo max connections must be positive");
    }
    Ok(max_connections)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_max_connections() {
        let value = "0".to_string();
        let error = parse_max_connections(Some(&value)).unwrap_err().to_string();

        assert_eq!(error, "tcp echo max connections must be positive");
    }

    #[test]
    fn rejects_blank_max_connections() {
        let value = " 1".to_string();
        let error = parse_max_connections(Some(&value)).unwrap_err().to_string();

        assert_eq!(
            error,
            "tcp echo max connections must not be empty or padded"
        );
    }

    #[test]
    fn rejects_invalid_max_connections() {
        let value = "many".to_string();
        let error = parse_max_connections(Some(&value)).unwrap_err().to_string();

        assert_eq!(
            error,
            "tcp echo max connections must be an unsigned integer"
        );
    }

    #[test]
    fn rejects_bad_address() {
        let mut plugin = TcpEchoPlugin::default();
        let error = plugin
            .handle(PluginRequest {
                plugin: "tcp-echo".into(),
                message: cubex_plugin_sdk::Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![" ".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "tcp echo address must not be empty or padded");
        assert!(!plugin.started);
    }

    #[test]
    fn rejects_extra_args() {
        let mut plugin = TcpEchoPlugin::default();
        let error = plugin
            .handle(PluginRequest {
                plugin: "tcp-echo".into(),
                message: cubex_plugin_sdk::Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["127.0.0.1:0".into(), "1".into(), "ignored".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "tcp echo accepts at most 2 args");
        assert!(!plugin.started);
    }
}
