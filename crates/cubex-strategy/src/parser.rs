use crate::ast::{
    CapabilityDecl, CapabilityKind, EngineDecl, Expr, FieldPath, LetDecl, Literal, PluginDecl,
    PluginKind, RouteDecl, RouteTarget, SourceSpan, Spanned, StoreDecl, Strategy,
};
use crate::error::{Result, StrategyError};
use cubex_protocol::PayloadKind;
use pest::Parser;
use pest::error::InputLocation;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct StrategyParser;

pub fn parse_str(input: &str) -> Result<Strategy> {
    parse_str_with_source(input, None)
}

pub(crate) fn parse_str_with_source(input: &str, source_name: Option<&str>) -> Result<Strategy> {
    let mut pairs = StrategyParser::parse(Rule::file, input).map_err(|err| {
        StrategyError::parse_diagnostic(
            input,
            source_name,
            pest_error_span(input, &err),
            err.variant.message().into_owned(),
        )
    })?;
    let file = pairs
        .next()
        .ok_or_else(|| ParserError::parse(SourceSpan::default(), "empty parser output"))
        .map_err(|err| err.into_strategy_error(input, source_name))?;
    let strategy_pair = file
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::strategy_decl)
        .ok_or_else(|| ParserError::parse(SourceSpan::default(), "missing strategy declaration"))
        .map_err(|err| err.into_strategy_error(input, source_name))?;
    parse_strategy_decl(strategy_pair).map_err(|err| err.into_strategy_error(input, source_name))
}

type ParseResult<T> = std::result::Result<T, ParserError>;

#[derive(Debug)]
struct ParserError {
    kind: ParserErrorKind,
    span: SourceSpan,
    message: String,
}

#[derive(Debug)]
enum ParserErrorKind {
    Parse,
    Compile,
}

impl ParserError {
    fn parse(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            kind: ParserErrorKind::Parse,
            span,
            message: message.into(),
        }
    }

    fn compile(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            kind: ParserErrorKind::Compile,
            span,
            message: message.into(),
        }
    }

    fn parse_pair(pair: &Pair<'_, Rule>, message: impl Into<String>) -> Self {
        Self::parse(pair_span(pair), message)
    }

    fn compile_pair(pair: &Pair<'_, Rule>, message: impl Into<String>) -> Self {
        Self::compile(pair_span(pair), message)
    }

    fn into_strategy_error(self, input: &str, source_name: Option<&str>) -> StrategyError {
        match self.kind {
            ParserErrorKind::Parse => {
                StrategyError::parse_diagnostic(input, source_name, self.span, self.message)
            }
            ParserErrorKind::Compile => {
                StrategyError::compile_diagnostic(input, source_name, self.span, self.message)
            }
        }
    }
}

fn pair_span(pair: &Pair<'_, Rule>) -> SourceSpan {
    let span = pair.as_span();
    SourceSpan::new(span.start(), span.end())
}

fn pest_error_span(input: &str, err: &pest::error::Error<Rule>) -> SourceSpan {
    match err.location {
        InputLocation::Pos(pos) => SourceSpan::new(pos.min(input.len()), pos.min(input.len())),
        InputLocation::Span((start, end)) => {
            SourceSpan::new(start.min(input.len()), end.min(input.len()))
        }
    }
}

fn parse_strategy_decl(pair: Pair<'_, Rule>) -> ParseResult<Strategy> {
    let span = pair_span(&pair);
    let mut inner = pair.into_inner();
    let name = parse_string(
        inner
            .next()
            .ok_or_else(|| ParserError::parse(span, "missing strategy name"))?,
    )?;
    let block = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, "missing strategy block"))?;

    let mut strategy = Strategy {
        name,
        span,
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
                    return Err(ParserError::compile_pair(
                        &item,
                        "engine block declared more than once",
                    ));
                }
                strategy.engine = Some(parse_engine_decl(item)?);
            }
            Rule::store_decl => {
                if strategy.store.is_some() {
                    return Err(ParserError::compile_pair(
                        &item,
                        "store block declared more than once",
                    ));
                }
                strategy.store = Some(parse_store_decl(item)?);
            }
            Rule::plugin_decl => strategy.plugins.push(parse_plugin_decl(item)?),
            Rule::let_decl => strategy.lets.push(parse_let_decl(item)?),
            Rule::route_decl => strategy.routes.push(parse_route_decl(item)?),
            _ => {
                return Err(ParserError::parse_pair(
                    &item,
                    format!("unexpected item `{}`", item.as_str()),
                ));
            }
        }
    }

    Ok(strategy)
}

