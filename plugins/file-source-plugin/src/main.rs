use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct FileSourcePlugin;

impl Plugin for FileSourcePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let Payload::Control(Control::Start { args }) = request.message.payload else {
            return Ok(PluginResponse::default());
        };

        if args.len() > 3 {
            anyhow::bail!("file source accepts at most 3 args");
        }
        let path = args
            .first()
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("file source needs input path as arg 0"))?;
        if path.as_os_str().is_empty() {
            anyhow::bail!("file source input path must not be empty");
        }
        if path.to_string_lossy().trim().is_empty() {
            anyhow::bail!("file source input path must not be blank");
        }
        let topic = args.get(1).cloned().unwrap_or_else(|| "file.read".into());
        if topic.trim().is_empty() {
            anyhow::bail!("file source topic must not be empty");
        }
        if topic.trim() != topic {
            anyhow::bail!("file source topic must not be padded");
        }
        let mode = args.get(2).map(String::as_str).unwrap_or("text");
        let payload = match mode {
            "text" => Payload::Text(std::fs::read_to_string(&path)?),
            "bytes" => Payload::Bytes(std::fs::read(&path)?),
            "record-key" => Payload::Record(BTreeMap::from([(
                "key".into(),
                Value::String(read_key(&path)?),
            )])),
            "record" => Payload::Record(read_record(&path, false)?),
            "record-typed" => Payload::Record(read_record(&path, true)?),
            other => {
                anyhow::bail!(
                    "file source mode must be `text`, `bytes`, `record-key`, `record`, or `record-typed`, got `{other}`"
                )
            }
        };

        Ok(PluginResponse {
            messages: vec![Message::new(request.plugin, topic, payload)],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn read_key(path: &Path) -> anyhow::Result<String> {
    let key = std::fs::read_to_string(path)?.trim().to_string();
    if key.is_empty() {
        anyhow::bail!("file source record key must not be empty");
    }
    Ok(key)
}

fn read_record(path: &Path, typed: bool) -> anyhow::Result<BTreeMap<String, Value>> {
    let mut record = BTreeMap::new();
    for (index, line) in std::fs::read_to_string(path)?.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            anyhow::bail!("file source record line {} must use key=value", index + 1);
        };
        if key.trim().is_empty() {
            anyhow::bail!("file source record key must not be empty");
        }
        if key.trim() != key {
            anyhow::bail!("file source record key must not be padded");
        }
        if record
            .insert(key.into(), record_value(value, typed))
            .is_some()
        {
            anyhow::bail!("file source record key `{key}` is duplicated");
        }
    }
    if record.is_empty() {
        anyhow::bail!("file source record must not be empty");
    }
    Ok(record)
}

fn record_value(value: &str, typed: bool) -> Value {
    if !typed {
        return Value::String(value.into());
    }
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => value
            .parse::<i64>()
            .map(Value::I64)
            .unwrap_or_else(|_| Value::String(value.into())),
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(FileSourcePlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_text_file() {
        let path = std::env::temp_dir().join(format!("cubex-file-source-{}.txt", unique_id()));
        std::fs::write(&path, "hello").unwrap();

        let response = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![path.to_string_lossy().into_owned()],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "file.read");
        assert_eq!(response.messages[0].payload, Payload::Text("hello".into()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_bytes_file() {
        let path = std::env::temp_dir().join(format!("cubex-file-source-{}.bin", unique_id()));
        std::fs::write(&path, [0_u8, 1, 255]).unwrap();

        let response = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            path.to_string_lossy().into_owned(),
                            "file.bytes".into(),
                            "bytes".into(),
                        ],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "file.bytes");
        assert_eq!(
            response.messages[0].payload,
            Payload::Bytes(vec![0, 1, 255])
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_record_key_file() {
        let path = std::env::temp_dir().join(format!("cubex-file-source-key-{}.txt", unique_id()));
        std::fs::write(&path, "demo\n").unwrap();

        let response = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            path.to_string_lossy().into_owned(),
                            "record.get".into(),
                            "record-key".into(),
                        ],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "record.get");
        assert_eq!(
            response.messages[0].payload,
            Payload::Record(BTreeMap::from([(
                "key".into(),
                Value::String("demo".into())
            )]))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_blank_record_key_file() {
        let path =
            std::env::temp_dir().join(format!("cubex-file-source-blank-key-{}.txt", unique_id()));
        std::fs::write(&path, "\n").unwrap();

        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            path.to_string_lossy().into_owned(),
                            "record.get".into(),
                            "record-key".into(),
                        ],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file source record key must not be empty");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_record_file() {
        let path =
            std::env::temp_dir().join(format!("cubex-file-source-record-{}.txt", unique_id()));
        std::fs::write(&path, "user=alice\nmessage=hello\n").unwrap();

        let response = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            path.to_string_lossy().into_owned(),
                            "record.put".into(),
                            "record".into(),
                        ],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "record.put");
        assert_eq!(
            response.messages[0].payload,
            Payload::Record(BTreeMap::from([
                ("message".into(), Value::String("hello".into())),
                ("user".into(), Value::String("alice".into())),
            ]))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_typed_record_file() {
        let path = std::env::temp_dir().join(format!(
            "cubex-file-source-typed-record-{}.txt",
            unique_id()
        ));
        std::fs::write(&path, "user=alice\npriority=7\nactive=true\n").unwrap();

        let response = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            path.to_string_lossy().into_owned(),
                            "record.put".into(),
                            "record-typed".into(),
                        ],
                    }),
                ),
            })
            .unwrap();

        assert_eq!(
            response.messages[0].payload,
            Payload::Record(BTreeMap::from([
                ("active".into(), Value::Bool(true)),
                ("priority".into(), Value::I64(7)),
                ("user".into(), Value::String("alice".into())),
            ]))
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_malformed_record_file() {
        let path =
            std::env::temp_dir().join(format!("cubex-file-source-bad-record-{}.txt", unique_id()));
        std::fs::write(&path, "user\n").unwrap();

        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            path.to_string_lossy().into_owned(),
                            "record.put".into(),
                            "record".into(),
                        ],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file source record line 1 must use key=value");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_unknown_mode() {
        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["missing.txt".into(), "file.read".into(), "json".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("file source mode must be"));
    }

    #[test]
    fn rejects_extra_args() {
        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![
                            "input.txt".into(),
                            "file.read".into(),
                            "text".into(),
                            "ignored".into(),
                        ],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file source accepts at most 3 args");
    }

    #[test]
    fn rejects_empty_input_path() {
        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![String::new()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file source input path must not be empty");
    }

    #[test]
    fn rejects_blank_input_path() {
        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
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

        assert_eq!(error, "file source input path must not be blank");
    }

    #[test]
    fn rejects_empty_topic() {
        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["missing.txt".into(), " ".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file source topic must not be empty");
    }

    #[test]
    fn rejects_padded_topic() {
        let error = FileSourcePlugin
            .handle(PluginRequest {
                plugin: "file-source".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["missing.txt".into(), " file.read".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file source topic must not be padded");
    }

    fn unique_id() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }
}
