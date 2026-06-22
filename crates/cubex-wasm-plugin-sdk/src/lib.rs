pub use cubex_protocol::{
    Control, HostPayload, HostRequest, HostResponse, MAX_FRAME_SIZE, Message, Payload,
    PluginRequest, PluginResponse, ProtocolError, Value,
};

pub trait Plugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse>;
}

pub fn decode_request(bytes: &[u8]) -> cubex_protocol::Result<PluginRequest> {
    cubex_protocol::decode(bytes)
}

pub fn encode_response(response: &PluginResponse) -> cubex_protocol::Result<Vec<u8>> {
    let bytes = cubex_protocol::encode(response)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| cubex_protocol::ProtocolError::FrameTooLarge(u32::MAX))?;
    if len > cubex_protocol::MAX_FRAME_SIZE {
        return Err(cubex_protocol::ProtocolError::FrameTooLarge(len));
    }
    Ok(bytes)
}

pub fn plugin_error_text(err: anyhow::Error) -> String {
    normalize_error_text(err.to_string())
}

pub fn normalize_response_error(response: &mut PluginResponse) {
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

#[unsafe(no_mangle)]
pub extern "C" fn cubex_plugin_alloc(len: i32) -> i32 {
    if len <= 0 {
        return 0;
    }
    let mut bytes = Vec::<u8>::with_capacity(len as usize);
    let ptr = bytes.as_mut_ptr() as i32;
    std::mem::forget(bytes);
    ptr
}

/// # Safety
///
/// `ptr` and `len` must come from `cubex_plugin_alloc` and must not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cubex_plugin_free(ptr: i32, len: i32) {
    if ptr != 0 && len > 0 {
        drop(unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) });
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[link(wasm_import_module = "cubex")]
unsafe extern "C" {
    #[link_name = "host_call"]
    fn cubex_host_call(ptr: i32, len: i32) -> i64;
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn host_call(request: HostRequest) -> anyhow::Result<HostPayload> {
    let input = cubex_protocol::encode(&request)?.into_boxed_slice();
    let len = i32::try_from(input.len())
        .map_err(|_| cubex_protocol::ProtocolError::FrameTooLarge(u32::MAX))?;
    let packed = unsafe { cubex_host_call(input.as_ptr() as i32, len) } as u64;
    let output_ptr = (packed & u64::from(u32::MAX)) as u32;
    let output_len = (packed >> 32) as u32;
    if output_len > MAX_FRAME_SIZE {
        anyhow::bail!("{}", ProtocolError::FrameTooLarge(output_len));
    }
    if output_ptr == 0 || output_len == 0 {
        anyhow::bail!("host call returned an empty response");
    }
    let output = unsafe {
        Vec::from_raw_parts(
            output_ptr as *mut u8,
            output_len as usize,
            output_len as usize,
        )
    };
    let response: HostResponse = cubex_protocol::decode(&output)?;
    if let Some(error) = response.error {
        anyhow::bail!(error);
    }
    Ok(response.payload)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn host_call(_request: HostRequest) -> anyhow::Result<HostPayload> {
    anyhow::bail!("host calls require the CubeX wasm runtime")
}

pub fn read_file(path: impl Into<String>) -> anyhow::Result<Vec<u8>> {
    match host_call(HostRequest::FileRead { path: path.into() })? {
        HostPayload::Bytes(bytes) => Ok(bytes),
        _ => anyhow::bail!("host returned non-bytes file response"),
    }
}

pub fn write_file(path: impl Into<String>, bytes: Vec<u8>) -> anyhow::Result<()> {
    match host_call(HostRequest::FileWrite {
        path: path.into(),
        bytes,
    })? {
        HostPayload::Unit => Ok(()),
        _ => anyhow::bail!("host returned non-unit file response"),
    }
}

pub fn tcp_request(
    addr: impl Into<String>,
    bytes: Vec<u8>,
    timeout_ms: u64,
) -> anyhow::Result<Vec<u8>> {
    match host_call(HostRequest::TcpRequest {
        addr: addr.into(),
        bytes,
        timeout_ms,
    })? {
        HostPayload::Bytes(bytes) => Ok(bytes),
        _ => anyhow::bail!("host returned non-bytes tcp response"),
    }
}

pub fn tcp_echo(addr: impl Into<String>, max_connections: u64) -> anyhow::Result<String> {
    match host_call(HostRequest::TcpEcho {
        addr: addr.into(),
        max_connections,
    })? {
        HostPayload::Text(addr) => Ok(addr),
        _ => anyhow::bail!("host returned non-text tcp response"),
    }
}

pub fn sleep_ms(millis: u64) -> anyhow::Result<()> {
    match host_call(HostRequest::Sleep { millis })? {
        HostPayload::Unit => Ok(()),
        _ => anyhow::bail!("host returned non-unit sleep response"),
    }
}

pub fn random_bytes(len: u32) -> anyhow::Result<Vec<u8>> {
    match host_call(HostRequest::RandomBytes { len })? {
        HostPayload::Bytes(bytes) => Ok(bytes),
        _ => anyhow::bail!("host returned non-bytes random response"),
    }
}

pub fn record_put(
    path: impl Into<String>,
    key: impl Into<String>,
    message: Message,
) -> anyhow::Result<()> {
    match host_call(HostRequest::RecordPut {
        path: path.into(),
        key: key.into(),
        message,
    })? {
        HostPayload::Unit => Ok(()),
        _ => anyhow::bail!("host returned non-unit record response"),
    }
}

pub fn record_get(
    path: impl Into<String>,
    key: impl Into<String>,
) -> anyhow::Result<Option<Message>> {
    match host_call(HostRequest::RecordGet {
        path: path.into(),
        key: key.into(),
    })? {
        HostPayload::Message(message) => Ok(message),
        _ => anyhow::bail!("host returned non-message record response"),
    }
}

pub fn record_delete(path: impl Into<String>, key: impl Into<String>) -> anyhow::Result<bool> {
    match host_call(HostRequest::RecordDelete {
        path: path.into(),
        key: key.into(),
    })? {
        HostPayload::Bool(deleted) => Ok(deleted),
        _ => anyhow::bail!("host returned non-bool record response"),
    }
}

pub fn record_list(path: impl Into<String>) -> anyhow::Result<Vec<String>> {
    match host_call(HostRequest::RecordList { path: path.into() })? {
        HostPayload::StringList(keys) => Ok(keys),
        _ => anyhow::bail!("host returned non-list record response"),
    }
}

#[macro_export]
macro_rules! export_plugin {
    ($plugin:expr) => {
        std::thread_local! {
            static CUBEX_PLUGIN: std::cell::RefCell<Box<dyn $crate::Plugin>> =
                std::cell::RefCell::new(Box::new($plugin));
        }

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn cubex_plugin_handle(ptr: i32, len: i32) -> i64 {
            let response = if len <= 0 {
                $crate::PluginResponse {
                    error: Some("invalid request length".into()),
                    ..$crate::PluginResponse::default()
                }
            } else if len as u32 > $crate::MAX_FRAME_SIZE {
                $crate::PluginResponse {
                    error: Some($crate::plugin_error_text(
                        $crate::ProtocolError::FrameTooLarge(len as u32).into(),
                    )),
                    ..$crate::PluginResponse::default()
                }
            } else {
                let input = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
                match $crate::decode_request(input) {
                    Ok(request) => CUBEX_PLUGIN.with(|plugin| {
                        let mut plugin = plugin.borrow_mut();
                        match plugin.handle(request) {
                            Ok(mut response) => {
                                $crate::normalize_response_error(&mut response);
                                response
                            }
                            Err(err) => $crate::PluginResponse {
                                error: Some($crate::plugin_error_text(err)),
                                ..$crate::PluginResponse::default()
                            },
                        }
                    }),
                    Err(err) => $crate::PluginResponse {
                        error: Some($crate::plugin_error_text(err.into())),
                        ..$crate::PluginResponse::default()
                    },
                }
            };
            let output = match $crate::encode_response(&response) {
                Ok(output) => output,
                Err(err) => {
                    let response = $crate::PluginResponse {
                        error: Some($crate::plugin_error_text(err.into())),
                        ..$crate::PluginResponse::default()
                    };
                    $crate::encode_response(&response).unwrap_or_default()
                }
            }
            .into_boxed_slice();
            let len = output.len();
            let ptr = Box::into_raw(output) as *mut u8 as i32;
            ((len as i64) << 32) | (ptr as u32 as i64)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_is_normalized() {
        assert_eq!(plugin_error_text(anyhow::anyhow!(" boom ")), "boom");
        assert_eq!(plugin_error_text(anyhow::anyhow!("")), "plugin error");
    }

    #[test]
    fn manual_error_drops_messages() {
        let mut response = PluginResponse {
            messages: vec![Message::new("plugin", "late", Payload::Text("bad".into()))],
            error: Some(" boom ".into()),
            ..PluginResponse::default()
        };

        normalize_response_error(&mut response);

        assert_eq!(response.error.as_deref(), Some("boom"));
        assert!(response.messages.is_empty());
    }
}
