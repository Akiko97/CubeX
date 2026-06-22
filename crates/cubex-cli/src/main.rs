use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(version, about = "CubeX runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run {
        #[arg(short, long, default_value = "cubex.toml")]
        config: PathBuf,
        #[arg(long)]
        strict: bool,
    },
    Check {
        #[arg(short, long, default_value = "cubex.toml")]
        config: PathBuf,
        #[arg(long)]
        strict: bool,
    },
    Events {
        path: PathBuf,
    },
    Records {
        path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Run { config, strict } => {
            let config = cubex_core::Config::from_file(config)?;
            let engine = cubex_core::Engine::from_config(config.clone())?;
            if strict {
                check_runtime_files(&config)?;
            }
            let report = engine.run()?;
            for line in report.logs {
                println!("{line}");
            }
            println!(
                "started={}, replayed={}, emitted={}, delivered={}",
                report.started.len(),
                report.replayed,
                report.emitted.len(),
                report.delivered.len()
            );
        }
        Command::Check { config, strict } => {
            let config = cubex_core::Config::from_file(config)?;
            let _ = cubex_core::Engine::from_config(config.clone())?;
            if strict {
                check_runtime_files(&config)?;
            }
            println!("ok");
        }
        Command::Events { path } => {
            require_existing_file(&path, "event log")?;
            for (index, message) in cubex_store::EventLog::new(path)
                .read_all()?
                .iter()
                .enumerate()
            {
                println!("{}", format_event(index, message));
            }
        }
        Command::Records { path } => {
            require_existing_file(&path, "record store")?;
            for record in cubex_store::RecordStore::new(path).load()?.values() {
                println!("{}", format_record(record));
            }
        }
    }
    Ok(())
}

fn format_event(index: usize, message: &cubex_protocol::Message) -> String {
    format!(
        "{index}\t{}\t{}\t{}\t{:?}",
        message.id, message.source, message.topic, message.payload
    )
}

fn format_record(record: &cubex_store::StoredRecord) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{:?}",
        record.key,
        record.updated_at_unix_ms,
        record.message.source,
        record.message.topic,
        record.message.payload
    )
}

