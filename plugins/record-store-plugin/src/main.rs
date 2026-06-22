use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse, Value};
use cubex_store::RecordStore;
use std::path::PathBuf;

#[derive(Default)]
struct RecordStorePlugin {
    store: Option<RecordStore>,
}

impl Plugin for RecordStorePlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        let message = request.message;
        if let Payload::Control(Control::Start { args }) = message.payload {
            if args.len() > 1 {
                anyhow::bail!("record store accepts at most 1 arg");
            }
            let path = args
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cubex-records.bin"));
            if path.as_os_str().is_empty() {
                anyhow::bail!("record store path must not be empty");
            }
            if path.to_string_lossy().trim().is_empty() {
                anyhow::bail!("record store path must not be blank");
            }
            let store = RecordStore::new(path);
            store.load()?;
            self.store = Some(store);
            return Ok(PluginResponse::default());
        }

        match message.topic.as_str() {
            "record.put" if matches!(&message.payload, Payload::Record(_)) => {
                let key = record_key(&message)?.unwrap_or_else(|| message.id.to_string());
                self.store().put(key.clone(), message)?;
                Ok(PluginResponse {
                    messages: vec![Message::new(
                        request.plugin,
                        "record.stored",
                        Payload::Text(key),
                    )],
                    logs: Vec::new(),
                    error: None,
                })
            }
            "record.get" if key_payload(&message.payload) => {
                let key = message_key(&message, "lookup")?;
                let response = match self.store().get(&key)? {
                    Some(record) => {
                        format!("record {} found from {}", record.key, record.message.source)
                    }
                    None => format!("record {key} not found"),
                };
                Ok(PluginResponse {
                    messages: vec![Message::new(
                        request.plugin,
                        "record.loaded",
                        Payload::Text(response),
                    )],
                    logs: Vec::new(),
                    error: None,
                })
            }
            "record.delete" if key_payload(&message.payload) => {
                let key = message_key(&message, "delete")?;
                let response = if self.store().delete(&key)? {
                    format!("record {key} deleted")
                } else {
                    format!("record {key} not found")
                };
                Ok(PluginResponse {
                    messages: vec![Message::new(
                        request.plugin,
                        "record.deleted",
                        Payload::Text(response),
                    )],
                    logs: Vec::new(),
                    error: None,
                })
            }
            "record.list" if key_payload(&message.payload) => {
                let keys = self
                    .store()
                    .load()?
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let response = if keys.is_empty() {
                    "records: <empty>".into()
                } else {
                    format!("records: {keys}")
                };
                Ok(PluginResponse {
                    messages: vec![Message::new(
                        request.plugin,
                        "record.listed",
                        Payload::Text(response),
                    )],
                    logs: Vec::new(),
                    error: None,
                })
            }
            _ => Ok(PluginResponse::default()),
        }
    }
}

fn key_payload(payload: &Payload) -> bool {
    matches!(payload, Payload::Text(_) | Payload::Record(_))
}

fn message_key(message: &Message, operation: &str) -> anyhow::Result<String> {
    match &message.payload {
        Payload::Text(key) => text_key(key, operation),
        Payload::Record(_) => record_key(message)?
            .ok_or_else(|| anyhow::anyhow!("record {operation} key is required")),
        _ => unreachable!("message_key is only called for key payloads"),
    }
}

fn text_key(key: &str, operation: &str) -> anyhow::Result<String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        anyhow::bail!("record {operation} key must not be empty");
    }
    Ok(key)
}

fn record_key(message: &Message) -> anyhow::Result<Option<String>> {
    let Payload::Record(fields) = &message.payload else {
        return Ok(None);
    };
    let Some(value) = fields.get("key") else {
        return Ok(None);
    };
    let Value::String(key) = value else {
        anyhow::bail!("record key must be a string");
    };
    if key.trim().is_empty() {
        anyhow::bail!("record key must not be empty");
    }
    if key.trim() != key {
        anyhow::bail!("record key must not be padded");
    }
    Ok(Some(key.clone()))
}

impl RecordStorePlugin {
    fn store(&mut self) -> &RecordStore {
        self.store
            .get_or_insert_with(|| RecordStore::new("cubex-records.bin"))
    }
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(RecordStorePlugin::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubex_plugin_sdk::Value;
    use std::collections::BTreeMap;

    #[test]
    fn get_trims_text_key() {
        let path = std::env::temp_dir().join(format!("cubex-record-get-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };
        let record = Payload::Record(BTreeMap::from([(
            "key".into(),
            Value::String("demo".into()),
        )]));

        plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.put", record),
            })
            .unwrap();
        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.get", Payload::Text("demo\n".into())),
            })
            .unwrap();

        assert_eq!(
            response.messages[0].payload,
            Payload::Text("record demo found from source".into())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn get_accepts_record_key() {
        let path =
            std::env::temp_dir().join(format!("cubex-record-get-record-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };

        plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.put", key_record("demo")),
            })
            .unwrap();
        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.get", key_record("demo")),
            })
            .unwrap();

