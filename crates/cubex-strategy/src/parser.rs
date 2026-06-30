use crate::ast::{
    CapabilityDecl, EngineDecl, Expr, FieldPath, LetDecl, Literal, PluginDecl, PluginKind,
    RouteDecl, StoreDecl, Strategy,
};
use crate::error::{Result, StrategyError};
use cubex_protocol::PayloadKind;
use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct StrategyParser;

pub fn parse_str(input: &str) -> Result<Strategy> {
    let mut pairs = StrategyParser::parse(Rule::file, input)
        .map_err(|err| StrategyError::Parse(err.to_string()))?;
    let file = pairs
        .next()
        .ok_or_else(|| StrategyError::Parse("empty parser output".into()))?;
    let strategy_pair = file
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::strategy_decl)
        .ok_or_else(|| StrategyError::Parse("missing strategy declaration".into()))?;
    parse_strategy_decl(strategy_pair)
}

fn parse_strategy_decl(pair: Pair<'_, Rule>) -> Result<Strategy> {
    let mut inner = pair.into_inner();
    let name = parse_string(
        inner
            .next()
            .ok_or_else(|| StrategyError::Parse("missing strategy name".into()))?,
    )?;
    let block = inner
        .next()
        .ok_or_else(|| StrategyError::Parse("missing strategy block".into()))?;

    let mut strategy = Strategy {
        name,
        engine: None,
        store: None,
        plugins: Vec::new(),
        lets: Vec::new(),
        routes: Vec::new(),
    };

    for item in block.into_inner() {
        match item.as_rule() {
            Rule::engine_decl => {
                if strategy.engine.is_some() {
                    return Err(StrategyError::Compile(
                        "engine block declared more than once".into(),
                    ));
                }
                strategy.engine = Some(parse_engine_decl(item)?);
            }
            Rule::store_decl => {
                if strategy.store.is_some() {
                    return Err(StrategyError::Compile(
                        "store block declared more than once".into(),
                    ));
                }
                strategy.store = Some(parse_store_decl(item)?);
            }
            Rule::plugin_decl => strategy.plugins.push(parse_plugin_decl(item)?),
            Rule::let_decl => strategy.lets.push(parse_let_decl(item)?),
            Rule::route_decl => strategy.routes.push(parse_route_decl(item)?),
            _ => {
                return Err(StrategyError::Parse(format!(
                    "unexpected item `{}`",
                    item.as_str()
                )));
            }
        }
    }

    Ok(strategy)
}

fn parse_engine_decl(pair: Pair<'_, Rule>) -> Result<EngineDecl> {
    let mut engine = EngineDecl::default();
    for field in pair.into_inner() {
        match field.as_rule() {
            Rule::engine_name_field => {
                if engine.name.is_some() {
                    return Err(StrategyError::Compile(
                        "engine.name declared more than once".into(),
                    ));
                }
                let value = field
                    .into_inner()
                    .next()
                    .ok_or_else(|| StrategyError::Parse("missing engine.name value".into()))?;
                engine.name = Some(parse_string(value)?);
            }
            Rule::max_messages_field => {
                if engine.max_messages.is_some() {
                    return Err(StrategyError::Compile(
                        "engine.max_messages declared more than once".into(),
                    ));
                }
                let value = field.into_inner().next().ok_or_else(|| {
                    StrategyError::Parse("missing engine.max_messages value".into())
                })?;
                engine.max_messages = Some(parse_usize(value)?);
            }
            _ => {
                return Err(StrategyError::Parse(format!(
                    "unexpected engine field `{}`",
                    field.as_str()
                )));
            }
        }
    }
    Ok(engine)
}

fn parse_store_decl(pair: Pair<'_, Rule>) -> Result<StoreDecl> {
    let mut store = StoreDecl::default();
    for field in pair.into_inner() {
        match field.as_rule() {
            Rule::store_path_field => {
                if store.path.is_some() {
                    return Err(StrategyError::Compile(
                        "store.path declared more than once".into(),
                    ));
                }
                let value = field
                    .into_inner()
                    .next()
                    .ok_or_else(|| StrategyError::Parse("missing store.path value".into()))?;
                store.path = Some(parse_string(value)?);
            }
            Rule::replay_on_start_field => {
                if store.replay_on_start.is_some() {
                    return Err(StrategyError::Compile(
                        "store.replay_on_start declared more than once".into(),
                    ));
                }
                let value = field.into_inner().next().ok_or_else(|| {
                    StrategyError::Parse("missing store.replay_on_start value".into())
                })?;
                store.replay_on_start = Some(parse_bool(value)?);
            }
            _ => {
                return Err(StrategyError::Parse(format!(
                    "unexpected store field `{}`",
                    field.as_str()
                )));
            }
        }
    }
    Ok(store)
}