fn require_existing_file(path: &Path, label: &str) -> anyhow::Result<()> {
    if !path.exists() {
        anyhow::bail!("{label} does not exist: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!("{label} is not a file: {}", path.display());
    }
    Ok(())
}

fn check_runtime_files(config: &cubex_core::Config) -> anyhow::Result<()> {
    if let Some(path) = &config.store.path {
        if path.is_dir() {
            anyhow::bail!("store.path is a directory: {}", path.display());
        }
        if path.exists() {
            if !path.is_file() {
                anyhow::bail!("store.path is not a file: {}", path.display());
            }
            cubex_store::EventLog::new(path)
                .read_all()
                .with_context(|| {
                    format!("store.path is not a valid event log: {}", path.display())
                })?;
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            for ancestor in parent
                .ancestors()
                .filter(|path| !path.as_os_str().is_empty())
            {
                if ancestor.exists() {
                    if !ancestor.is_dir() {
                        anyhow::bail!(
                            "store.path parent is not a directory: {}",
                            ancestor.display()
                        );
                    }
                    break;
                }
            }
        }
    }

    for plugin in &config.plugins {
        if let Some(working_dir) = &plugin.working_dir
            && !working_dir.exists()
        {
            anyhow::bail!(
                "plugin `{}` working_dir does not exist: {}",
                plugin.name,
                working_dir.display()
            );
        }
        if let Some(working_dir) = &plugin.working_dir
            && !working_dir.is_dir()
        {
            anyhow::bail!(
                "plugin `{}` working_dir is not a directory: {}",
                plugin.name,
                working_dir.display()
            );
        }
        if let Some(wasm) = &plugin.wasm {
            if !wasm.exists() {
                anyhow::bail!(
                    "plugin `{}` wasm does not exist: {}",
                    plugin.name,
                    wasm.display()
                );
            }
            if !wasm.is_file() {
                anyhow::bail!(
                    "plugin `{}` wasm is not a file: {}",
                    plugin.name,
                    wasm.display()
                );
            }
            continue;
        }
        if !plugin.command.exists() {
            anyhow::bail!(
                "plugin `{}` command does not exist: {}",
                plugin.name,
                plugin.command.display()
            );
        }
        if !plugin.command.is_file() {
            anyhow::bail!(
                "plugin `{}` command is not a file: {}",
                plugin.name,
                plugin.command.display()
            );
        }
        if !is_executable(&plugin.command)? {
            anyhow::bail!(
                "plugin `{}` command is not executable: {}",
                plugin.name,
                plugin.command.display()
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> anyhow::Result<bool> {
    use std::os::unix::fs::PermissionsExt;

    Ok(std::fs::metadata(path)?.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> anyhow::Result<bool> {
    Ok(path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_events_as_tsv_lines() {
        let message = cubex_protocol::Message::new(
            "source",
            "topic",
            cubex_protocol::Payload::Text("hello".into()),
        );

        let line = format_event(7, &message);

        assert!(line.starts_with("7\t"));
        assert!(line.contains("\tsource\ttopic\tText(\"hello\")"));
    }

    #[test]
    fn formats_records_as_tsv_lines() {
        let record = cubex_store::StoredRecord {
            key: "answer".into(),
            updated_at_unix_ms: 42,
            message: cubex_protocol::Message::new(
                "source",
                "record.put",
                cubex_protocol::Payload::Text("hello".into()),
            ),
        };

        let line = format_record(&record);

        assert_eq!(line, "answer\t42\tsource\trecord.put\tText(\"hello\")");
    }

    #[test]
    fn strict_check_rejects_store_path_directory() {
        let path = std::env::temp_dir().join(format!("cubex-store-dir-{}", unique_id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();

        let mut config = cubex_core::Config::default();
        config.store.path = Some(path.clone());

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("store.path is a directory"));

        let _ = std::fs::remove_dir(path);
    }

    #[test]
    fn strict_check_rejects_store_path_file_parent() {
        let parent = std::env::temp_dir().join(format!("cubex-store-parent-{}", unique_id()));
        std::fs::write(&parent, []).unwrap();

        let mut config = cubex_core::Config::default();
        config.store.path = Some(parent.join("events.bin"));

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("store.path parent is not a directory"));

        let _ = std::fs::remove_file(parent);
    }

    #[test]
    fn strict_check_rejects_store_path_file_ancestor() {
        let parent = std::env::temp_dir().join(format!("cubex-store-ancestor-{}", unique_id()));
        std::fs::write(&parent, []).unwrap();

        let mut config = cubex_core::Config::default();
        config.store.path = Some(parent.join("missing").join("events.bin"));

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("store.path parent is not a directory"));

        let _ = std::fs::remove_file(parent);
    }

    #[test]
    fn strict_check_rejects_corrupt_store_path_file() {
        let path = std::env::temp_dir().join(format!("cubex-store-corrupt-{}", unique_id()));
        std::fs::write(&path, [1_u8, 0]).unwrap();

        let mut config = cubex_core::Config::default();
        config.store.path = Some(path.clone());

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("store.path is not a valid event log"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn strict_check_rejects_working_dir_file() {
        let path = std::env::temp_dir().join(format!("cubex-workdir-file-{}", unique_id()));
        std::fs::write(&path, []).unwrap();

        let mut config = cubex_core::Config::default();
        config.plugins.push(cubex_core::PluginConfig {
            name: "plugin".into(),
            command: "unused".into(),
            wasm: None,
            working_dir: Some(path.clone()),
            args: Vec::new(),
            autostart: false,
        });

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("working_dir is not a directory"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn strict_check_rejects_command_directory() {
        let path = std::env::temp_dir().join(format!("cubex-command-dir-{}", unique_id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();

        let mut config = cubex_core::Config::default();
        config.plugins.push(cubex_core::PluginConfig {
            name: "plugin".into(),
            command: path.clone(),
            wasm: None,
            working_dir: None,
            args: Vec::new(),
            autostart: false,
        });

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("command is not a file"));

        let _ = std::fs::remove_dir(path);
    }

    #[test]
    fn strict_check_rejects_missing_wasm() {
        let path = std::env::temp_dir().join(format!("cubex-missing-wasm-{}", unique_id()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&path);

        let mut config = cubex_core::Config::default();
        config.plugins.push(cubex_core::PluginConfig {
            name: "wasm".into(),
            command: PathBuf::new(),
            wasm: Some(path.clone()),
            working_dir: None,
            args: Vec::new(),
            autostart: false,
        });

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("wasm does not exist"));
    }

    #[test]
    fn strict_check_rejects_wasm_directory() {
        let path = std::env::temp_dir().join(format!("cubex-wasm-dir-{}", unique_id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();

        let mut config = cubex_core::Config::default();
        config.plugins.push(cubex_core::PluginConfig {
            name: "wasm".into(),
            command: PathBuf::new(),
            wasm: Some(path.clone()),
            working_dir: None,
            args: Vec::new(),
            autostart: false,
        });

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("wasm is not a file"));

        let _ = std::fs::remove_dir(path);
    }

    #[test]
    fn inspection_requires_existing_file() {
        let path = std::env::temp_dir().join(format!("cubex-missing-{}", unique_id()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&path);

        let error = require_existing_file(&path, "event log")
            .unwrap_err()
            .to_string();

        assert!(error.contains("event log does not exist"));
    }

    #[test]
    fn inspection_rejects_directory() {
        let path = std::env::temp_dir().join(format!("cubex-inspect-dir-{}", unique_id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).unwrap();

        let error = require_existing_file(&path, "record store")
            .unwrap_err()
            .to_string();

        assert!(error.contains("record store is not a file"));
        let _ = std::fs::remove_dir(path);
    }

    #[test]
    fn inspection_accepts_file() {
        let path = std::env::temp_dir().join(format!("cubex-inspect-file-{}", unique_id()));
        std::fs::write(&path, []).unwrap();

        require_existing_file(&path, "event log").unwrap();

        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn strict_check_rejects_non_executable_command() {
        let path = std::env::temp_dir().join(format!("cubex-command-file-{}", unique_id()));
        std::fs::write(&path, []).unwrap();

        let mut config = cubex_core::Config::default();
        config.plugins.push(cubex_core::PluginConfig {
            name: "plugin".into(),
            command: path.clone(),
            wasm: None,
            working_dir: None,
            args: Vec::new(),
            autostart: false,
        });

        let error = check_runtime_files(&config).unwrap_err().to_string();
        assert!(error.contains("command is not executable"));

        let _ = std::fs::remove_file(path);
    }

    fn unique_id() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
            + "-"
            + &std::process::id().to_string()
    }
}
