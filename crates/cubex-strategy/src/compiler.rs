use crate::ast::{
    CapabilityDecl, Expr, FieldPath, Literal, PluginDecl, PluginKind, RouteDecl, Strategy,
};
use crate::error::{Result, StrategyError};
use crate::parser::parse_str;
use cubex_core::{
    CapabilityConfig, Config, EngineConfig, PluginConfig, RouteConfig, RouteValue, StoreConfig,
};
use cubex_protocol::PayloadKind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn compile_file(path: impl AsRef<Path>) -> Result<Config> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|source| StrategyError::ReadFile {
        path: path.to_path_buf(),
        source,
    })?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let base_dir = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(StrategyError::CurrentDir)?
            .join(parent)
    };
    compile_str_with_base(&text, &base_dir)
}

pub fn compile_str(input: &str) -> Result<Config> {
    let strategy = parse_str(input)?;
    compile_strategy(strategy, None)
}

pub fn compile_str_with_base(input: &str, base_dir: impl AsRef<Path>) -> Result<Config> {
    let strategy = parse_str(input)?;
    compile_strategy(strategy, Some(base_dir.as_ref()))
}

fn compile_strategy(strategy: Strategy, base_dir: Option<&Path>) -> Result<Config> {
    let engine_decl = strategy.engine.unwrap_or_default();
    let engine = EngineConfig {
        name: engine_decl.name.unwrap_or(strategy.name),
        max_messages: engine_decl
            .max_messages
            .unwrap_or_else(|| EngineConfig::default().max_messages),
    };

    let store_decl = strategy.store.unwrap_or_default();
    let store = StoreConfig {
        path: store_decl.path.map(PathBuf::from),
        replay_on_start: store_decl.replay_on_start.unwrap_or(false),
    };

    let mut symbols = BTreeSet::new();
    let mut plugin_names = BTreeSet::new();
    let mut plugins = Vec::new();
    for plugin in strategy.plugins {
        insert_symbol(&mut symbols, "binding", &plugin.name)?;
        plugin_names.insert(plugin.name.clone());
        plugins.push(compile_plugin(plugin)?);
    }

    let mut lets = BTreeMap::new();
    for declaration in strategy.lets {
        insert_symbol(&mut symbols, "binding", &declaration.name)?;
        if lets
            .insert(declaration.name.clone(), declaration.expr)
            .is_some()
        {
            return Err(StrategyError::Compile(format!(
                "predicate `{}` declared more than once",
                declaration.name
            )));
        }
    }

    let mut routes = Vec::new();
    for route in strategy.routes {
        insert_symbol(&mut symbols, "binding", &route.name)?;
        routes.push(compile_route(route, &lets, &plugin_names, &engine.name)?);
    }

    let mut config = Config {
        engine,
        store,
        plugins,
        routes,
    };
    if let Some(base_dir) = base_dir {
        config.resolve_relative_paths(base_dir);
    }
    Ok(config)
}

fn insert_symbol(symbols: &mut BTreeSet<String>, label: &str, name: &str) -> Result<()> {
    if !symbols.insert(name.to_string()) {
        return Err(StrategyError::Compile(format!(
            "{label} `{name}` declared more than once"
        )));
    }
    Ok(())
}

fn compile_plugin(plugin: PluginDecl) -> Result<PluginConfig> {
    match plugin.kind {
        PluginKind::Process { command } => {
            if !plugin.capabilities.is_empty() {
                return Err(StrategyError::Compile(format!(
                    "process plugin `{}` cannot declare capabilities",
                    plugin.name
                )));
            }
            Ok(PluginConfig {
                name: plugin.name,
                command: PathBuf::from(command),
                wasm: None,
                working_dir: plugin.working_dir.map(PathBuf::from),
                args: plugin.args,
                autostart: plugin.autostart,
                capabilities: Vec::new(),
            })
        }
        PluginKind::Wasm { path } => Ok(PluginConfig {
            name: plugin.name,
            command: PathBuf::new(),
            wasm: Some(PathBuf::from(path)),
            working_dir: plugin.working_dir.map(PathBuf::from),
            args: plugin.args,
            autostart: plugin.autostart,
            capabilities: plugin
                .capabilities
                .into_iter()
                .map(compile_capability)
                .collect(),
        }),
    }
}

fn compile_capability(capability: CapabilityDecl) -> CapabilityConfig {
    match capability {
        CapabilityDecl::FileRead(path) => CapabilityConfig::FileRead {
            path: PathBuf::from(path),
        },
        CapabilityDecl::FileWrite(path) => CapabilityConfig::FileWrite {
            path: PathBuf::from(path),
        },
        CapabilityDecl::TcpConnect(addr) => CapabilityConfig::TcpConnect { addr },
        CapabilityDecl::TcpListen(addr) => CapabilityConfig::TcpListen { addr },
        CapabilityDecl::Timer => CapabilityConfig::Timer,
        CapabilityDecl::RecordStore(path) => CapabilityConfig::RecordStore {
            path: PathBuf::from(path),
        },
    }
}

fn compile_route(
    route: RouteDecl,
    lets: &BTreeMap<String, Expr>,
    plugin_names: &BTreeSet<String>,
    engine_name: &str,
) -> Result<RouteConfig> {
    if route.targets.is_empty() {
        return Err(StrategyError::Compile(format!(
            "route `{}` must have at least one target",
            route.name
        )));
    }

    let mut targets = BTreeSet::new();
    for target in &route.targets {
        if !targets.insert(target.as_str()) {
            return Err(StrategyError::Compile(format!(
                "route `{}` targets `{}` more than once",
                route.name, target
            )));
        }
        if !plugin_names.contains(target) {
            return Err(StrategyError::Compile(format!(
                "route `{}` targets unknown plugin `{}`",
                route.name, target
            )));
        }
    }

    let mut stack = BTreeSet::new();
    let filter = compile_expr(&route.expr, lets, plugin_names, engine_name, &mut stack)?;
    filter.into_route(route.name, route.targets)
}

