use crate::ast::{
    CapabilityDecl, CapabilityKind, Expr, FieldPath, IncludeDecl, LetDecl, Literal, PluginDecl,
    PluginKind, PredicateFnDecl, RouteDecl, RouteTarget, SourceSpan, Spanned, Strategy,
    StrategyFile, StrategyFileBody, StrategyFragment,
};
use crate::error::{DiagnosticKind, Result, SourceDiagnostic, StrategyError};
use crate::parser::parse_file_with_source;
use cubex_core::{
    CapabilityConfig, Config, EngineConfig, PluginConfig, RouteConfig, RouteValue, StoreConfig,
};
use cubex_protocol::PayloadKind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub fn compile_file(path: impl AsRef<Path>) -> Result<Config> {
    let mut loader = IncludeLoader::default();
    let loaded = loader.load_root_file(path.as_ref())?;
    compile_loaded_strategy(loaded)
}

pub fn compile_str(input: &str) -> Result<Config> {
    let mut loader = IncludeLoader::default();
    let loaded = loader.load_root_source(input.to_string(), None, None)?;
    compile_loaded_strategy(loaded)
}

pub fn compile_str_with_base(input: &str, base_dir: impl AsRef<Path>) -> Result<Config> {
    let mut loader = IncludeLoader::default();
    let loaded = loader.load_root_source(
        input.to_string(),
        None,
        Some(base_dir.as_ref().to_path_buf()),
    )?;
    compile_loaded_strategy(loaded)
}

type CompileResult<T> = std::result::Result<T, CompileError>;

#[derive(Debug)]
struct CompileError {
    span: SourceSpan,
    message: String,
    kind: CompileErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompileErrorKind {
    General,
    UnreachableRoute,
}

impl CompileError {
    fn at(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            kind: CompileErrorKind::General,
        }
    }

    fn unreachable_route(span: SourceSpan, reason: impl Into<String>) -> Self {
        Self {
            span,
            message: reason.into(),
            kind: CompileErrorKind::UnreachableRoute,
        }
    }

    fn with_route_context(self, route_name: &str) -> Self {
        if self.kind != CompileErrorKind::UnreachableRoute {
            return self;
        }

        Self {
            span: self.span,
            message: format!("route `{route_name}` is unreachable: {}", self.message),
            kind: CompileErrorKind::General,
        }
    }
}

#[derive(Debug)]
struct LoadedStrategy {
    strategy: Strategy,
    sources: SourceMap,
}

fn compile_loaded_strategy(loaded: LoadedStrategy) -> Result<Config> {
    let LoadedStrategy { strategy, sources } = loaded;
    compile_strategy(strategy, PathResolver::from_sources(&sources))
        .map_err(|err| sources.compile_error(err.span, err.message))
}

#[derive(Debug)]
enum LoadedFile {
    Strategy(Strategy),
    Fragment(StrategyFragment),
}

#[derive(Debug, Default)]
struct IncludeLoader {
    sources: SourceMap,
    stack: Vec<PathBuf>,
}

