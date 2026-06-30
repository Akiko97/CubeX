use cubex_protocol::PayloadKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StrategyFile {
    pub(crate) includes: Vec<IncludeDecl>,
    pub(crate) body: StrategyFileBody,
    pub(crate) span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StrategyFileBody {
    Strategy(Strategy),
    Fragment(StrategyFragment),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct StrategyFragment {
    pub(crate) span: SourceSpan,
    pub(crate) engine: Option<EngineDecl>,
    pub(crate) store: Option<StoreDecl>,
    pub(crate) plugins: Vec<PluginDecl>,
    pub(crate) lets: Vec<LetDecl>,
    pub(crate) functions: Vec<PredicateFnDecl>,
    pub(crate) routes: Vec<RouteDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncludeDecl {
    pub(crate) path: String,
    pub(crate) path_span: SourceSpan,
    pub(crate) span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    pub name: String,
    pub span: SourceSpan,
    pub engine: Option<EngineDecl>,
    pub store: Option<StoreDecl>,
    pub plugins: Vec<PluginDecl>,
    pub lets: Vec<LetDecl>,
    pub functions: Vec<PredicateFnDecl>,
    pub routes: Vec<RouteDecl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

impl SourceSpan {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn offset(self, offset: usize) -> Self {
        Self {
            start: self.start + offset,
            end: self.end + offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: SourceSpan,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: SourceSpan) -> Self {
        Self { value, span }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineDecl {
    pub name: Option<String>,
    pub max_messages: Option<usize>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreDecl {
    pub path: Option<String>,
    pub replay_on_start: Option<bool>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDecl {
    pub name: String,
    pub name_span: SourceSpan,
    pub span: SourceSpan,
    pub kind: PluginKind,
    pub args: Vec<String>,
    pub autostart: bool,
    pub working_dir: Option<String>,
    pub capabilities: Vec<CapabilityDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginKind {
    Process { command: String },
    Wasm { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDecl {
    pub kind: CapabilityKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityKind {
    FileRead(String),
    FileWrite(String),
    TcpConnect(String),
    TcpListen(String),
    Timer,
    RecordStore(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetDecl {
    pub name: String,
    pub name_span: SourceSpan,
    pub span: SourceSpan,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredicateFnDecl {
    pub name: String,
    pub name_span: SourceSpan,
    pub span: SourceSpan,
    pub params: Vec<Spanned<String>>,
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecl {
    pub name: String,
    pub name_span: SourceSpan,
    pub span: SourceSpan,
    pub expr: Expr,
    pub targets: Vec<RouteTarget>,
    pub target_list_span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteTarget {
    pub name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    And {
        parts: Vec<Expr>,
        span: SourceSpan,
    },
    Comparison {
        field: Spanned<FieldPath>,
        value: Spanned<Literal>,
        span: SourceSpan,
    },
    Ref {
        name: String,
        span: SourceSpan,
    },
    Call {
        name: String,
        name_span: SourceSpan,
        args: Vec<Spanned<Literal>>,
        span: SourceSpan,
    },
}

impl Expr {
    pub fn span(&self) -> SourceSpan {
        match self {
            Expr::And { span, .. }
            | Expr::Comparison { span, .. }
            | Expr::Ref { span, .. }
            | Expr::Call { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldPath {
    Source,
    Topic,
    Payload,
    Record(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    String(String),
    Bool(bool),
    I64(i64),
    Payload(PayloadKind),
    Ident(String),
}