fn compile_expr(
    expr: &Expr,
    lets: &BTreeMap<String, Expr>,
    plugin_names: &BTreeSet<String>,
    engine_name: &str,
    stack: &mut BTreeSet<String>,
) -> Result<RouteFilter> {
    match expr {
        Expr::And(parts) => {
            let mut filter = RouteFilter::default();
            for part in parts {
                filter.merge(compile_expr(part, lets, plugin_names, engine_name, stack)?)?;
            }
            Ok(filter)
        }
        Expr::Comparison { field, value } => {
            compile_comparison(field, value, plugin_names, engine_name)
        }
        Expr::Ref(name) => {
            if !stack.insert(name.clone()) {
                return Err(StrategyError::Compile(format!(
                    "predicate reference cycle includes `{name}`"
                )));
            }
            let expr = lets.get(name).ok_or_else(|| {
                StrategyError::Compile(format!("unknown predicate reference `{name}`"))
            })?;
            let filter = compile_expr(expr, lets, plugin_names, engine_name, stack);
            stack.remove(name);
            filter
        }
    }
}

fn compile_comparison(
    field: &FieldPath,
    value: &Literal,
    plugin_names: &BTreeSet<String>,
    engine_name: &str,
) -> Result<RouteFilter> {
    let mut filter = RouteFilter::default();
    match field {
        FieldPath::Source => {
            let source = match value {
                Literal::Ident(value) | Literal::String(value) => value.clone(),
                _ => {
                    return Err(StrategyError::Compile(
                        "source comparisons require a plugin identifier or string".into(),
                    ));
                }
            };
            if source != engine_name && !plugin_names.contains(&source) {
                return Err(StrategyError::Compile(format!(
                    "source comparison references unknown plugin or engine `{source}`"
                )));
            }
            filter.source = Some(source);
        }
        FieldPath::Topic => {
            let Literal::String(topic) = value else {
                return Err(StrategyError::Compile(
                    "topic comparisons require a string literal".into(),
                ));
            };
            filter.topic = Some(topic.clone());
        }
        FieldPath::Payload => {
            let Literal::Payload(payload) = value else {
                return Err(StrategyError::Compile(
                    "payload comparisons require a payload kind".into(),
                ));
            };
            filter.payload = Some(*payload);
        }
        FieldPath::Record(key) => {
            let value = match value {
                Literal::String(value) => RouteValue::String(value.clone()),
                Literal::Bool(value) => RouteValue::Bool(*value),
                Literal::I64(value) => RouteValue::I64(*value),
                Literal::Ident(value) => {
                    return Err(StrategyError::Compile(format!(
                        "record field `{key}` comparison value `{value}` must be quoted"
                    )));
                }
                Literal::Payload(_) => {
                    return Err(StrategyError::Compile(format!(
                        "record field `{key}` cannot compare against a payload kind"
                    )));
                }
            };
            filter.record.insert(key.clone(), value);
        }
    }
    Ok(filter)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RouteFilter {
    source: Option<String>,
    topic: Option<String>,
    payload: Option<PayloadKind>,
    record: BTreeMap<String, RouteValue>,
}

impl RouteFilter {
    fn merge(&mut self, other: Self) -> Result<()> {
        merge_option("source", &mut self.source, other.source)?;
        merge_option("topic", &mut self.topic, other.topic)?;
        merge_option("payload", &mut self.payload, other.payload)?;
        for (key, value) in other.record {
            if let Some(existing) = self.record.get(&key)
                && existing != &value
            {
                return Err(StrategyError::Compile(format!(
                    "conflicting record field predicate for `{key}`"
                )));
            }
            self.record.insert(key, value);
        }
        Ok(())
    }

    fn into_route(mut self, name: String, to: Vec<String>) -> Result<RouteConfig> {
        if !self.record.is_empty() {
            match self.payload {
                None => self.payload = Some(PayloadKind::Record),
                Some(PayloadKind::Record) => {}
                Some(payload) => {
                    return Err(StrategyError::Compile(format!(
                        "route `{name}` has record predicates but payload is `{}`",
                        payload_label(payload)
                    )));
                }
            }
        }
        Ok(RouteConfig {
            name,
            source: self.source,
            topic: self.topic,
            payload: self.payload,
            record: self.record,
            to,
        })
    }
}

fn merge_option<T: Eq + std::fmt::Debug>(
    field: &str,
    target: &mut Option<T>,
    incoming: Option<T>,
) -> Result<()> {
    let Some(incoming) = incoming else {
        return Ok(());
    };
    if let Some(existing) = target.as_ref()
        && existing != &incoming
    {
        return Err(StrategyError::Compile(format!(
            "conflicting `{field}` predicates: {existing:?} vs {incoming:?}"
        )));
    }
    *target = Some(incoming);
    Ok(())
}

fn payload_label(payload: PayloadKind) -> &'static str {
    match payload {
        PayloadKind::Control => "control",
        PayloadKind::Text => "text",
        PayloadKind::Bytes => "bytes",
        PayloadKind::Record => "record",
    }
}
