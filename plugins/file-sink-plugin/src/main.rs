use cubex_plugin_sdk::{Control, Message, Payload, Plugin, PluginRequest, PluginResponse};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct FileSinkPlugin {
    output: Option<PathBuf>,
}

impl Plugin for FileSinkPlugin {
    fn handle(&mut self, request: PluginRequest) -> anyhow::Result<PluginResponse> {
        match request.message.payload {
            Payload::Control(Control::Start { args }) => {
                if args.len() > 1 {
                    anyhow::bail!("file sink accepts at most 1 arg");
                }
                let output = args
                    .first()
                    .map(PathBuf::from)
                    .ok_or_else(|| anyhow::anyhow!("file sink needs output path arg 0"))?;
                if output.as_os_str().is_empty() {
                    anyhow::bail!("file sink output path must not be empty");
                }
                if output.to_string_lossy().trim().is_empty() {
                    anyhow::bail!("file sink output path must not be blank");
                }
                self.output = Some(output);
                Ok(PluginResponse::default())
            }
            Payload::Text(text) => self.write(request.plugin, text.into_bytes()),
            Payload::Bytes(bytes) => self.write(request.plugin, bytes),
            _ => Ok(PluginResponse::default()),
        }
    }
}

impl FileSinkPlugin {
    fn write(&self, plugin: String, bytes: Vec<u8>) -> anyhow::Result<PluginResponse> {
        let output = self
            .output
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("file sink was not started"))?;
        if let Some(parent) = output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        write_file(output, &bytes)?;

        Ok(PluginResponse {
            messages: vec![Message::new(
                plugin,
                "file.written",
                Payload::Text(output.to_string_lossy().into_owned()),
            )],
            logs: Vec::new(),
            error: None,
        })
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tmp = write_temp_file(path, bytes)?;
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

fn write_temp_file(path: &Path, bytes: &[u8]) -> anyhow::Result<PathBuf> {
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
    anyhow::bail!("could not create file sink temporary file")
}

fn temp_file_path(path: &Path, attempt: u16) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("cubex-file-sink")),
    );
    name.push(format!(".{attempt}.tmp"));
    path.with_file_name(name)
}

fn main() -> anyhow::Result<()> {
    cubex_plugin_sdk::run_stdio(FileSinkPlugin::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_text_after_start() {
        let path = std::env::temp_dir().join(format!("cubex-file-sink-{}.txt", unique_id()));
        let mut plugin = FileSinkPlugin::default();

        plugin
            .handle(PluginRequest {
                plugin: "file-sink".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec![path.to_string_lossy().into_owned()],
                    }),
                ),
            })
            .unwrap();
        let response = plugin
            .handle(PluginRequest {
                plugin: "file-sink".into(),
                message: Message::new("source", "file.read", Payload::Text("hello".into())),
            })
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert_eq!(response.messages[0].topic, "file.written");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_plain_relative_file() {
        let path = PathBuf::from(format!("cubex-file-sink-{}.txt", unique_id()));
        let mut plugin = FileSinkPlugin {
            output: Some(path.clone()),
        };

        plugin
            .handle(PluginRequest {
                plugin: "file-sink".into(),
                message: Message::new("source", "file.read", Payload::Text("hello".into())),
            })
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn writes_bytes_payload() {
        let path = std::env::temp_dir().join(format!("cubex-file-sink-{}.bin", unique_id()));
        let mut plugin = FileSinkPlugin {
            output: Some(path.clone()),
        };

        plugin
            .handle(PluginRequest {
                plugin: "file-sink".into(),
                message: Message::new("source", "file.bytes", Payload::Bytes(vec![0, 1, 255])),
            })
            .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), vec![0, 1, 255]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn does_not_overwrite_existing_temp_file() {
        let path = std::env::temp_dir().join(format!("cubex-file-sink-tmp-{}.txt", unique_id()));
        let stale_tmp = temp_file_path(&path, 0);
        std::fs::write(&stale_tmp, b"stale").unwrap();
        let mut plugin = FileSinkPlugin {
            output: Some(path.clone()),
        };

        plugin
            .handle(PluginRequest {
                plugin: "file-sink".into(),
                message: Message::new("source", "file.read", Payload::Text("fresh".into())),
            })
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh");
        assert_eq!(std::fs::read(&stale_tmp).unwrap(), b"stale");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(stale_tmp);
    }

    #[test]
    fn rejects_empty_output_path() {
        let error = FileSinkPlugin::default()
            .handle(PluginRequest {
                plugin: "file-sink".into(),
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

        assert_eq!(error, "file sink output path must not be empty");
    }

    #[test]
    fn rejects_blank_output_path() {
        let error = FileSinkPlugin::default()
            .handle(PluginRequest {
                plugin: "file-sink".into(),
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

        assert_eq!(error, "file sink output path must not be blank");
    }

    #[test]
    fn rejects_extra_args() {
        let error = FileSinkPlugin::default()
            .handle(PluginRequest {
                plugin: "file-sink".into(),
                message: Message::new(
                    "engine",
                    "system.start",
                    Payload::Control(Control::Start {
                        args: vec!["output.txt".into(), "ignored".into()],
                    }),
                ),
            })
            .unwrap_err()
            .to_string();

        assert_eq!(error, "file sink accepts at most 1 arg");
    }

    fn unique_id() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }
}