fn parse_plugin_decl(pair: Pair<'_, Rule>) -> Result<PluginDecl> {
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| StrategyError::Parse("missing plugin name".into()))?
        .as_str()
        .to_string();
    let kind_pair = inner
        .next()
        .ok_or_else(|| StrategyError::Parse(format!("plugin `{name}` missing kind")))?;
    let kind = parse_plugin_kind(kind_pair)?;

    let mut args = None;
    let mut autostart = None;
    let mut working_dir = None;
    let mut capabilities = Vec::new();

    if let Some(body) = inner.next() {
        for field in body.into_inner() {
            match field.as_rule() {
                Rule::args_field => {
                    if args.is_some() {
                        return Err(StrategyError::Compile(format!(
                            "plugin `{name}` declares args more than once"
                        )));
                    }
                    let value = field.into_inner().next().ok_or_else(|| {
                        StrategyError::Parse(format!("plugin `{name}` missing args"))
                    })?;
                    args = Some(parse_string_list(value)?);
                }
                Rule::autostart_field => {
                    if autostart.is_some() {
                        return Err(StrategyError::Compile(format!(
                            "plugin `{name}` declares autostart more than once"
                        )));
                    }
                    let value = field.into_inner().next().ok_or_else(|| {
                        StrategyError::Parse(format!("plugin `{name}` missing autostart"))
                    })?;
                    autostart = Some(parse_bool(value)?);
                }
                Rule::working_dir_field => {
                    if working_dir.is_some() {
                        return Err(StrategyError::Compile(format!(
                            "plugin `{name}` declares working_dir more than once"
                        )));
                    }
                    let value = field.into_inner().next().ok_or_else(|| {
                        StrategyError::Parse(format!("plugin `{name}` missing working_dir"))
                    })?;
                    working_dir = Some(parse_string(value)?);
                }
                Rule::capability_decl => capabilities.push(parse_capability_decl(field)?),
                _ => {
                    return Err(StrategyError::Parse(format!(
                        "unexpected plugin field `{}`",
                        field.as_str()
                    )));
                }
            }
        }
    }

    Ok(PluginDecl {
        name,
        kind,
        args: args.unwrap_or_default(),
        autostart: autostart.unwrap_or(false),
        working_dir,
        capabilities,
    })
}

fn parse_plugin_kind(pair: Pair<'_, Rule>) -> Result<PluginKind> {
    match pair.as_rule() {
        Rule::process_plugin => {
            let path = pair
                .into_inner()
                .next()
                .ok_or_else(|| StrategyError::Parse("process plugin missing command".into()))?;
            Ok(PluginKind::Process {
                command: parse_string(path)?,
            })
        }
        Rule::wasm_plugin => {
            let path = pair
                .into_inner()
                .next()
                .ok_or_else(|| StrategyError::Parse("wasm plugin missing path".into()))?;
            Ok(PluginKind::Wasm {
                path: parse_string(path)?,
            })
        }
        _ => Err(StrategyError::Parse(format!(
            "unexpected plugin kind `{}`",
            pair.as_str()
        ))),
    }
}

fn parse_capability_decl(pair: Pair<'_, Rule>) -> Result<CapabilityDecl> {
    let capability = pair
        .into_inner()
        .next()
        .ok_or_else(|| StrategyError::Parse("missing capability".into()))?;
    match capability.as_rule() {
        Rule::file_read_capability => Ok(CapabilityDecl::FileRead(parse_single_string_arg(
            capability,
            "file_read",
        )?)),
        Rule::file_write_capability => Ok(CapabilityDecl::FileWrite(parse_single_string_arg(
            capability,
            "file_write",
        )?)),
        Rule::tcp_connect_capability => Ok(CapabilityDecl::TcpConnect(parse_single_string_arg(
            capability,
            "tcp_connect",
        )?)),
        Rule::tcp_listen_capability => Ok(CapabilityDecl::TcpListen(parse_single_string_arg(
            capability,
            "tcp_listen",
        )?)),
        Rule::record_store_capability => Ok(CapabilityDecl::RecordStore(parse_single_string_arg(
            capability,
            "record_store",
        )?)),
        Rule::timer_capability => Ok(CapabilityDecl::Timer),
        _ => Err(StrategyError::Parse(format!(
            "unexpected capability `{}`",
            capability.as_str()
        ))),
    }
}

fn parse_single_string_arg(pair: Pair<'_, Rule>, label: &str) -> Result<String> {
    let value = pair
        .into_inner()
        .next()
        .ok_or_else(|| StrategyError::Parse(format!("{label} missing string argument")))?;
    parse_string(value)
}

fn parse_let_decl(pair: Pair<'_, Rule>) -> Result<LetDecl> {
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| StrategyError::Parse("missing let name".into()))?
        .as_str()
        .to_string();
    let expr = inner
        .next()
        .ok_or_else(|| StrategyError::Parse(format!("let `{name}` missing expression")))
        .and_then(parse_expr)?;
    Ok(LetDecl { name, expr })
}

