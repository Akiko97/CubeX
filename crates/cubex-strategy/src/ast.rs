use cubex_protocol::PayloadKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    pub name: String,
    pub engine: Option<EngineDecl>,
    pub store: Option<StoreDecl>,
    pub plugins: Vec<PluginDecl>,
    pub lets: Vec<LetDecl>,
    pub routes: Vec<RouteDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineDecl {
    pub name: Option<String>,
    pub max_messages: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreDecl {
    pub path: Option<String>,
    pub replay_on_start: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginDecl {
    pub name: String,
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
pub enum CapabilityDecl {
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
    pub expr: Expr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDecl {
    pub name: String,
    pub expr: Expr,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    And(Vec<Expr>),
    Comparison { field: FieldPath, value: Literal },
    Ref(String),
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