impl IncludeLoader {
    fn load_root_file(&mut self, path: &Path) -> Result<LoadedStrategy> {
        let source_name = path.display().to_string();
        let base_dir = file_base_dir(path)?;
        let key = include_key(path)?;
        let text = std::fs::read_to_string(path).map_err(|source| StrategyError::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;

        self.stack.push(key);
        let loaded = self.load_source(text, Some(source_name), Some(base_dir));
        self.stack.pop();

        self.require_strategy(loaded?)
    }

    fn load_root_source(
        &mut self,
        text: String,
        source_name: Option<String>,
        base_dir: Option<PathBuf>,
    ) -> Result<LoadedStrategy> {
        let loaded = self.load_source(text, source_name, base_dir)?;
        self.require_strategy(loaded)
    }

    fn require_strategy(&self, loaded: LoadedFile) -> Result<LoadedStrategy> {
        match loaded {
            LoadedFile::Strategy(strategy) => Ok(LoadedStrategy {
                strategy,
                sources: self.sources.clone(),
            }),
            LoadedFile::Fragment(fragment) => Err(self
                .sources
                .parse_error(fragment.span, "missing strategy declaration")),
        }
    }

    fn load_source(
        &mut self,
        text: String,
        source_name: Option<String>,
        base_dir: Option<PathBuf>,
    ) -> Result<LoadedFile> {
        let source_base = self
            .sources
            .push(text.clone(), source_name.clone(), base_dir.clone());
        let mut file = parse_file_with_source(&text, source_name.as_deref())?;
        offset_strategy_file(&mut file, source_base);

        let mut included = StrategyFragment {
            span: file.span,
            ..StrategyFragment::default()
        };
        for include in &file.includes {
            let fragment = self.load_include(include, base_dir.as_deref())?;
            merge_fragment(&mut included, fragment, &self.sources)?;
        }

        match file.body {
            StrategyFileBody::Strategy(strategy) => {
                let (name, span, body) = strategy_into_fragment(strategy);
                merge_fragment(&mut included, body, &self.sources)?;
                Ok(LoadedFile::Strategy(fragment_into_strategy(
                    name, span, included,
                )))
            }
            StrategyFileBody::Fragment(fragment) => {
                merge_fragment(&mut included, fragment, &self.sources)?;
                Ok(LoadedFile::Fragment(included))
            }
        }
    }

    fn load_include(
        &mut self,
        include: &IncludeDecl,
        including_base_dir: Option<&Path>,
    ) -> Result<StrategyFragment> {
        let base_dir = including_base_dir.ok_or_else(|| {
            self.sources.compile_error(
                include.path_span,
                "include requires a base directory when compiling from a string",
            )
        })?;
        let include_path = resolve_include_path(base_dir, &include.path);
        let key = include_key(&include_path)?;

        if let Some(index) = self.stack.iter().position(|entry| entry == &key) {
            let cycle = self.stack[index..]
                .iter()
                .chain(std::iter::once(&key))
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(self.sources.compile_error(
                include.path_span,
                format!("include cycle detected: {cycle}"),
            ));
        }

        let text = std::fs::read_to_string(&include_path).map_err(|source| {
            self.sources.compile_error(
                include.path_span,
                format!(
                    "failed to read include `{}`: {source}",
                    include_path.display()
                ),
            )
        })?;
        let source_name = include_path.display().to_string();
        let base_dir = file_base_dir(&include_path)?;

        self.stack.push(key);
        let loaded = self.load_source(text, Some(source_name), Some(base_dir));
        self.stack.pop();

        match loaded? {
            LoadedFile::Strategy(strategy) => {
                let (_, _, fragment) = strategy_into_fragment(strategy);
                Ok(fragment)
            }
            LoadedFile::Fragment(fragment) => Ok(fragment),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct SourceMap {
    files: Vec<SourceFile>,
    next_base: usize,
}

impl SourceMap {
    fn push(
        &mut self,
        source: String,
        source_name: Option<String>,
        base_dir: Option<PathBuf>,
    ) -> usize {
        let base = self.next_base;
        let len = source.len();
        self.files.push(SourceFile {
            source,
            source_name,
            base_dir,
            base,
            len,
        });
        self.next_base += len + 1;
        base
    }

    fn parse_error(&self, span: SourceSpan, message: impl Into<String>) -> StrategyError {
        self.diagnostic(DiagnosticKind::Parse, span, message)
    }

    fn compile_error(&self, span: SourceSpan, message: impl Into<String>) -> StrategyError {
        self.diagnostic(DiagnosticKind::Compile, span, message)
    }

    fn diagnostic(
        &self,
        kind: DiagnosticKind,
        span: SourceSpan,
        message: impl Into<String>,
    ) -> StrategyError {
        if let Some(file) = self.file_for_span(span) {
            let local = SourceSpan::new(
                span.start.saturating_sub(file.base),
                span.end.saturating_sub(file.base),
            );
            return StrategyError::Diagnostic(SourceDiagnostic::new(
                kind,
                &file.source,
                file.source_name.as_deref(),
                local,
                message,
            ));
        }
        StrategyError::Diagnostic(SourceDiagnostic::new(
            kind,
            "",
            None,
            SourceSpan::default(),
            message,
        ))
    }

    fn base_dir_for_span(&self, span: SourceSpan) -> Option<&Path> {
        self.file_for_span(span)
            .and_then(|file| file.base_dir.as_deref())
    }

    fn file_for_span(&self, span: SourceSpan) -> Option<&SourceFile> {
        self.files
            .iter()
            .find(|file| span.start >= file.base && span.start <= file.base + file.len)
            .or_else(|| self.files.first())
    }
}

#[derive(Debug, Clone)]
struct SourceFile {
    source: String,
    source_name: Option<String>,
    base_dir: Option<PathBuf>,
    base: usize,
    len: usize,
}

#[derive(Debug, Clone, Copy)]
struct PathResolver<'a> {
    sources: Option<&'a SourceMap>,
}

impl<'a> PathResolver<'a> {
    fn from_sources(sources: &'a SourceMap) -> Self {
        Self {
            sources: Some(sources),
        }
    }

    fn resolve_path(&self, path: impl Into<PathBuf>, span: SourceSpan) -> PathBuf {
        let path = path.into();
        let Some(base_dir) = self
            .sources
            .and_then(|sources| sources.base_dir_for_span(span))
        else {
            return path;
        };
        resolve_config_path(base_dir, path)
    }
}

fn merge_fragment(
    target: &mut StrategyFragment,
    incoming: StrategyFragment,
    sources: &SourceMap,
) -> Result<()> {
    if let Some(engine) = incoming.engine {
        if target.engine.is_some() {
            return Err(sources.compile_error(engine.span, "engine block declared more than once"));
        }
        target.engine = Some(engine);
    }
    if let Some(store) = incoming.store {
        if target.store.is_some() {
            return Err(sources.compile_error(store.span, "store block declared more than once"));
        }
        target.store = Some(store);
    }

    target.plugins.extend(incoming.plugins);
    target.lets.extend(incoming.lets);
    target.functions.extend(incoming.functions);
    target.routes.extend(incoming.routes);
    Ok(())
}

fn strategy_into_fragment(strategy: Strategy) -> (String, SourceSpan, StrategyFragment) {
    (
        strategy.name,
        strategy.span,
        StrategyFragment {
            span: strategy.span,
            engine: strategy.engine,
            store: strategy.store,
            plugins: strategy.plugins,
            lets: strategy.lets,
            functions: strategy.functions,
            routes: strategy.routes,
        },
    )
}

fn fragment_into_strategy(name: String, span: SourceSpan, fragment: StrategyFragment) -> Strategy {
    Strategy {
        name,
        span,
        engine: fragment.engine,
        store: fragment.store,
        plugins: fragment.plugins,
        lets: fragment.lets,
        functions: fragment.functions,
        routes: fragment.routes,
    }
}

fn file_base_dir(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if parent.is_absolute() {
        Ok(parent.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(StrategyError::CurrentDir)?
            .join(parent))
    }
}

fn include_key(path: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical);
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(StrategyError::CurrentDir)?
            .join(path))
    }
}

