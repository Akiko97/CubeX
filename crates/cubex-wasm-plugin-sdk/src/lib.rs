pub use cubex_protocol::{
    Control, MAX_FRAME_SIZE, Message, Payload, PluginRequest, PluginResponse, ProtocolError, Value,
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

#[macro_export]
macro_rules! export_plugin {
    ($plugin:expr) => {
        std::thread_local! {
            static CUBEX_PLUGIN: std::cell::RefCell<Box<dyn $crate::Plugin>> =
                std::cell::RefCell::new(Box::new($plugin));
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

        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn cubex_plugin_free(ptr: i32, len: i32) {
            if ptr != 0 && len > 0 {
                drop(unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) });
            }
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