fn parse_route_decl(pair: Pair<'_, Rule>) -> Result<RouteDecl> {
    let mut inner = pair.into_inner();
    let name = inner
        .next()
        .ok_or_else(|| StrategyError::Parse("missing route name".into()))?
        .as_str()
        .to_string();
    let expr = inner
        .next()
        .ok_or_else(|| StrategyError::Parse(format!("route `{name}` missing expression")))
        .and_then(parse_expr)?;
    let targets = inner
        .next()
        .ok_or_else(|| StrategyError::Parse(format!("route `{name}` missing targets")))
        .and_then(parse_target_list)?;
    Ok(RouteDecl {
        name,
        expr,
        targets,
    })
}

fn parse_expr(pair: Pair<'_, Rule>) -> Result<Expr> {
    match pair.as_rule() {
        Rule::expr => {
            let inner = pair
                .into_inner()
                .next()
                .ok_or_else(|| StrategyError::Parse("empty expression".into()))?;
            parse_expr(inner)
        }
        Rule::and_expr => {
            let mut parts = Vec::new();
            for part in pair.into_inner() {
                parts.push(parse_expr(part)?);
            }
            match parts.len() {
                0 => Err(StrategyError::Parse("empty && expression".into())),
                1 => Ok(parts.remove(0)),
                _ => Ok(Expr::And(parts)),
            }
        }
        Rule::comparison => parse_comparison(pair),
        Rule::ref_expr => {
            let name = pair
                .into_inner()
                .next()
                .ok_or_else(|| StrategyError::Parse("missing predicate reference".into()))?
                .as_str()
                .to_string();
            Ok(Expr::Ref(name))
        }
        _ => Err(StrategyError::Parse(format!(
            "unexpected expression `{}`",
            pair.as_str()
        ))),
    }
}

fn parse_comparison(pair: Pair<'_, Rule>) -> Result<Expr> {
    let mut inner = pair.into_inner();
    let field = inner
        .next()
        .ok_or_else(|| StrategyError::Parse("comparison missing field".into()))
        .and_then(parse_field_path)?;
    let value = inner
        .next()
        .ok_or_else(|| StrategyError::Parse("comparison missing value".into()))
        .and_then(parse_literal)?;
    Ok(Expr::Comparison { field, value })
}

fn parse_field_path(pair: Pair<'_, Rule>) -> Result<FieldPath> {
    let text = pair.as_str();
    if text == "source" {
        return Ok(FieldPath::Source);
    }
    if text == "topic" {
        return Ok(FieldPath::Topic);
    }
    if text == "payload" {
        return Ok(FieldPath::Payload);
    }
    if let Some(field) = text.strip_prefix("record.") {
        return Ok(FieldPath::Record(field.to_string()));
    }
    Err(StrategyError::Parse(format!("unknown field path `{text}`")))
}

fn parse_literal(pair: Pair<'_, Rule>) -> Result<Literal> {
    match pair.as_rule() {
        Rule::string => Ok(Literal::String(parse_string(pair)?)),
        Rule::bool => Ok(Literal::Bool(parse_bool(pair)?)),
        Rule::int => Ok(Literal::I64(parse_i64(pair)?)),
        Rule::payload_kind => Ok(Literal::Payload(parse_payload_kind(pair.as_str())?)),
        Rule::ident => Ok(Literal::Ident(pair.as_str().to_string())),
        _ => Err(StrategyError::Parse(format!(
            "unexpected literal `{}`",
            pair.as_str()
        ))),
    }
}

fn parse_string_list(pair: Pair<'_, Rule>) -> Result<Vec<String>> {
    pair.into_inner().map(parse_string).collect()
}

fn parse_target_list(pair: Pair<'_, Rule>) -> Result<Vec<String>> {
    Ok(pair
        .into_inner()
        .map(|target| target.as_str().to_string())
        .collect())
}

fn parse_string(pair: Pair<'_, Rule>) -> Result<String> {
    let raw = pair.as_str();
    let inner = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| StrategyError::Parse(format!("invalid string literal `{raw}`")))?;
    let mut output = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| StrategyError::Parse("unterminated string escape".into()))?;
        match escaped {
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            other => {
                return Err(StrategyError::Parse(format!(
                    "unsupported string escape `\\{other}`"
                )));
            }
        }
    }
    Ok(output)
}

fn parse_bool(pair: Pair<'_, Rule>) -> Result<bool> {
    match pair.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        value => Err(StrategyError::Parse(format!("invalid bool `{value}`"))),
    }
}

fn parse_i64(pair: Pair<'_, Rule>) -> Result<i64> {
    pair.as_str()
        .parse()
        .map_err(|_| StrategyError::Parse(format!("invalid integer `{}`", pair.as_str())))
}

fn parse_usize(pair: Pair<'_, Rule>) -> Result<usize> {
    pair.as_str()
        .parse()
        .map_err(|_| StrategyError::Parse(format!("invalid unsigned integer `{}`", pair.as_str())))
}

fn parse_payload_kind(value: &str) -> Result<PayloadKind> {
    match value {
        "control" => Ok(PayloadKind::Control),
        "text" => Ok(PayloadKind::Text),
        "bytes" => Ok(PayloadKind::Bytes),
        "record" => Ok(PayloadKind::Record),
        _ => Err(StrategyError::Parse(format!(
            "unknown payload kind `{value}`"
        ))),
    }
}