fn parse_engine_decl(pair: Pair<'_, Rule>) -> ParseResult<EngineDecl> {
    let span = pair_span(&pair);
    let mut engine = EngineDecl {
        span,
        ..EngineDecl::default()
    };
    for field in pair.into_inner() {
        match field.as_rule() {
            Rule::engine_name_field => {
                if engine.name.is_some() {
                    return Err(ParserError::compile_pair(
                        &field,
                        "engine.name declared more than once",
                    ));
                }
                let value = field
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParserError::parse(span, "missing engine.name value"))?;
                engine.name = Some(parse_string(value)?);
            }
            Rule::max_messages_field => {
                if engine.max_messages.is_some() {
                    return Err(ParserError::compile_pair(
                        &field,
                        "engine.max_messages declared more than once",
                    ));
                }
                let value = field
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParserError::parse(span, "missing engine.max_messages value"))?;
                engine.max_messages = Some(parse_usize(value)?);
            }
            _ => {
                return Err(ParserError::parse_pair(
                    &field,
                    format!("unexpected engine field `{}`", field.as_str()),
                ));
            }
        }
    }
    Ok(engine)
}

fn parse_store_decl(pair: Pair<'_, Rule>) -> ParseResult<StoreDecl> {
    let span = pair_span(&pair);
    let mut store = StoreDecl {
        span,
        ..StoreDecl::default()
    };
    for field in pair.into_inner() {
        match field.as_rule() {
            Rule::store_path_field => {
                if store.path.is_some() {
                    return Err(ParserError::compile_pair(
                        &field,
                        "store.path declared more than once",
                    ));
                }
                let value = field
                    .into_inner()
                    .next()
                    .ok_or_else(|| ParserError::parse(span, "missing store.path value"))?;
                store.path = Some(parse_string(value)?);
            }
            Rule::replay_on_start_field => {
                if store.replay_on_start.is_some() {
                    return Err(ParserError::compile_pair(
                        &field,
                        "store.replay_on_start declared more than once",
                    ));
                }
                let value = field.into_inner().next().ok_or_else(|| {
                    ParserError::parse(span, "missing store.replay_on_start value")
                })?;
                store.replay_on_start = Some(parse_bool(value)?);
            }
            _ => {
                return Err(ParserError::parse_pair(
                    &field,
                    format!("unexpected store field `{}`", field.as_str()),
                ));
            }
        }
    }
    Ok(store)
}

fn parse_plugin_decl(pair: Pair<'_, Rule>) -> ParseResult<PluginDecl> {
    let span = pair_span(&pair);
    let mut inner = pair.into_inner();
    let name_pair = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, "missing plugin name"))?;
    let name_span = pair_span(&name_pair);
    let name = name_pair.as_str().to_string();
    let kind_pair = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, format!("plugin `{name}` missing kind")))?;
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
                        return Err(ParserError::compile_pair(
                            &field,
                            format!("plugin `{name}` declares args more than once"),
                        ));
                    }
                    let value = field.into_inner().next().ok_or_else(|| {
                        ParserError::parse(span, format!("plugin `{name}` missing args"))
                    })?;
                    args = Some(parse_string_list(value)?);
                }
                Rule::autostart_field => {
                    if autostart.is_some() {
                        return Err(ParserError::compile_pair(
                            &field,
                            format!("plugin `{name}` declares autostart more than once"),
                        ));
                    }
                    let value = field.into_inner().next().ok_or_else(|| {
                        ParserError::parse(span, format!("plugin `{name}` missing autostart"))
                    })?;
                    autostart = Some(parse_bool(value)?);
                }
                Rule::working_dir_field => {
                    if working_dir.is_some() {
                        return Err(ParserError::compile_pair(
                            &field,
                            format!("plugin `{name}` declares working_dir more than once"),
                        ));
                    }
                    let value = field.into_inner().next().ok_or_else(|| {
                        ParserError::parse(span, format!("plugin `{name}` missing working_dir"))
                    })?;
                    working_dir = Some(parse_string(value)?);
                }
                Rule::capability_decl => capabilities.push(parse_capability_decl(field)?),
                _ => {
                    return Err(ParserError::parse_pair(
                        &field,
                        format!("unexpected plugin field `{}`", field.as_str()),
                    ));
                }
            }
        }
    }

    Ok(PluginDecl {
        name,
        name_span,
        span,
        kind,
        args: args.unwrap_or_default(),
        autostart: autostart.unwrap_or(false),
        working_dir,
        capabilities,
    })
}