fn resolve_include_path(base_dir: &Path, include_path: &str) -> PathBuf {
    let path = PathBuf::from(include_path);
    if path.is_relative() {
        base_dir.join(path)
    } else {
        path
    }
}

fn resolve_config_path(base_dir: &Path, path: PathBuf) -> PathBuf {
    if !path.as_os_str().is_empty() && !path_is_blank(&path) && path.is_relative() {
        base_dir.join(path)
    } else {
        path
    }
}

fn path_is_blank(path: &Path) -> bool {
    path.as_os_str().to_string_lossy().trim().is_empty()
}

fn offset_strategy_file(file: &mut StrategyFile, offset: usize) {
    file.span = file.span.offset(offset);
    for include in &mut file.includes {
        offset_include(include, offset);
    }
    match &mut file.body {
        StrategyFileBody::Strategy(strategy) => offset_strategy(strategy, offset),
        StrategyFileBody::Fragment(fragment) => offset_fragment(fragment, offset),
    }
}

fn offset_include(include: &mut IncludeDecl, offset: usize) {
    include.span = include.span.offset(offset);
    include.path_span = include.path_span.offset(offset);
}

fn offset_strategy(strategy: &mut Strategy, offset: usize) {
    strategy.span = strategy.span.offset(offset);
    if let Some(engine) = &mut strategy.engine {
        engine.span = engine.span.offset(offset);
    }
    if let Some(store) = &mut strategy.store {
        store.span = store.span.offset(offset);
    }
    for plugin in &mut strategy.plugins {
        offset_plugin(plugin, offset);
    }
    for declaration in &mut strategy.lets {
        offset_let(declaration, offset);
    }
    for declaration in &mut strategy.functions {
        offset_fn(declaration, offset);
    }
    for route in &mut strategy.routes {
        offset_route(route, offset);
    }
}

fn offset_fragment(fragment: &mut StrategyFragment, offset: usize) {
    fragment.span = fragment.span.offset(offset);
    if let Some(engine) = &mut fragment.engine {
        engine.span = engine.span.offset(offset);
    }
    if let Some(store) = &mut fragment.store {
        store.span = store.span.offset(offset);
    }
    for plugin in &mut fragment.plugins {
        offset_plugin(plugin, offset);
    }
    for declaration in &mut fragment.lets {
        offset_let(declaration, offset);
    }
    for declaration in &mut fragment.functions {
        offset_fn(declaration, offset);
    }
    for route in &mut fragment.routes {
        offset_route(route, offset);
    }
}

