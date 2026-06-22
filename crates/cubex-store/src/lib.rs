use cubex_protocol::{Message, Payload};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(#[from] cubex_protocol::ProtocolError),
    #[error("codec error: {0}")]
    Codec(#[from] Box<bincode::ErrorKind>),
    #[error("invalid event message: {0}")]
    InvalidEventMessage(String),
    #[error("invalid record key: {0}")]
    InvalidRecordKey(String),
    #[error("invalid record message: {0}")]
    InvalidRecordMessage(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone)]
pub struct EventLog {
    path: PathBuf,
}

impl EventLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn append(&self, message: &Message) -> Result<()> {
        validate_event_message(message)?;
        ensure_parent(&self.path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        cubex_protocol::write_frame(&mut file, message)?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<Message>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(&self.path)?;
        let mut messages = Vec::new();
        while let Some(message) = cubex_protocol::read_frame(&mut file)? {
            validate_event_message(&message)?;
            messages.push(message);
        }
        Ok(messages)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecord {
    pub key: String,
    pub message: Message,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Clone)]
pub struct RecordStore {
    path: PathBuf,
}

impl RecordStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn put(&self, key: impl Into<String>, message: Message) -> Result<()> {
        let key = key.into();
        validate_key(&key)?;
        validate_record_message(&message)?;
        let mut records = self.load()?;
        records.insert(
            key.clone(),
            StoredRecord {
                key,
                message,
                updated_at_unix_ms: unix_ms(),
            },
        );
        self.save(&records)
    }

    pub fn get(&self, key: &str) -> Result<Option<StoredRecord>> {
        validate_key(key)?;
        Ok(self.load()?.get(key).cloned())
    }

    pub fn delete(&self, key: &str) -> Result<bool> {
        validate_key(key)?;
        let mut records = self.load()?;
        let deleted = records.remove(key).is_some();
        if deleted {
            self.save(&records)?;
        }
        Ok(deleted)
    }

    pub fn load(&self) -> Result<BTreeMap<String, StoredRecord>> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            return Ok(BTreeMap::new());
        }
        let records: BTreeMap<String, StoredRecord> = bincode::deserialize(&bytes)?;
        validate_records(&records)?;
        Ok(records)
    }

    fn save(&self, records: &BTreeMap<String, StoredRecord>) -> Result<()> {
        ensure_parent(&self.path)?;
        let bytes = bincode::serialize(records)?;
        let tmp = write_temp_file(&self.path, &bytes)?;
        if let Err(err) = std::fs::rename(&tmp, &self.path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(err.into());
        }
        Ok(())
    }
}

fn validate_records(records: &BTreeMap<String, StoredRecord>) -> Result<()> {
    for (key, record) in records {
        validate_key(key)?;
        validate_key(&record.key)?;
        if key != &record.key {
            return Err(StoreError::InvalidRecordKey(
                "stored key must match map key".into(),
            ));
        }
        validate_record_message(&record.message)?;
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        return Err(StoreError::InvalidRecordKey("key must not be empty".into()));
    }
    if key.trim() != key {
        return Err(StoreError::InvalidRecordKey(
            "key must not be padded".into(),
        ));
    }
    Ok(())
}

fn validate_event_message(message: &Message) -> Result<()> {
    message_error(message).map_or(Ok(()), |reason| {
        Err(StoreError::InvalidEventMessage(reason.into()))
    })
}

fn validate_record_message(message: &Message) -> Result<()> {
    message_error(message).map_or(Ok(()), |reason| {
        Err(StoreError::InvalidRecordMessage(reason.into()))
    })
}

fn message_error(message: &Message) -> Option<&'static str> {
    if message.id.is_nil() {
        return Some("id must not be nil");
    }
    if message.source.trim().is_empty() {
        return Some("source must not be empty");
    }
    if message.source.trim() != message.source {
        return Some("source must not be padded");
    }
    if message.topic.trim().is_empty() {
        return Some("topic must not be empty");
    }
    if message.topic.trim() != message.topic {
        return Some("topic must not be padded");
    }
    if matches!(message.payload, Payload::Control(_)) {
        return Some("control payloads are reserved for host messages");
    }
    None
}

fn write_temp_file(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    for attempt in 0..1000 {
        let tmp = temp_file_path(path, attempt);
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(mut file) => {
                if let Err(err) = file.write_all(bytes) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(err.into());
                }
                return Ok(tmp);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err.into()),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create record store temporary file",
    )
    .into())
}