fn parse_plugin_kind(pair: Pair<'_, Rule>) -> ParseResult<PluginKind> {
    let span = pair_span(&pair);
    match pair.as_rule() {
        Rule::process_plugin => {
            let path = pair
                .into_inner()
                .next()
                .ok_or_else(|| ParserError::parse(span, "process plugin missing command"))?;
            Ok(PluginKind::Process {
                command: parse_string(path)?,
            })
        }
        Rule::wasm_plugin => {
            let path = pair
                .into_inner()
                .next()
                .ok_or_else(|| ParserError::parse(span, "wasm plugin missing path"))?;
            Ok(PluginKind::Wasm {
                path: parse_string(path)?,
            })
        }
        _ => Err(ParserError::parse(
            span,
            format!("unexpected plugin kind `{}`", pair.as_str()),
        )),
    }
}

fn parse_capability_decl(pair: Pair<'_, Rule>) -> ParseResult<CapabilityDecl> {
    let span = pair_span(&pair);
    let capability = pair
        .into_inner()
        .next()
        .ok_or_else(|| ParserError::parse(span, "missing capability"))?;
    let kind = match capability.as_rule() {
        Rule::file_read_capability => {
            CapabilityKind::FileRead(parse_single_string_arg(capability, "file_read")?)
        }
        Rule::file_write_capability => {
            CapabilityKind::FileWrite(parse_single_string_arg(capability, "file_write")?)
        }
        Rule::tcp_connect_capability => {
            CapabilityKind::TcpConnect(parse_single_string_arg(capability, "tcp_connect")?)
        }
        Rule::tcp_listen_capability => {
            CapabilityKind::TcpListen(parse_single_string_arg(capability, "tcp_listen")?)
        }
        Rule::record_store_capability => {
            CapabilityKind::RecordStore(parse_single_string_arg(capability, "record_store")?)
        }
        Rule::timer_capability => CapabilityKind::Timer,
        _ => {
            return Err(ParserError::parse(
                span,
                format!("unexpected capability `{}`", capability.as_str()),
            ));
        }
    };
    Ok(CapabilityDecl { kind, span })
}

fn parse_single_string_arg(pair: Pair<'_, Rule>, label: &str) -> ParseResult<String> {
    let span = pair_span(&pair);
    let value = pair
        .into_inner()
        .next()
        .ok_or_else(|| ParserError::parse(span, format!("{label} missing string argument")))?;
    parse_string(value)
}

fn parse_let_decl(pair: Pair<'_, Rule>) -> ParseResult<LetDecl> {
    let span = pair_span(&pair);
    let mut inner = pair.into_inner();
    let name_pair = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, "missing let name"))?;
    let name_span = pair_span(&name_pair);
    let name = name_pair.as_str().to_string();
    let expr = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, format!("let `{name}` missing expression")))
        .and_then(parse_expr)?;
    Ok(LetDecl {
        name,
        name_span,
        span,
        expr,
    })
}

fn parse_route_decl(pair: Pair<'_, Rule>) -> ParseResult<RouteDecl> {
    let span = pair_span(&pair);
    let mut inner = pair.into_inner();
    let name_pair = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, "missing route name"))?;
    let name_span = pair_span(&name_pair);
    let name = name_pair.as_str().to_string();
    let expr = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, format!("route `{name}` missing expression")))
        .and_then(parse_expr)?;
    let target_list = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, format!("route `{name}` missing targets")))?;
    let target_list_span = pair_span(&target_list);
    let targets = parse_target_list(target_list)?;
    Ok(RouteDecl {
        name,
        name_span,
        span,
        expr,
        targets,
        target_list_span,
    })
}

fn parse_expr(pair: Pair<'_, Rule>) -> ParseResult<Expr> {
    let span = pair_span(&pair);
    match pair.as_rule() {
        Rule::expr => {
            let inner = pair
                .into_inner()
                .next()
                .ok_or_else(|| ParserError::parse(span, "empty expression"))?;
            parse_expr(inner)
        }
        Rule::and_expr => {
            let mut parts = Vec::new();
            for part in pair.into_inner() {
                parts.push(parse_expr(part)?);
            }
            match parts.len() {
                0 => Err(ParserError::parse(span, "empty && expression")),
                1 => Ok(parts.remove(0)),
                _ => Ok(Expr::And { parts, span }),
            }
        }
        Rule::comparison => parse_comparison(pair),
        Rule::ref_expr => {
            let name = pair
                .into_inner()
                .next()
                .ok_or_else(|| ParserError::parse(span, "missing predicate reference"))?
                .as_str()
                .to_string();
            Ok(Expr::Ref { name, span })
        }
        _ => Err(ParserError::parse(
            span,
            format!("unexpected expression `{}`", pair.as_str()),
        )),
    }
}

