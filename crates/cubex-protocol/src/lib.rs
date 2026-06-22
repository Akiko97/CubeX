use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use uuid::Uuid;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
static NEXT_MESSAGE_ID: AtomicU64 = AtomicU64::new(1);

pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub source: String,
    pub topic: String,
    pub payload: Payload,
}

impl Message {
    pub fn new(source: impl Into<String>, topic: impl Into<String>, payload: Payload) -> Self {
        Self {
            id: new_message_id(),
            source: source.into(),
            topic: topic.into(),
            payload,
        }
    }

    pub fn payload_kind(&self) -> PayloadKind {
        self.payload.kind()
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn new_message_id() -> Uuid {
    Uuid::new_v4()
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn new_message_id() -> Uuid {
    Uuid::from_u128(NEXT_MESSAGE_ID.fetch_add(1, Ordering::Relaxed) as u128)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Payload {
    Control(Control),
    Text(String),
    Bytes(Vec<u8>),
    Record(BTreeMap<String, Value>),
}

impl Payload {
    pub fn kind(&self) -> PayloadKind {
        match self {
            Self::Control(_) => PayloadKind::Control,
            Self::Text(_) => PayloadKind::Text,
            Self::Bytes(_) => PayloadKind::Bytes,
            Self::Record(_) => PayloadKind::Record,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PayloadKind {
    Control,
    Text,
    Bytes,
    Record,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Control {
    Start { args: Vec<String> },
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Value {
    Bool(bool),
    I64(i64),
    U64(u64),
    String(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRequest {
    pub plugin: String,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PluginResponse {
    pub messages: Vec<Message>,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostRequest {
    FileRead {
        path: String,
    },
    FileWrite {
        path: String,
        bytes: Vec<u8>,
    },
    TcpRequest {
        addr: String,
        bytes: Vec<u8>,
        timeout_ms: u64,
    },
    TcpEcho {
        addr: String,
        max_connections: u64,
    },
    Sleep {
        millis: u64,
    },
    RandomBytes {
        len: u32,
    },
    RecordPut {
        path: String,
        key: String,
        message: Message,
    },
    RecordGet {
        path: String,
        key: String,
    },
    RecordDelete {
        path: String,
        key: String,
    },
    RecordList {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostResponse {
    pub payload: HostPayload,
    pub error: Option<String>,
}

impl HostResponse {
    pub fn ok(payload: HostPayload) -> Self {
        Self {
            payload,
            error: None,
        }
    }

    pub fn error(reason: impl Into<String>) -> Self {
        Self {
            payload: HostPayload::Unit,
            error: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostPayload {
    Unit,
    Bytes(Vec<u8>),
    Text(String),
    Bool(bool),
    Message(Option<Message>),
    StringList(Vec<String>),
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("frame is too large: {0} bytes")]
    FrameTooLarge(u32),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec error: {0}")]
    Codec(#[from] Box<bincode::ErrorKind>),
}

pub type Result<T> = std::result::Result<T, ProtocolError>;

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(bincode::serialize(value)?)
}

pub fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    Ok(bincode::deserialize(bytes)?)
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let bytes = encode(value)?;
    let len = u32::try_from(bytes.len()).map_err(|_| ProtocolError::FrameTooLarge(u32::MAX))?;
    if len > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(reader: &mut R) -> Result<Option<T>> {
    let mut len_buf = [0_u8; 4];
    let mut read = 0;
    while read < len_buf.len() {
        let count = reader.read(&mut len_buf[read..])?;
        if count == 0 {
            if read == 0 {
                return Ok(None);
            }
            return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
        }
        read += count;
    }
    let len = u32::from_le_bytes(len_buf);
    if len > MAX_FRAME_SIZE {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    let mut bytes = vec![0_u8; len as usize];
    reader.read_exact(&mut bytes)?;
    Ok(Some(decode(&bytes)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip() {
        let msg = Message::new("a", "b", Payload::Text("hello".into()));
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &msg).unwrap();

        let decoded: Message = read_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(decoded.source, "a");
        assert_eq!(decoded.topic, "b");
        assert_eq!(decoded.payload, Payload::Text("hello".into()));
    }

    #[test]
    fn empty_stream_is_end_of_frames() {
        let decoded: Option<Message> = read_frame(&mut [].as_slice()).unwrap();
        assert_eq!(decoded, None);
    }

    #[test]
    fn partial_length_header_is_error() {
        let err = read_frame::<_, Message>(&mut [1_u8, 0].as_slice()).unwrap_err();
        assert!(
            matches!(err, ProtocolError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof)
        );
    }

    #[test]
    fn partial_payload_is_error() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        bytes.extend_from_slice(&[1_u8, 2]);

        let err = read_frame::<_, Message>(&mut bytes.as_slice()).unwrap_err();
        assert!(
            matches!(err, ProtocolError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof)
        );
    }

    #[test]
    fn oversized_frame_is_rejected_before_payload_read() {
        let bytes = (MAX_FRAME_SIZE + 1).to_le_bytes();
        let err = read_frame::<_, Message>(&mut bytes.as_slice()).unwrap_err();
        assert!(matches!(err, ProtocolError::FrameTooLarge(size) if size == MAX_FRAME_SIZE + 1));
    }
}