fn temp_file_path(path: &Path, attempt: u16) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("cubex-store")),
    );
    name.push(format!(".{attempt}.tmp"));
    path.with_file_name(name)
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cubex_protocol::Payload;

    #[test]
    fn event_log_round_trip() {
        let path = std::env::temp_dir().join(format!("cubex-event-{}.bin", uuid()));
        let log = EventLog::new(&path);
        log.append(&Message::new("a", "topic", Payload::Text("one".into())))
            .unwrap();
        log.append(&Message::new("b", "topic", Payload::Text("two".into())))
            .unwrap();

        let messages = log.read_all().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].source, "a");
        assert_eq!(messages[1].source, "b");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn event_log_truncated_frame_is_error() {
        let path = std::env::temp_dir().join(format!("cubex-event-bad-{}.bin", uuid()));
        std::fs::write(&path, [1_u8, 0]).unwrap();

        assert!(EventLog::new(&path).read_all().is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn event_log_rejects_invalid_messages() {
        let path = std::env::temp_dir().join(format!("cubex-event-invalid-{}.bin", uuid()));
        let error = EventLog::new(&path)
            .append(&Message {
                id: Default::default(),
                source: "test".into(),
                topic: "topic".into(),
                payload: Payload::Text("bad".into()),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "invalid event message: id must not be nil");
        assert!(!path.exists());
    }

    #[test]
    fn event_log_read_rejects_invalid_messages() {
        let path = std::env::temp_dir().join(format!("cubex-event-read-invalid-{}.bin", uuid()));
        write_event(
            &path,
            &Message {
                id: Default::default(),
                source: "test".into(),
                topic: "topic".into(),
                payload: Payload::Text("bad".into()),
            },
        );

        let error = EventLog::new(&path).read_all().unwrap_err().to_string();

        assert_eq!(error, "invalid event message: id must not be nil");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_round_trip() {
        let path = std::env::temp_dir().join(format!("cubex-record-{}.bin", uuid()));
        let store = RecordStore::new(&path);
        store
            .put(
                "answer",
                Message::new("test", "record", Payload::Text("42".into())),
            )
            .unwrap();

        let record = store.get("answer").unwrap().unwrap();
        assert_eq!(record.key, "answer");
        assert_eq!(record.message.source, "test");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_delete_removes_existing_records() {
        let path = std::env::temp_dir().join(format!("cubex-record-delete-{}.bin", uuid()));
        let store = RecordStore::new(&path);
        store
            .put(
                "answer",
                Message::new("test", "record", Payload::Text("42".into())),
            )
            .unwrap();

        assert!(store.delete("answer").unwrap());
        assert!(store.get("answer").unwrap().is_none());
        assert!(!store.delete("answer").unwrap());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_accepts_plain_relative_file_path() {
        let path = PathBuf::from(format!("cubex-record-{}.bin", uuid()));
        let store = RecordStore::new(&path);
        store
            .put(
                "answer",
                Message::new("test", "record", Payload::Text("42".into())),
            )
            .unwrap();

        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_empty_file_is_empty_store() {
        let path = std::env::temp_dir().join(format!("cubex-record-empty-{}.bin", uuid()));
        std::fs::write(&path, []).unwrap();

        let records = RecordStore::new(&path).load().unwrap();

        assert!(records.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_corrupt_file_is_error() {
        let path = std::env::temp_dir().join(format!("cubex-record-bad-{}.bin", uuid()));
        std::fs::write(&path, [1_u8, 2, 3]).unwrap();

        assert!(RecordStore::new(&path).load().is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_put_rejects_invalid_messages() {
        let path = std::env::temp_dir().join(format!("cubex-record-message-{}.bin", uuid()));
        let message = Message {
            id: Default::default(),
            source: "test".into(),
            topic: "record".into(),
            payload: Payload::Text("42".into()),
        };

        let error = RecordStore::new(&path)
            .put("answer", message)
            .unwrap_err()
            .to_string();

        assert_eq!(error, "invalid record message: id must not be nil");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_load_rejects_mismatched_keys() {
        let path = std::env::temp_dir().join(format!("cubex-record-key-mismatch-{}.bin", uuid()));
        let records: BTreeMap<String, StoredRecord> = BTreeMap::from([(
            "answer".into(),
            StoredRecord {
                key: "other".into(),
                message: Message::new("test", "record", Payload::Text("42".into())),
                updated_at_unix_ms: 0,
            },
        )]);
        std::fs::write(&path, bincode::serialize(&records).unwrap()).unwrap();

        let error = RecordStore::new(&path).load().unwrap_err().to_string();

        assert_eq!(error, "invalid record key: stored key must match map key");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_load_rejects_invalid_messages() {
        let path = std::env::temp_dir().join(format!("cubex-record-message-bad-{}.bin", uuid()));
        let records: BTreeMap<String, StoredRecord> = BTreeMap::from([(
            "answer".into(),
            StoredRecord {
                key: "answer".into(),
                message: Message::new(
                    "test",
                    "system.stop",
                    Payload::Control(cubex_protocol::Control::Stop),
                ),
                updated_at_unix_ms: 0,
            },
        )]);
        std::fs::write(&path, bincode::serialize(&records).unwrap()).unwrap();

        let error = RecordStore::new(&path).load().unwrap_err().to_string();

        assert_eq!(
            error,
            "invalid record message: control payloads are reserved for host messages"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_does_not_overwrite_existing_temp_file() {
        let path = std::env::temp_dir().join(format!("cubex-record-tmp-{}.bin", uuid()));
        let stale_tmp = temp_file_path(&path, 0);
        std::fs::write(&stale_tmp, b"stale").unwrap();

        RecordStore::new(&path)
            .put(
                "answer",
                Message::new("test", "record", Payload::Text("42".into())),
            )
            .unwrap();

        assert_eq!(std::fs::read(&stale_tmp).unwrap(), b"stale");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(stale_tmp);
    }

    #[test]
    fn record_store_rejects_invalid_keys() {
        let path = std::env::temp_dir().join(format!("cubex-record-key-{}.bin", uuid()));
        let store = RecordStore::new(&path);

        let error = store
            .put(
                " ",
                Message::new("test", "record", Payload::Text("42".into())),
            )
            .unwrap_err()
            .to_string();

        assert_eq!(error, "invalid record key: key must not be empty");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn record_store_get_rejects_invalid_keys() {
        let store = RecordStore::new("unused.bin");

        for (key, message) in [
            (" ", "invalid record key: key must not be empty"),
            (" answer", "invalid record key: key must not be padded"),
        ] {
            let error = store.get(key).unwrap_err().to_string();
            assert_eq!(error, message);
        }
    }

    fn uuid() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }

    fn write_event(path: &Path, message: &Message) {
        let mut file = File::create(path).unwrap();
        cubex_protocol::write_frame(&mut file, message).unwrap();
    }
}