fn parse_comparison(pair: Pair<'_, Rule>) -> ParseResult<Expr> {
    let span = pair_span(&pair);
    let mut inner = pair.into_inner();
    let field = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, "comparison missing field"))
        .and_then(parse_field_path)?;
    let value = inner
        .next()
        .ok_or_else(|| ParserError::parse(span, "comparison missing value"))
        .and_then(parse_literal)?;
    Ok(Expr::Comparison { field, value, span })
}

fn parse_field_path(pair: Pair<'_, Rule>) -> ParseResult<Spanned<FieldPath>> {
    let span = pair_span(&pair);
    let text = pair.as_str();
    if text == "source" {
        return Ok(Spanned::new(FieldPath::Source, span));
    }
    if text == "topic" {
        return Ok(Spanned::new(FieldPath::Topic, span));
    }
    if text == "payload" {
        return Ok(Spanned::new(FieldPath::Payload, span));
    }
    if let Some(field) = text.strip_prefix("record.") {
        return Ok(Spanned::new(FieldPath::Record(field.to_string()), span));
    }
    Err(ParserError::parse(
        span,
        format!("unknown field path `{text}`"),
    ))
}

fn parse_literal(pair: Pair<'_, Rule>) -> ParseResult<Spanned<Literal>> {
    let span = pair_span(&pair);
    match pair.as_rule() {
        Rule::string => Ok(Spanned::new(Literal::String(parse_string(pair)?), span)),
        Rule::bool => Ok(Spanned::new(Literal::Bool(parse_bool(pair)?), span)),
        Rule::int => Ok(Spanned::new(Literal::I64(parse_i64(pair)?), span)),
        Rule::payload_kind => Ok(Spanned::new(
            Literal::Payload(parse_payload_kind(pair.as_str(), span)?),
            span,
        )),
        Rule::ident => Ok(Spanned::new(
            Literal::Ident(pair.as_str().to_string()),
            span,
        )),
        _ => Err(ParserError::parse(
            span,
            format!("unexpected literal `{}`", pair.as_str()),
        )),
    }
}

fn parse_string_list(pair: Pair<'_, Rule>) -> ParseResult<Vec<String>> {
    pair.into_inner().map(parse_string).collect()
}

fn parse_target_list(pair: Pair<'_, Rule>) -> ParseResult<Vec<RouteTarget>> {
    Ok(pair
        .into_inner()
        .map(|target| RouteTarget {
            name: target.as_str().to_string(),
            span: pair_span(&target),
        })
        .collect())
}

fn parse_string(pair: Pair<'_, Rule>) -> ParseResult<String> {
    let span = pair_span(&pair);
    let raw = pair.as_str();
    let inner = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| ParserError::parse(span, format!("invalid string literal `{raw}`")))?;
    let mut output = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| ParserError::parse(span, "unterminated string escape"))?;
        match escaped {
            '"' => output.push('"'),
            '\\' => output.push('\\'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            other => {
                return Err(ParserError::parse(
                    span,
                    format!("unsupported string escape `\\{other}`"),
                ));
            }
        }
    }
    Ok(output)
}

fn parse_bool(pair: Pair<'_, Rule>) -> ParseResult<bool> {
    let span = pair_span(&pair);
    match pair.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        value => Err(ParserError::parse(span, format!("invalid bool `{value}`"))),
    }
}

fn parse_i64(pair: Pair<'_, Rule>) -> ParseResult<i64> {
    let span = pair_span(&pair);
    pair.as_str()
        .parse()
        .map_err(|_| ParserError::parse(span, format!("invalid integer `{}`", pair.as_str())))
}

fn parse_usize(pair: Pair<'_, Rule>) -> ParseResult<usize> {
    let span = pair_span(&pair);
    pair.as_str().parse().map_err(|_| {
        ParserError::parse(
            span,
            format!("invalid unsigned integer `{}`", pair.as_str()),
        )
    })
}

fn parse_payload_kind(value: &str, span: SourceSpan) -> ParseResult<PayloadKind> {
    match value {
        "control" => Ok(PayloadKind::Control),
        "text" => Ok(PayloadKind::Text),
        "bytes" => Ok(PayloadKind::Bytes),
        "record" => Ok(PayloadKind::Record),
        _ => Err(ParserError::parse(
            span,
            format!("unknown payload kind `{value}`"),
        )),
    }
}
