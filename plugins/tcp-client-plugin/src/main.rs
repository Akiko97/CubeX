use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Default)]
struct TcpClientPlugin;

impl Plugin for TcpClientPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };

        if args.len() > 3 {
            anyhow::bail!("tcp client accepts at most 3 args");
        }
        let addr = args
            .first()
            .ok_or_else(|| anyhow::anyhow!("tcp client needs address as arg 0"))?;
        if addr.trim().is_empty() || addr.trim() != addr {
            anyhow::bail!("tcp client address must not be empty or padded");
        }
        let text = args.get(1).cloned().unwrap_or_else(|| "ping".into());
        let timeout = parse_timeout(args.get(2))?;
        let mut stream = connect(addr, timeout)?;
        stream.write_all(text.as_bytes())?;
        stream.shutdown(Shutdown::Write)?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "tcp.response",
                Payload::Text(response),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(TcpClientPlugin)
}

fn parse_timeout(value: Option<&String>) -> anyhow::Result<Duration> {
    let Some(value) = value else {
        return Ok(Duration::from_secs(2));
    };
    if value.trim().is_empty() || value.trim() != value {
        anyhow::bail!("tcp client timeout must not be empty or padded");
    }
    let millis: u64 = value
        .parse()
        .map_err(|_| anyhow::anyhow!("tcp client timeout must be an unsigned integer"))?;
    if millis == 0 {
        anyhow::bail!("tcp client timeout must be positive");
    }
    Ok(Duration::from_millis(millis))
}

fn connect(addr: &str, timeout: Duration) -> anyhow::Result<TcpStream> {
    let mut resolved = false;
    let mut last_error = None;
    for addr in addr.to_socket_addrs()? {
        resolved = true;
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }
    if !resolved {
        anyhow::bail!("tcp client address did not resolve");
    }
    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("tcp client connect failed"))
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn sends_default_ping_and_emits_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            stream.read_to_string(&mut request).unwrap();
            assert_eq!(request, "ping");
            stream.write_all(b"pong").unwrap();
        });

        let response = TcpClientPlugin
            .handle(PluginRequest {
                plugin: "tcp-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![addr.to_string()],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "tcp.response");
        assert_eq!(response.messages[0].payload, Payload::Text("pong".into()));
        server.join().unwrap();
    }

    #[test]
    fn rejects_bad_address() {
        let error = TcpClientPlugin
            .handle(PluginRequest {
                plugin: "tcp-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![" 127.0.0.1:1".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "tcp client address must not be empty or padded");
    }

    #[test]
    fn rejects_extra_args() {
        let error = TcpClientPlugin
            .handle(PluginRequest {
                plugin: "tcp-client".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            "127.0.0.1:1".into(),
                            "ping".into(),
                            "1".into(),
                            "ignored".into(),
                        ],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "tcp client accepts at most 3 args");
    }

    #[test]
    fn rejects_bad_timeout() {
        for (timeout, message) in [
            (" 1", "tcp client timeout must not be empty or padded"),
            ("many", "tcp client timeout must be an unsigned integer"),
            ("0", "tcp client timeout must be positive"),
        ] {
            let timeout = timeout.to_string();
            let error = parse_timeout(Some(&timeout)).unwrap_err().to_string();
            assert_eq!(error, message);
        }
    }
}