fn offset_plugin(plugin: &mut PluginDecl, offset: usize) {
    plugin.span = plugin.span.offset(offset);
    plugin.name_span = plugin.name_span.offset(offset);
    for capability in &mut plugin.capabilities {
        capability.span = capability.span.offset(offset);
    }
}

fn offset_let(declaration: &mut LetDecl, offset: usize) {
    declaration.span = declaration.span.offset(offset);
    declaration.name_span = declaration.name_span.offset(offset);
    offset_expr(&mut declaration.expr, offset);
}

fn offset_fn(declaration: &mut PredicateFnDecl, offset: usize) {
    declaration.span = declaration.span.offset(offset);
    declaration.name_span = declaration.name_span.offset(offset);
    for param in &mut declaration.params {
        param.span = param.span.offset(offset);
    }
    offset_expr(&mut declaration.expr, offset);
}

fn offset_route(route: &mut RouteDecl, offset: usize) {
    route.span = route.span.offset(offset);
    route.name_span = route.name_span.offset(offset);
    route.target_list_span = route.target_list_span.offset(offset);
    offset_expr(&mut route.expr, offset);
    for target in &mut route.targets {
        offset_route_target(target, offset);
    }
}

fn offset_route_target(target: &mut RouteTarget, offset: usize) {
    target.span = target.span.offset(offset);
}

fn offset_expr(expr: &mut Expr, offset: usize) {
    match expr {
        Expr::And { parts, span } => {
            *span = span.offset(offset);
            for part in parts {
                offset_expr(part, offset);
            }
        }
        Expr::Comparison { field, value, span } => {
            *span = span.offset(offset);
            field.span = field.span.offset(offset);
            value.span = value.span.offset(offset);
        }
        Expr::Ref { span, .. } => {
            *span = span.offset(offset);
        }
        Expr::Call {
            name_span,
            args,
            span,
            ..
        } => {
            *span = span.offset(offset);
            *name_span = name_span.offset(offset);
            for arg in args {
                arg.span = arg.span.offset(offset);
            }
        }
    }
}

fn compile_strategy(strategy: Strategy, resolver: PathResolver<'_>) -> CompileResult<Config> {
    let engine_decl = strategy.engine.unwrap_or_default();
    let engine = EngineConfig {
        name: engine_decl.name.unwrap_or(strategy.name),
        max_messages: engine_decl
            .max_messages
            .unwrap_or_else(|| EngineConfig::default().max_messages),
    };

    let store_decl = strategy.store.unwrap_or_default();
    let store = StoreConfig {
        path: store_decl
            .path
            .map(|path| resolver.resolve_path(path, store_decl.span)),
        replay_on_start: store_decl.replay_on_start.unwrap_or(false),
    };

    let mut symbols = BTreeSet::new();
    let mut plugin_names = BTreeSet::new();
    let mut plugin_declarations = Vec::new();
    let mut plugins = Vec::new();
    for plugin in strategy.plugins {
        insert_symbol(&mut symbols, "binding", &plugin.name, plugin.name_span)?;
        plugin_names.insert(plugin.name.clone());
        plugin_declarations.push(PluginStaticInfo {
            name: plugin.name.clone(),
            name_span: plugin.name_span,
            autostart: plugin.autostart,
        });
        plugins.push(compile_plugin(plugin, resolver)?);
    }

    let mut lets = BTreeMap::new();
    let mut predicate_declarations = Vec::new();
    for declaration in strategy.lets {
        insert_symbol(
            &mut symbols,
            "binding",
            &declaration.name,
            declaration.name_span,
        )?;
        predicate_declarations.push(PredicateStaticInfo {
            name: declaration.name.clone(),
            name_span: declaration.name_span,
            kind: PredicateKind::Binding,
        });
        if lets
            .insert(declaration.name.clone(), declaration.expr)
            .is_some()
        {
            return Err(CompileError::at(
                declaration.name_span,
                format!("predicate `{}` declared more than once", declaration.name),
            ));
        }
    }

    let mut functions = BTreeMap::new();
    for declaration in strategy.functions {
        let name_span = declaration.name_span;
        insert_symbol(&mut symbols, "binding", &declaration.name, name_span)?;
        predicate_declarations.push(PredicateStaticInfo {
            name: declaration.name.clone(),
            name_span,
            kind: PredicateKind::Function,
        });
        let function = compile_predicate_fn(declaration)?;
        if functions.insert(function.name.clone(), function).is_some() {
            return Err(CompileError::at(
                name_span,
                "predicate function declared more than once",
            ));
        }
    }

    let mut routes = Vec::new();
    let mut route_declarations = Vec::new();
    let mut usage = StaticUsage::default();
    for route in strategy.routes {
        let route_name_span = route.name_span;
        insert_symbol(&mut symbols, "binding", &route.name, route.name_span)?;
        let route = compile_route(
            route,
            &lets,
            &functions,
            &plugin_names,
            &engine.name,
            &mut usage,
        )?;
        if let Some(source) = &route.source
            && plugin_names.contains(source)
        {
            usage.plugins.insert(source.clone());
        }
        usage.plugins.extend(route.to.iter().cloned());
        let route_declaration = RouteStaticInfo::from_route(&route, route_name_span);
        check_equivalent_route_predicates(&route_declarations, &route_declaration)?;
        route_declarations.push(route_declaration);
        routes.push(route);
    }

    usage.plugins.extend(
        plugin_declarations
            .iter()
            .filter(|plugin| plugin.autostart)
            .map(|plugin| plugin.name.clone()),
    );

    check_unused_plugins(&plugin_declarations, &usage)?;
    check_unused_predicates(&predicate_declarations, &usage)?;

    Ok(Config {
        engine,
        store,
        plugins,
        routes,
    })
}

