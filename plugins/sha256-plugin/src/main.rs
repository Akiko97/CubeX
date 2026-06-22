use cubex_plugin_sdk::{Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Default)]
struct Sha256Plugin;

impl Plugin for Sha256Plugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let bytes = match request.message.payload {
            Payload::Text(text) => text.into_bytes(),
            Payload::Bytes(bytes) => bytes,
            _ => return Ok(PluginResponse::default()),
        };

        let digest = Sha256::digest(&bytes);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut fields = BTreeMap::new();
        fields.insert("algorithm".into(), Value::String("sha256".into()));
        fields.insert("hex".into(), Value::String(hex));
        fields.insert("size".into(), Value::U64(bytes.len() as u64));

        Ok(PluginResponse {
            messages: vec![Message::new(
                request.plugin,
                "crypto.sha256",
                Payload::Record(fields),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(Sha256Plugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_text_payload() {
        let response = Sha256Plugin
            .handle(PluginRequest {
                plugin: "sha256".into(),
                message: Message::new("source", "text", Payload::Text("abc".into())),
            })
            .unwrap();

        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(response.messages[0].topic, "crypto.sha256");
        assert_eq!(fields["algorithm"], Value::String("sha256".into()));
        assert_eq!(
            fields["hex"],
            Value::String(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into()
            )
        );
        assert_eq!(fields["size"], Value::U64(3));
    }

    #[test]
    fn hashes_bytes_payload() {
        let response = Sha256Plugin
            .handle(PluginRequest {
                plugin: "sha256".into(),
                message: Message::new(
                    "source",
                    "bytes",
                    Payload::Bytes(b"CubeX bytes payload\n".to_vec()),
                ),
            })
            .unwrap();

        let Payload::Record(fields) = &response.messages[0].payload else {
            panic!("expected record");
        };
        assert_eq!(
            fields["hex"],
            Value::String(
                "7f4617f80e9020429b94e1ec86c8b0631d36f1ff76efa95920430f793477fd4a".into()
            )
        );
        assert_eq!(fields["size"], Value::U64(20));
    }
}