        assert_eq!(
            response.messages[0].payload,
            Payload::Text("record demo found from source".into())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn get_reports_missing_records() {
        let path = std::env::temp_dir().join(format!("cubex-record-missing-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };

        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.get", Payload::Text("missing".into())),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "record.loaded");
        assert_eq!(
            response.messages[0].payload,
            Payload::Text("record missing not found".into())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_empty_lookup_key() {
        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.get", Payload::Text("\n".into())),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "record lookup key must not be empty");
    }

    #[test]
    fn delete_removes_records() {
        let path = std::env::temp_dir().join(format!("cubex-record-delete-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };
        let record = Payload::Record(BTreeMap::from([(
            "key".into(),
            Value::String("demo".into()),
        )]));

        plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.put", record),
            })
            .unwrap();
        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.delete", Payload::Text("demo".into())),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "record.deleted");
        assert_eq!(
            response.messages[0].payload,
            Payload::Text("record demo deleted".into())
        );
        assert!(RecordStore::new(&path).get("demo").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn delete_accepts_record_key() {
        let path =
            std::env::temp_dir().join(format!("cubex-record-delete-record-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };

        plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.put", key_record("demo")),
            })
            .unwrap();
        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.delete", key_record("demo")),
            })
            .unwrap();

        assert_eq!(
            response.messages[0].payload,
            Payload::Text("record demo deleted".into())
        );
        assert!(RecordStore::new(&path).get("demo").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_empty_delete_key() {
        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.delete", Payload::Text("\n".into())),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "record delete key must not be empty");
    }

    #[test]
    fn list_reports_stored_keys() {
        let path = std::env::temp_dir().join(format!("cubex-record-list-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };

        for key in ["alpha", "beta"] {
            plugin
                .handle(PluginRequest {
                    plugin: "store".into(),
                    message: Message::new(
                        "source",
                        "record.put",
                        Payload::Record(BTreeMap::from([(
                            "key".into(),
                            Value::String(key.into()),
                        )])),
                    ),
                })
                .unwrap();
        }
        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.list", Payload::Text(String::new())),
            })
            .unwrap();

        assert_eq!(response.messages[0].topic, "record.listed");
        assert_eq!(
            response.messages[0].payload,
            Payload::Text("records: alpha, beta".into())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn list_accepts_record_payload() {
        let path =
            std::env::temp_dir().join(format!("cubex-record-list-record-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };

        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.list", Payload::Record(BTreeMap::new())),
            })
            .unwrap();

        assert_eq!(
            response.messages[0].payload,
            Payload::Text("records: <empty>".into())
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_empty_store_path() {
        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
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

        assert_eq!(error, "record store path must not be empty");
    }

    #[test]
    fn rejects_blank_store_path() {
        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
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

        assert_eq!(error, "record store path must not be blank");
    }

    #[test]
    fn rejects_corrupt_store_path() {
        let path = std::env::temp_dir().join(format!("cubex-record-corrupt-{}.bin", unique_id()));
        std::fs::write(&path, [1_u8, 2, 3]).unwrap();

        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![path.to_string_lossy().into_owned()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("codec error"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_extra_args() {
        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["records.bin".into(), "ignored".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "record store accepts at most 1 arg");
    }

    #[test]
    fn ignores_records_with_other_topics() {
        let path = std::env::temp_dir().join(format!("cubex-record-ignore-{}.bin", unique_id()));
        let mut plugin = RecordStorePlugin {
            store: Some(RecordStore::new(&path)),
        };
        let record = Payload::Record(BTreeMap::from([(
            "key".into(),
            Value::String("demo".into()),
        )]));

        let response = plugin
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "other.record", record),
            })
            .unwrap();

        assert!(response.messages.is_empty());
        assert!(RecordStore::new(&path).get("demo").unwrap().is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejects_invalid_record_keys() {
        for (key, message) in [
            ("", "record key must not be empty"),
            (" demo", "record key must not be padded"),
        ] {
            let record =
                Payload::Record(BTreeMap::from([("key".into(), Value::String(key.into()))]));
            let error = RecordStorePlugin::default()
                .handle(PluginRequest {
                    plugin: "store".into(),
                    message: Message::new("source", "record.put", record),
                })
                .unwrap_err()
                .to_string();

            assert_eq!(error, message);
        }

        let record = Payload::Record(BTreeMap::from([("key".into(), Value::U64(7))]));
        let error = RecordStorePlugin::default()
            .handle(PluginRequest {
                plugin: "store".into(),
                message: Message::new("source", "record.put", record),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "record key must be a string");
    }

    fn key_record(key: &str) -> Payload {
        Payload::Record(BTreeMap::from([("key".into(), Value::String(key.into()))]))
    }

    fn unique_id() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }
}
