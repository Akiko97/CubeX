use crate::{Error, Result};
use cubex_protocol::PayloadKind;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub plugins: Vec<PluginConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&text)?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let base_dir = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir()?.join(parent)
        };
        config.resolve_relative_paths(&base_dir);
        Ok(config)
    }

    pub(crate) fn resolve_relative_paths(&mut self, base_dir: &Path) {
        for plugin in &mut self.plugins {
            if !plugin.command.as_os_str().is_empty()
                && !path_is_blank(&plugin.command)
                && plugin.command.is_relative()
            {
                plugin.command = base_dir.join(&plugin.command);
            }
            if let Some(wasm) = &mut plugin.wasm
                && !wasm.as_os_str().is_empty()
                && !path_is_blank(wasm)
                && wasm.is_relative()
            {
                *wasm = base_dir.join(&wasm);
            }
            if let Some(working_dir) = &mut plugin.working_dir
                && !working_dir.as_os_str().is_empty()
                && !path_is_blank(working_dir)
                && working_dir.is_relative()
            {
                *working_dir = base_dir.join(&working_dir);
            }
        }
        if let Some(path) = &mut self.store.path
            && !path.as_os_str().is_empty()
            && !path_is_blank(path)
            && path.is_relative()
        {
            *path = base_dir.join(&path);
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EngineConfig {
    pub name: String,
    pub max_messages: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            name: "cubex".into(),
            max_messages: 1024,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StoreConfig {
    pub path: Option<PathBuf>,
    pub replay_on_start: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConfig {
    pub name: String,
    #[serde(default)]
    pub command: PathBuf,
    #[serde(default)]
    pub wasm: Option<PathBuf>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub autostart: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteConfig {
    pub name: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub payload: Option<PayloadKind>,
    #[serde(default)]
    pub record: BTreeMap<String, RouteValue>,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum RouteValue {
    Bool(bool),
    I64(i64),
    String(String),
}

pub(crate) fn validate_config(config: &Config) -> Result<()> {
    if config.engine.name.trim().is_empty() {
        return Err(Error::InvalidConfig("engine.name must not be empty".into()));
    }
    if has_edge_whitespace(&config.engine.name) {
        return Err(Error::InvalidConfig(
            "engine.name must not have leading or trailing whitespace".into(),
        ));
    }
    if config.engine.max_messages == 0 {
        return Err(Error::InvalidConfig(
            "engine.max_messages must be positive".into(),
        ));
    }
    if config.store.replay_on_start && config.store.path.is_none() {
        return Err(Error::InvalidConfig(
            "store.replay_on_start requires store.path".into(),
        ));
    }
    if config
        .store
        .path
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(Error::InvalidConfig("store.path must not be empty".into()));
    }
    if config
        .store
        .path
        .as_ref()
        .is_some_and(|path| path.to_string_lossy().trim().is_empty())
    {
        return Err(Error::InvalidConfig("store.path must not be blank".into()));
    }

    let mut names = BTreeSet::new();
    for plugin in &config.plugins {
        if plugin.name.trim().is_empty() {
            return Err(Error::InvalidConfig("plugin.name must not be empty".into()));
        }
        if has_edge_whitespace(&plugin.name) {
            return Err(Error::InvalidConfig(format!(
                "plugin `{}` name must not have leading or trailing whitespace",
                plugin.name
            )));
        }
        if plugin.name == config.engine.name {
            return Err(Error::InvalidConfig(format!(
                "plugin `{}` must not use engine.name",
                plugin.name
            )));
        }
        if !plugin.command.as_os_str().is_empty() && path_is_blank(&plugin.command) {
            return Err(Error::InvalidConfig(
                "plugin.command must not be blank".into(),
            ));
        }
        if plugin
            .wasm
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(Error::InvalidConfig("plugin.wasm must not be empty".into()));
        }
        if plugin.wasm.as_ref().is_some_and(|path| path_is_blank(path)) {
            return Err(Error::InvalidConfig("plugin.wasm must not be blank".into()));
        }
        let has_command = !plugin.command.as_os_str().is_empty();
        let has_wasm = plugin.wasm.is_some();
        if !has_command && !has_wasm {
            return Err(Error::InvalidConfig(
                "plugin.command or plugin.wasm must be set".into(),
            ));
        }
        if has_command && has_wasm {
            return Err(Error::InvalidConfig(
                "plugin.command and plugin.wasm are mutually exclusive".into(),
            ));
        }
        if plugin
            .working_dir
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(Error::InvalidConfig(
                "plugin.working_dir must not be empty".into(),
            ));
        }
        if plugin
            .working_dir
            .as_ref()
            .is_some_and(|path| path.to_string_lossy().trim().is_empty())
        {
            return Err(Error::InvalidConfig(
                "plugin.working_dir must not be blank".into(),
            ));
        }
        if !names.insert(plugin.name.as_str()) {
            return Err(Error::DuplicatePlugin(plugin.name.clone()));
        }
    }

    let mut route_names = BTreeSet::new();
    for route in &config.routes {
        if route.name.trim().is_empty() {
            return Err(Error::InvalidConfig("route.name must not be empty".into()));
        }
        if has_edge_whitespace(&route.name) {
            return Err(Error::InvalidConfig(format!(
                "route `{}` name must not have leading or trailing whitespace",
                route.name
            )));
        }
        if !route_names.insert(route.name.as_str()) {
            return Err(Error::InvalidConfig(format!(
                "route `{}` is configured more than once",
                route.name
            )));
        }
        if route
            .source
            .as_ref()
            .is_some_and(|source| source.trim().is_empty())
        {
            return Err(Error::InvalidConfig(format!(
                "route `{}` source must not be empty",
                route.name
            )));
        }
        if route
            .source
            .as_ref()
            .is_some_and(|source| has_edge_whitespace(source))
        {
            return Err(Error::InvalidConfig(format!(
                "route `{}` source must not have leading or trailing whitespace",
                route.name
            )));
        }
        if let Some(source) = &route.source
            && source != &config.engine.name
            && !names.contains(source.as_str())
        {
            return Err(Error::UnknownRouteSource {
                route: route.name.clone(),
                source_name: source.clone(),
            });
        }
        if route
            .topic
            .as_ref()
            .is_some_and(|topic| topic.trim().is_empty())
        {
            return Err(Error::InvalidConfig(format!(
                "route `{}` topic must not be empty",
                route.name
            )));
        }
        if route
            .topic
            .as_ref()
            .is_some_and(|topic| has_edge_whitespace(topic))
        {
            return Err(Error::InvalidConfig(format!(
                "route `{}` topic must not have leading or trailing whitespace",
                route.name
            )));
        }
        if !route.record.is_empty()
            && route
                .payload
                .is_some_and(|payload| payload != PayloadKind::Record)
        {
            return Err(Error::InvalidConfig(format!(
                "route `{}` record match requires payload `record` or no payload filter",
                route.name
            )));
        }
        for key in route.record.keys() {
            if key.trim().is_empty() {
                return Err(Error::InvalidConfig(format!(
                    "route `{}` record key must not be empty",
                    route.name
                )));
            }
            if has_edge_whitespace(key) {
                return Err(Error::InvalidConfig(format!(
                    "route `{}` record key must not have leading or trailing whitespace",
                    route.name
                )));
            }
        }
        if route.to.is_empty() {
            return Err(Error::EmptyRouteTargets(route.name.clone()));
        }
        let mut targets = BTreeSet::new();
        for target in &route.to {
            if target.trim().is_empty() {
                return Err(Error::InvalidConfig(format!(
                    "route `{}` target must not be empty",
                    route.name
                )));
            }
            if has_edge_whitespace(target) {
                return Err(Error::InvalidConfig(format!(
                    "route `{}` target must not have leading or trailing whitespace",
                    route.name
                )));
            }
            if !targets.insert(target.as_str()) {
                return Err(Error::InvalidConfig(format!(
                    "route `{}` targets `{}` more than once",
                    route.name, target
                )));
            }
            if !names.contains(target.as_str()) {
                return Err(Error::UnknownRouteTarget {
                    route: route.name.clone(),
                    target: target.clone(),
                });
            }
        }
    }

    Ok(())
}

pub(crate) fn has_edge_whitespace(value: &str) -> bool {
    value.trim() != value
}

fn path_is_blank(path: &Path) -> bool {
    path.to_string_lossy().trim().is_empty()
}