#[derive(Debug, Clone)]
struct PluginStaticInfo {
    name: String,
    name_span: SourceSpan,
    autostart: bool,
}

#[derive(Debug, Clone)]
struct PredicateStaticInfo {
    name: String,
    name_span: SourceSpan,
    kind: PredicateKind,
}

#[derive(Debug, Clone)]
struct RouteStaticInfo {
    name: String,
    name_span: SourceSpan,
    source: Option<String>,
    topic: Option<String>,
    payload: Option<PayloadKind>,
    record: BTreeMap<String, RouteValue>,
}

impl RouteStaticInfo {
    fn from_route(route: &RouteConfig, name_span: SourceSpan) -> Self {
        Self {
            name: route.name.clone(),
            name_span,
            source: route.source.clone(),
            topic: route.topic.clone(),
            payload: route.payload,
            record: route.record.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum PredicateKind {
    Binding,
    Function,
}

#[derive(Debug, Default)]
struct StaticUsage {
    plugins: BTreeSet<String>,
    predicates: BTreeSet<String>,
}

fn check_unused_plugins(
    plugin_declarations: &[PluginStaticInfo],
    usage: &StaticUsage,
) -> CompileResult<()> {
    for plugin in plugin_declarations {
        if !usage.plugins.contains(&plugin.name) {
            return Err(CompileError::at(
                plugin.name_span,
                format!("unused plugin `{}`", plugin.name),
            ));
        }
    }
    Ok(())
}

fn check_unused_predicates(
    predicate_declarations: &[PredicateStaticInfo],
    usage: &StaticUsage,
) -> CompileResult<()> {
    for predicate in predicate_declarations {
        if !usage.predicates.contains(&predicate.name) {
            let label = match predicate.kind {
                PredicateKind::Binding => "predicate binding",
                PredicateKind::Function => "predicate function",
            };
            return Err(CompileError::at(
                predicate.name_span,
                format!("unused {label} `{}`", predicate.name),
            ));
        }
    }
    Ok(())
}

fn check_equivalent_route_predicates(
    previous_routes: &[RouteStaticInfo],
    route: &RouteStaticInfo,
) -> CompileResult<()> {
    for previous in previous_routes {
        if route_predicates_equivalent(previous, route) {
            return Err(CompileError::at(
                route.name_span,
                format!(
                    "route `{}` has equivalent predicate to route `{}`",
                    route.name, previous.name
                ),
            ));
        }
    }
    Ok(())
}

fn route_predicates_equivalent(left: &RouteStaticInfo, right: &RouteStaticInfo) -> bool {
    left.source == right.source
        && left.topic == right.topic
        && left.payload == right.payload
        && left.record == right.record
}

#[derive(Debug, Clone)]
struct PredicateFn {
    name: String,
    params: Vec<Spanned<String>>,
    expr: Expr,
}

fn compile_predicate_fn(declaration: PredicateFnDecl) -> CompileResult<PredicateFn> {
    let mut params = BTreeSet::new();
    for param in &declaration.params {
        if !params.insert(param.value.clone()) {
            return Err(CompileError::at(
                param.span,
                format!(
                    "predicate function `{}` declares parameter `{}` more than once",
                    declaration.name, param.value
                ),
            ));
        }
    }
    Ok(PredicateFn {
        name: declaration.name,
        params: declaration.params,
        expr: declaration.expr,
    })
}

fn insert_symbol(
    symbols: &mut BTreeSet<String>,
    label: &str,
    name: &str,
    span: SourceSpan,
) -> CompileResult<()> {
    if !symbols.insert(name.to_string()) {
        return Err(CompileError::at(
            span,
            format!("{label} `{name}` declared more than once"),
        ));
    }
    Ok(())
}

fn compile_plugin(plugin: PluginDecl, resolver: PathResolver<'_>) -> CompileResult<PluginConfig> {
    match plugin.kind {
        PluginKind::Process { command } => {
            if !plugin.capabilities.is_empty() {
                let span = plugin
                    .capabilities
                    .first()
                    .map(|capability| capability.span)
                    .unwrap_or(plugin.span);
                return Err(CompileError::at(
                    span,
                    format!(
                        "process plugin `{}` cannot declare capabilities",
                        plugin.name
                    ),
                ));
            }
            Ok(PluginConfig {
                name: plugin.name,
                command: resolver.resolve_path(command, plugin.span),
                wasm: None,
                working_dir: plugin
                    .working_dir
                    .map(|path| resolver.resolve_path(path, plugin.span)),
                args: plugin.args,
                autostart: plugin.autostart,
                capabilities: Vec::new(),
            })
        }
        PluginKind::Wasm { path } => Ok(PluginConfig {
            name: plugin.name,
            command: PathBuf::new(),
            wasm: Some(resolver.resolve_path(path, plugin.span)),
            working_dir: plugin
                .working_dir
                .map(|path| resolver.resolve_path(path, plugin.span)),
            args: plugin.args,
            autostart: plugin.autostart,
            capabilities: plugin
                .capabilities
                .into_iter()
                .map(|capability| compile_capability(capability, resolver))
                .collect(),
        }),
    }
}

fn compile_capability(capability: CapabilityDecl, resolver: PathResolver<'_>) -> CapabilityConfig {
    match capability.kind {
        CapabilityKind::FileRead(path) => CapabilityConfig::FileRead {
            path: resolver.resolve_path(path, capability.span),
        },
        CapabilityKind::FileWrite(path) => CapabilityConfig::FileWrite {
            path: resolver.resolve_path(path, capability.span),
        },
        CapabilityKind::TcpConnect(addr) => CapabilityConfig::TcpConnect { addr },
        CapabilityKind::TcpListen(addr) => CapabilityConfig::TcpListen { addr },
        CapabilityKind::Timer => CapabilityConfig::Timer,
        CapabilityKind::RecordStore(path) => CapabilityConfig::RecordStore {
            path: resolver.resolve_path(path, capability.span),
        },
    }
}

fn compile_route(
    route: RouteDecl,
    lets: &BTreeMap<String, Expr>,
    functions: &BTreeMap<String, PredicateFn>,
    plugin_names: &BTreeSet<String>,
    engine_name: &str,
    usage: &mut StaticUsage,
) -> CompileResult<RouteConfig> {
    let route_name = route.name;
    let route_expr = route.expr;
    let target_list_span = route.target_list_span;

    if route.targets.is_empty() {
        return Err(CompileError::at(
            target_list_span,
            format!("route `{}` must have at least one target", route_name),
        ));
    }

    let mut targets = BTreeSet::new();
    let mut to = Vec::new();
    for target in route.targets {
        if !targets.insert(target.name.clone()) {
            return Err(CompileError::at(
                target.span,
                format!(
                    "route `{}` targets `{}` more than once",
                    route_name, target.name
                ),
            ));
        }
        if !plugin_names.contains(&target.name) {
            return Err(CompileError::at(
                target.span,
                format!(
                    "route `{}` targets unknown plugin `{}`",
                    route_name, target.name
                ),
            ));
        }
        to.push(target.name);
    }

    let mut stack = BTreeSet::new();
    let bindings = PredicateBindings::new();
    let filter = compile_expr(
        &route_expr,
        lets,
        functions,
        plugin_names,
        engine_name,
        &mut stack,
        &bindings,
        usage,
    )
    .map_err(|err| err.with_route_context(&route_name))?;
    filter
        .into_route(route_name.clone(), to, route_expr.span())
        .map_err(|err| err.with_route_context(&route_name))
}

type PredicateBindings = BTreeMap<String, Spanned<Literal>>;

fn compile_expr(
    expr: &Expr,
    lets: &BTreeMap<String, Expr>,
    functions: &BTreeMap<String, PredicateFn>,
    plugin_names: &BTreeSet<String>,
    engine_name: &str,
    stack: &mut BTreeSet<String>,
    bindings: &PredicateBindings,
    usage: &mut StaticUsage,
) -> CompileResult<RouteFilter> {
    match expr {
        Expr::And { parts, .. } => {
            let mut filter = RouteFilter::default();
            for part in parts {
                filter.merge(
                    compile_expr(
                        part,
                        lets,
                        functions,
                        plugin_names,
                        engine_name,
                        stack,
                        bindings,
                        usage,
                    )?,
                    part.span(),
                )?;
            }
            Ok(filter)
        }
        Expr::Comparison { field, value, .. } => {
            let value = resolve_literal(value, bindings);
            compile_comparison(field, &value, plugin_names, engine_name)
        }
        Expr::Ref { name, span } => {
            if !stack.insert(name.clone()) {
                return Err(CompileError::at(
                    *span,
                    format!("predicate reference cycle includes `{name}`"),
                ));
            }
            let expr = lets.get(name).ok_or_else(|| {
                if functions.contains_key(name) {
                    CompileError::at(
                        *span,
                        format!("predicate function `{name}` must be called with parentheses"),
                    )
                } else {
                    CompileError::at(*span, format!("unknown predicate reference `{name}`"))
                }
            })?;
            usage.predicates.insert(name.clone());
            let filter = compile_expr(
                expr,
                lets,
                functions,
                plugin_names,
                engine_name,
                stack,
                bindings,
                usage,
            );
            stack.remove(name);
            filter
        }
        Expr::Call {
            name,
            name_span,
            args,
            span,
        } => {
            let function = functions.get(name).ok_or_else(|| {
                if lets.contains_key(name) {
                    CompileError::at(
                        *name_span,
                        format!("predicate `{name}` is not parameterized and cannot be called"),
                    )
                } else {
                    CompileError::at(*name_span, format!("unknown predicate function `{name}`"))
                }
            })?;
            if function.params.len() != args.len() {
                return Err(CompileError::at(
                    *span,
                    format!(
                        "predicate function `{name}` expects {} argument{} but got {}",
                        function.params.len(),
                        plural_suffix(function.params.len()),
                        args.len()
                    ),
                ));
            }
            usage.predicates.insert(name.clone());
            if !stack.insert(name.clone()) {
                return Err(CompileError::at(
                    *name_span,
                    format!("predicate reference cycle includes `{name}`"),
                ));
            }

            let resolved_args = args
                .iter()
                .map(|arg| resolve_literal(arg, bindings))
                .collect::<Vec<_>>();
            let mut call_bindings = bindings.clone();
            for (param, arg) in function.params.iter().zip(resolved_args) {
                call_bindings.insert(param.value.clone(), arg);
            }
            let filter = compile_expr(
                &function.expr,
                lets,
                functions,
                plugin_names,
                engine_name,
                stack,
                &call_bindings,
                usage,
            );
            stack.remove(name);
            filter
        }
    }
}

fn resolve_literal(literal: &Spanned<Literal>, bindings: &PredicateBindings) -> Spanned<Literal> {
    match &literal.value {
        Literal::Ident(name) => bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| literal.clone()),
        _ => literal.clone(),
    }
}

fn compile_comparison(
    field: &Spanned<FieldPath>,
    literal: &Spanned<Literal>,
    plugin_names: &BTreeSet<String>,
    engine_name: &str,
) -> CompileResult<RouteFilter> {
    let mut filter = RouteFilter::default();
    match &field.value {
        FieldPath::Source => {
            let source = match &literal.value {
                Literal::Ident(value) | Literal::String(value) => value.clone(),
                _ => {
                    return Err(CompileError::at(
                        literal.span,
                        "source comparisons require a plugin identifier or string",
                    ));
                }
            };
            if source != engine_name && !plugin_names.contains(&source) {
                return Err(CompileError::at(
                    literal.span,
                    format!("source comparison references unknown plugin or engine `{source}`"),
                ));
            }
            filter.source = Some(source);
        }
        FieldPath::Topic => {
            let Literal::String(topic) = &literal.value else {
                return Err(CompileError::at(
                    literal.span,
                    "topic comparisons require a string literal",
                ));
            };
            filter.topic = Some(topic.clone());
        }
        FieldPath::Payload => {
            let Literal::Payload(payload) = &literal.value else {
                return Err(CompileError::at(
                    literal.span,
                    "payload comparisons require a payload kind",
                ));
            };
            filter.payload = Some(*payload);
        }
        FieldPath::Record(key) => {
            let route_value = match &literal.value {
                Literal::String(value) => RouteValue::String(value.clone()),
                Literal::Bool(value) => RouteValue::Bool(*value),
                Literal::I64(value) => RouteValue::I64(*value),
                Literal::Ident(value) => {
                    return Err(CompileError::at(
                        literal.span,
                        format!("record field `{key}` comparison value `{value}` must be quoted"),
                    ));
                }
                Literal::Payload(_) => {
                    return Err(CompileError::at(
                        literal.span,
                        format!("record field `{key}` cannot compare against a payload kind"),
                    ));
                }
            };
            filter.record.insert(key.clone(), route_value);
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
    fn merge(&mut self, other: Self, span: SourceSpan) -> CompileResult<()> {
        merge_option("source", &mut self.source, other.source, span, |value| {
            string_value_label(value)
        })?;
        merge_option("topic", &mut self.topic, other.topic, span, |value| {
            string_literal_label(value)
        })?;
        merge_option(
            "payload",
            &mut self.payload,
            other.payload,
            span,
            payload_value_label,
        )?;
        for (key, value) in other.record {
            if let Some(existing) = self.record.get(&key)
                && existing != &value
            {
                return Err(CompileError::unreachable_route(
                    span,
                    format!(
                        "conflicting `record.{key}` predicates: {} vs {}",
                        route_value_label(existing),
                        route_value_label(&value)
                    ),
                ));
            }
            self.record.insert(key, value);
        }
        self.check_record_payload_compatibility(span)?;
        Ok(())
    }

    fn into_route(
        mut self,
        name: String,
        to: Vec<String>,
        span: SourceSpan,
    ) -> CompileResult<RouteConfig> {
        if !self.record.is_empty() {
            match self.payload {
                None => self.payload = Some(PayloadKind::Record),
                Some(PayloadKind::Record) => {}
                Some(payload) => {
                    return Err(CompileError::unreachable_route(
                        span,
                        format!(
                            "record predicates require payload `record`, but payload is `{}`",
                            payload_label(payload)
                        ),
                    ));
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

    fn check_record_payload_compatibility(&self, span: SourceSpan) -> CompileResult<()> {
        if !self.record.is_empty()
            && let Some(payload) = self.payload
            && payload != PayloadKind::Record
        {
            return Err(CompileError::unreachable_route(
                span,
                format!(
                    "record predicates require payload `record`, but payload is `{}`",
                    payload_label(payload)
                ),
            ));
        }
        Ok(())
    }
}

fn merge_option<T: Eq>(
    field: &str,
    target: &mut Option<T>,
    incoming: Option<T>,
    span: SourceSpan,
    label: impl Fn(&T) -> String,
) -> CompileResult<()> {
    let Some(incoming) = incoming else {
        return Ok(());
    };
    if let Some(existing) = target.as_ref()
        && existing != &incoming
    {
        return Err(CompileError::unreachable_route(
            span,
            format!(
                "conflicting `{field}` predicates: {} vs {}",
                label(existing),
                label(&incoming)
            ),
        ));
    }
    *target = Some(incoming);
    Ok(())
}

fn string_value_label(value: &str) -> String {
    format!("`{}`", value.escape_debug())
}

fn string_literal_label(value: &str) -> String {
    format!("`\"{}\"`", value.escape_debug())
}

fn payload_value_label(payload: &PayloadKind) -> String {
    format!("`{}`", payload_label(*payload))
}

fn route_value_label(value: &RouteValue) -> String {
    match value {
        RouteValue::Bool(value) => format!("`{value}`"),
        RouteValue::I64(value) => format!("`{value}`"),
        RouteValue::String(value) => string_literal_label(value),
    }
}

fn payload_label(payload: PayloadKind) -> &'static str {
    match payload {
        PayloadKind::Control => "control",
        PayloadKind::Text => "text",
        PayloadKind::Bytes => "bytes",
        PayloadKind::Record => "record",
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
