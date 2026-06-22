use crate::config::{
    CapabilityConfig, Config, PluginConfig, RouteConfig, has_edge_whitespace, validate_config,
};
use crate::error::{Error, Result};
use cubex_protocol::{
    Control, HostPayload, HostRequest, HostResponse, Message, Payload, PluginRequest,
    PluginResponse,
};
use cubex_store::EventLog;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufReader, BufWriter, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;
use wasmtime::{
    Caller, Config as WasmtimeConfig, Engine as WasmtimeEngine, Instance, Linker, Memory, Module,
    Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

const WASM_CALL_FUEL: u64 = 100_000_000;
const WASM_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const MAX_WASM_SLEEP_MS: u64 = 60_000;
const MAX_WASM_TCP_TIMEOUT_MS: u64 = 60_000;
const MAX_WASM_TCP_ECHO_CONNECTIONS: u64 = 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    pub started: Vec<String>,
    pub replayed: usize,
    pub delivered: Vec<Delivery>,
    pub emitted: Vec<Message>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub route: String,
    pub target: String,
    pub message_id: uuid::Uuid,
}

pub struct Engine {
    name: String,
    max_messages: usize,
    store: Option<EventLog>,
    replay_on_start: bool,
    plugin_order: Vec<String>,
    plugins: BTreeMap<String, RuntimePlugin>,
    routes: Vec<RouteConfig>,
}

impl Engine {
    pub fn from_config(config: Config) -> Result<Self> {
        validate_config(&config)?;
        let plugin_order = config
            .plugins
            .iter()
            .map(|plugin| plugin.name.clone())
            .collect();
        let plugins = config
            .plugins
            .into_iter()
            .map(|plugin| Ok((plugin.name.clone(), RuntimePlugin::new(plugin)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        Ok(Self {
            name: config.engine.name,
            max_messages: config.engine.max_messages,
            store: config.store.path.map(EventLog::new),
            replay_on_start: config.store.replay_on_start,
            plugin_order,
            plugins,
            routes: config.routes,
        })
    }

    pub fn run(&self) -> Result<RunReport> {
        let result = self.run_inner();
        let stop_result = self.stop_plugins();
        match (result, stop_result) {
            (Ok(mut report), Ok(logs)) => {
                report.logs.extend(logs);
                Ok(report)
            }
            (Ok(_), Err(err)) | (Err(err), _) => Err(err),
        }
    }

    fn run_inner(&self) -> Result<RunReport> {
        let mut report = RunReport::default();
        let mut queue = VecDeque::new();
        let mut started = BTreeSet::new();

        if self.replay_on_start
            && let Some(store) = &self.store
        {
            let messages = store.read_all()?;
            for message in messages {
                self.validate_replayed_message(&message)?;
                report.replayed += 1;
                queue.push_back(QueuedMessage::replayed(message));
            }
        }

        for name in &self.plugin_order {
            let plugin = self
                .plugins
                .get(name)
                .ok_or_else(|| Error::MissingPlugin(name.clone()))?;
            if !plugin.autostart() {
                continue;
            }
            self.start_plugin(name, plugin, false, &mut queue, &mut report)?;
            started.insert(name.clone());
        }

        let mut processed = 0;
        while let Some(queued) = queue.pop_front() {
            processed += 1;
            if processed > self.max_messages {
                return Err(Error::MessageLimit(self.max_messages));
            }

            let message = queued.message;
            if !queued.replayed
                && let Some(store) = &self.store
            {
                store.append(&message)?;
            }
            report.emitted.push(message.clone());
            for route in self.routes.iter().filter(|route| route.matches(&message)) {
                for target in &route.to {
                    let plugin = self
                        .plugins
                        .get(target)
                        .ok_or_else(|| Error::MissingPlugin(target.clone()))?;
                    if !started.contains(target) {
                        self.start_plugin(
                            target,
                            plugin,
                            queued.replayed,
                            &mut queue,
                            &mut report,
                        )?;
                        started.insert(target.clone());
                    }
                    report.delivered.push(Delivery {
                        route: route.name.clone(),
                        target: target.clone(),
                        message_id: message.id,
                    });
                    self.call_plugin(
                        target,
                        plugin,
                        message.clone(),
                        queued.replayed,
                        &mut queue,
                        &mut report,
                    )?;
                }
            }
        }

        Ok(report)
    }

    fn start_plugin(
        &self,
        name: &str,
        plugin: &RuntimePlugin,
        replayed: bool,
        queue: &mut VecDeque<QueuedMessage>,
        report: &mut RunReport,
    ) -> Result<()> {
        report.started.push(name.to_string());
        let start = Message::new(
            self.name.clone(),
            "system.start",
            Payload::Control(Control::Start {
                args: plugin.args().to_vec(),
            }),
        );
        self.call_plugin(name, plugin, start, replayed, queue, report)
    }

    fn stop_plugins(&self) -> Result<Vec<String>> {
        let mut logs = Vec::new();
        let mut first_error = None;
        for name in self.plugin_order.iter().rev() {
            let plugin = self
                .plugins
                .get(name)
                .ok_or_else(|| Error::MissingPlugin(name.clone()))?;
            match plugin.shutdown(&self.name) {
                Ok(Some(mut response)) => {
                    logs.append(&mut response.logs);
                    if first_error.is_none() {
                        if let Err(err) = validate_error_response(name, &response) {
                            first_error = Some(err);
                        } else if let Some(reason) = response.error.take() {
                            first_error = Some(plugin_error(name, reason));
                        } else if !response.messages.is_empty() {
                            first_error = Some(Error::InvalidPluginMessage {
                                plugin: name.clone(),
                                reason: "system.stop response must not emit messages".into(),
                            });
                        }
                    }
                }
                Ok(None) => {}
                Err(err) if first_error.is_none() => first_error = Some(err),
                Err(_) => {}
            }
        }
        if let Some(err) = first_error {
            Err(err)
        } else {
            Ok(logs)
        }
    }

    fn call_plugin(
        &self,
        name: &str,
        plugin: &RuntimePlugin,
        message: Message,
        replayed: bool,
        queue: &mut VecDeque<QueuedMessage>,
        report: &mut RunReport,
    ) -> Result<()> {
        let mut response = plugin.call(PluginRequest {
            plugin: name.to_string(),
            message,
        })?;
        if let Err(err) = validate_error_response(name, &response) {
            report.logs.append(&mut response.logs);
            return Err(err);
        }
        if let Some(reason) = response.error.take() {
            report.logs.append(&mut response.logs);
            return Err(plugin_error(name, reason));
        }
        normalize_plugin_messages(name, &mut response.messages)?;
        queue.extend(
            response
                .messages
                .into_iter()
                .map(|message| QueuedMessage { message, replayed }),
        );
        report.logs.append(&mut response.logs);
        Ok(())
    }

    fn validate_replayed_message(&self, message: &Message) -> Result<()> {
        validate_stored_message(message)?;
        if message.source != self.name && !self.plugins.contains_key(&message.source) {
            return Err(Error::InvalidStoredMessage(format!(
                "source `{}` is not configured",
                message.source
            )));
        }
        Ok(())
    }
}

fn normalize_plugin_messages(name: &str, messages: &mut [Message]) -> Result<()> {
    for message in messages {
        if message.id.is_nil() {
            return Err(Error::InvalidPluginMessage {
                plugin: name.into(),
                reason: "id must not be nil".into(),
            });
        }
        if message.topic.trim().is_empty() {
            return Err(Error::InvalidPluginMessage {
                plugin: name.into(),
                reason: "topic must not be empty".into(),
            });
        }
        if has_edge_whitespace(&message.topic) {
            return Err(Error::InvalidPluginMessage {
                plugin: name.into(),
                reason: "topic must not have leading or trailing whitespace".into(),
            });
        }
        if matches!(message.payload, Payload::Control(_)) {
            return Err(Error::InvalidPluginMessage {
                plugin: name.into(),
                reason: "control payloads are reserved for host messages".into(),
            });
        }
        message.source = name.to_string();
    }
    Ok(())
}

fn validate_stored_message(message: &Message) -> Result<()> {
    if message.id.is_nil() {
        return Err(Error::InvalidStoredMessage("id must not be nil".into()));
    }
    if message.source.trim().is_empty() {
        return Err(Error::InvalidStoredMessage(
            "source must not be empty".into(),
        ));
    }
    if has_edge_whitespace(&message.source) {
        return Err(Error::InvalidStoredMessage(
            "source must not have leading or trailing whitespace".into(),
        ));
    }
    if message.topic.trim().is_empty() {
        return Err(Error::InvalidStoredMessage(
            "topic must not be empty".into(),
        ));
    }
    if has_edge_whitespace(&message.topic) {
        return Err(Error::InvalidStoredMessage(
            "topic must not have leading or trailing whitespace".into(),
        ));
    }
    if matches!(message.payload, Payload::Control(_)) {
        return Err(Error::InvalidStoredMessage(
            "control payloads are reserved for host messages".into(),
        ));
    }
    Ok(())
}

fn plugin_error(name: &str, reason: String) -> Error {
    if reason.trim().is_empty() {
        return Error::InvalidPluginMessage {
            plugin: name.into(),
            reason: "error must not be empty".into(),
        };
    }
    if has_edge_whitespace(&reason) {
        return Error::InvalidPluginMessage {
            plugin: name.into(),
            reason: "error must not have leading or trailing whitespace".into(),
        };
    }
    Error::PluginError {
        plugin: name.into(),
        reason,
    }
}

fn validate_error_response(name: &str, response: &PluginResponse) -> Result<()> {
    if response.error.is_some() && !response.messages.is_empty() {
        return Err(Error::InvalidPluginMessage {
            plugin: name.into(),
            reason: "error response must not emit messages".into(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct QueuedMessage {
    message: Message,
    replayed: bool,
}

impl QueuedMessage {
    fn replayed(message: Message) -> Self {
        Self {
            message,
            replayed: true,
        }
    }
}

enum RuntimePlugin {
    Process(Box<ProcessPlugin>),
    Wasm(Box<WasmPlugin>),
}

impl RuntimePlugin {
    fn new(config: PluginConfig) -> Result<Self> {
        if let Some(wasm) = &config.wasm {
            return Ok(Self::Wasm(Box::new(WasmPlugin::new(
                config.name,
                wasm.clone(),
                config.working_dir,
                config.args,
                config.autostart,
                config.capabilities,
            )?)));
        }
        Ok(Self::Process(Box::new(ProcessPlugin::new(config)?)))
    }

    fn autostart(&self) -> bool {
        match self {
            Self::Process(plugin) => plugin.autostart,
            Self::Wasm(plugin) => plugin.autostart,
        }
    }

    fn args(&self) -> &[String] {
        match self {
            Self::Process(plugin) => &plugin.args,
            Self::Wasm(plugin) => &plugin.args,
        }
    }

    fn call(&self, request: PluginRequest) -> Result<PluginResponse> {
        match self {
            Self::Process(plugin) => plugin.call(request),
            Self::Wasm(plugin) => plugin.call(request),
        }
    }

    fn shutdown(&self, source: &str) -> Result<Option<PluginResponse>> {
        match self {
            Self::Process(plugin) => plugin.shutdown(source),
            Self::Wasm(plugin) => plugin.shutdown(source),
        }
    }
}

struct ProcessPlugin {
    name: String,
    command: PathBuf,
    working_dir: Option<PathBuf>,
    args: Vec<String>,
    autostart: bool,
    child: Mutex<Option<PluginChild>>,
}

impl ProcessPlugin {
    fn new(config: PluginConfig) -> Result<Self> {
        Ok(Self {
            name: config.name,
            command: config.command,
            working_dir: config.working_dir,
            args: config.args,
            autostart: config.autostart,
            child: Mutex::new(None),
        })
    }

    fn call(&self, request: PluginRequest) -> Result<PluginResponse> {
        let mut guard = self.lock_child()?;
        if guard.is_none() {
            *guard = Some(self.spawn()?);
        }
        let child = guard.as_mut().ok_or_else(|| Error::PluginState {
            plugin: self.name.clone(),
            reason: "child was not started".into(),
        })?;
        child.call(&self.name, request)
    }

    fn shutdown(&self, source: &str) -> Result<Option<PluginResponse>> {
        let mut guard = self.lock_child()?;
        let Some(child) = guard.take() else {
            return Ok(None);
        };
        child.shutdown(&self.name, source)
    }

    fn lock_child(&self) -> Result<std::sync::MutexGuard<'_, Option<PluginChild>>> {
        self.child.lock().map_err(|_| Error::PluginState {
            plugin: self.name.clone(),
            reason: "child lock poisoned".into(),
        })
    }

    fn spawn(&self) -> Result<PluginChild> {
        let mut command = Command::new(&self.command);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(working_dir) = &self.working_dir {
            command.current_dir(working_dir);
        }
        let mut child = command.spawn().map_err(|err| Error::PluginState {
            plugin: self.name.clone(),
            reason: format!("failed to spawn `{}`: {err}", self.command.display()),
        })?;

        let stdin = child.stdin.take().ok_or_else(|| Error::PluginState {
            plugin: self.name.clone(),
            reason: "missing piped stdin".into(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| Error::PluginState {
            plugin: self.name.clone(),
            reason: "missing piped stdout".into(),
        })?;
        Ok(PluginChild {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        })
    }
}

struct PluginChild {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl PluginChild {
    fn call(&mut self, name: &str, request: PluginRequest) -> Result<PluginResponse> {
        match cubex_protocol::write_frame(&mut self.stdin, &request) {
            Ok(()) => {}
            Err(cubex_protocol::ProtocolError::Io(err))
                if err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                return Err(Error::PluginExited { name: name.into() });
            }
            Err(err) => return Err(err.into()),
        }
        match cubex_protocol::read_frame(&mut self.stdout) {
            Ok(Some(response)) => Ok(response),
            Ok(None) => Err(Error::PluginExited { name: name.into() }),
            Err(cubex_protocol::ProtocolError::Io(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                Err(Error::PluginExited { name: name.into() })
            }
            Err(err) => Err(err.into()),
        }
    }

    fn shutdown(mut self, name: &str, source: &str) -> Result<Option<PluginResponse>> {
        let stop = PluginRequest {
            plugin: name.to_string(),
            message: Message::new(source, "system.stop", Payload::Control(Control::Stop)),
        };
        let mut error = None;
        let wrote_stop = match cubex_protocol::write_frame(&mut self.stdin, &stop) {
            Ok(()) => true,
            Err(cubex_protocol::ProtocolError::Io(err))
                if err.kind() == std::io::ErrorKind::BrokenPipe =>
            {
                false
            }
            Err(err) => {
                error = Some(err.into());
                false
            }
        };
        let response = if wrote_stop {
            match cubex_protocol::read_frame::<_, PluginResponse>(&mut self.stdout) {
                Ok(response) => response,
                Err(err) => {
                    error = Some(err.into());
                    None
                }
            }
        } else {
            None
        };
        drop(self.stdin);
        let wait_result = self.child.wait();
        if let Some(err) = error {
            return Err(err);
        }
        match wait_result {
            Ok(status) if status.success() => Ok(response),
            Ok(status) => Err(Error::PluginState {
                plugin: name.into(),
                reason: format!("exited with {status}"),
            }),
            Err(err) => Err(Error::PluginState {
                plugin: name.into(),
                reason: format!("failed to wait for child: {err}"),
            }),
        }
    }
}

struct WasmPlugin {
    name: String,
    path: PathBuf,
    working_dir: Option<PathBuf>,
    args: Vec<String>,
    autostart: bool,
    capabilities: Vec<CapabilityConfig>,
    engine: WasmtimeEngine,
    instance: Mutex<Option<WasmPluginInstance>>,
}

struct WasmPluginInstance {
    store: Store<WasmStoreState>,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    free: TypedFunc<(i32, i32), ()>,
    handle: TypedFunc<(i32, i32), i64>,
}

struct WasmStoreState {
    limits: StoreLimits,
    host: WasmHostContext,
}

struct WasmHostContext {
    plugin: String,
    working_dir: Option<PathBuf>,
    capabilities: Vec<CapabilityConfig>,
}

impl WasmPlugin {
    fn new(
        name: String,
        path: PathBuf,
        working_dir: Option<PathBuf>,
        args: Vec<String>,
        autostart: bool,
        capabilities: Vec<CapabilityConfig>,
    ) -> Result<Self> {
        let mut config = WasmtimeConfig::new();
        config.consume_fuel(true);
        let engine = WasmtimeEngine::new(&config).map_err(|err| Error::PluginState {
            plugin: name.clone(),
            reason: format!("failed to configure wasm engine: {err}"),
        })?;
        Ok(Self {
            name,
            path,
            working_dir,
            args,
            autostart,
            capabilities,
            engine,
            instance: Mutex::new(None),
        })
    }

    fn call(&self, request: PluginRequest) -> Result<PluginResponse> {
        let mut guard = self.instance.lock().map_err(|_| Error::PluginState {
            plugin: self.name.clone(),
            reason: "wasm instance lock poisoned".into(),
        })?;
        if guard.is_none() {
            *guard = Some(self.instantiate()?);
        }
        guard
            .as_mut()
            .ok_or_else(|| Error::PluginState {
                plugin: self.name.clone(),
                reason: "wasm instance was not started".into(),
            })?
            .call(&self.name, request)
    }

    fn shutdown(&self, source: &str) -> Result<Option<PluginResponse>> {
        let mut guard = self.instance.lock().map_err(|_| Error::PluginState {
            plugin: self.name.clone(),
            reason: "wasm instance lock poisoned".into(),
        })?;
        let Some(mut instance) = guard.take() else {
            return Ok(None);
        };
        instance
            .call(
                &self.name,
                PluginRequest {
                    plugin: self.name.clone(),
                    message: Message::new(source, "system.stop", Payload::Control(Control::Stop)),
                },
            )
            .map(Some)
    }

    fn instantiate(&self) -> Result<WasmPluginInstance> {
        let mut store = Store::new(
            &self.engine,
            WasmStoreState {
                limits: wasm_store_limits(),
                host: WasmHostContext {
                    plugin: self.name.clone(),
                    working_dir: self.working_dir.clone(),
                    capabilities: self.capabilities.clone(),
                },
            },
        );
        store.limiter(|state| &mut state.limits);
        set_wasm_fuel(&mut store, &self.name)?;
        let module =
            Module::from_file(&self.engine, &self.path).map_err(|err| Error::PluginState {
                plugin: self.name.clone(),
                reason: format!("failed to load wasm `{}`: {err}", self.path.display()),
            })?;
        let mut linker = Linker::new(&self.engine);
        linker
            .func_wrap("cubex", "host_call", wasm_host_call)
            .map_err(|err| wasm_state(&self.name, err))?;
        let instance =
            linker
                .instantiate(&mut store, &module)
                .map_err(|err| Error::PluginState {
                    plugin: self.name.clone(),
                    reason: format!("failed to instantiate wasm: {err}"),
                })?;
        WasmPluginInstance::new(&self.name, store, instance)
    }
}

impl WasmPluginInstance {
    fn new(name: &str, mut store: Store<WasmStoreState>, instance: Instance) -> Result<Self> {
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| wasm_state(name, "missing wasm export `memory`"))?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "cubex_plugin_alloc")
            .map_err(|err| wasm_state(name, err))?;
        let free = instance
            .get_typed_func::<(i32, i32), ()>(&mut store, "cubex_plugin_free")
            .map_err(|err| wasm_state(name, err))?;
        let handle = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, "cubex_plugin_handle")
            .map_err(|err| wasm_state(name, err))?;
        Ok(Self {
            store,
            memory,
            alloc,
            free,
            handle,
        })
    }

    fn call(&mut self, name: &str, request: PluginRequest) -> Result<PluginResponse> {
        let bytes = cubex_protocol::encode(&request)?;
        let input_len = u32::try_from(bytes.len())
            .map_err(|_| cubex_protocol::ProtocolError::FrameTooLarge(u32::MAX))?;
        if input_len > cubex_protocol::MAX_FRAME_SIZE {
            return Err(cubex_protocol::ProtocolError::FrameTooLarge(input_len).into());
        }
        let input_len = input_len as i32;
        self.refuel(name)?;
        let input_ptr = self
            .alloc
            .call(&mut self.store, input_len)
            .map_err(|err| wasm_state(name, err))?;
        let input_offset = input_ptr as u32 as usize;
        if let Err(err) = self.memory.write(&mut self.store, input_offset, &bytes) {
            self.free_buffer(input_ptr, input_len);
            return Err(wasm_state(name, err));
        }
        self.refuel(name)?;
        let packed = match self.handle.call(&mut self.store, (input_ptr, input_len)) {
            Ok(packed) => packed as u64,
            Err(err) => {
                self.free_buffer(input_ptr, input_len);
                return Err(wasm_state(name, err));
            }
        };
        self.free_buffer(input_ptr, input_len);

        let output_ptr = (packed & u64::from(u32::MAX)) as u32;
        let output_len = (packed >> 32) as u32;
        if output_len > cubex_protocol::MAX_FRAME_SIZE {
            return Err(cubex_protocol::ProtocolError::FrameTooLarge(output_len).into());
        }
        let mut output = vec![0_u8; output_len as usize];
        if let Err(err) = self
            .memory
            .read(&mut self.store, output_ptr as usize, &mut output)
        {
            self.free_buffer(output_ptr as i32, output_len as i32);
            return Err(wasm_state(name, err));
        }
        self.free_buffer(output_ptr as i32, output_len as i32);
        cubex_protocol::decode(&output).map_err(Error::from)
    }

    fn refuel(&mut self, name: &str) -> Result<()> {
        set_wasm_fuel(&mut self.store, name)
    }

    fn free_buffer(&mut self, ptr: i32, len: i32) {
        let _ = self.store.set_fuel(WASM_CALL_FUEL);
        let _ = self.free.call(&mut self.store, (ptr, len));
    }
}

fn wasm_state(name: &str, reason: impl std::fmt::Display) -> Error {
    Error::PluginState {
        plugin: name.into(),
        reason: reason.to_string(),
    }
}

fn set_wasm_fuel(store: &mut Store<WasmStoreState>, name: &str) -> Result<()> {
    store
        .set_fuel(WASM_CALL_FUEL)
        .map_err(|err| wasm_state(name, err))
}

fn wasm_store_limits() -> StoreLimits {
    StoreLimitsBuilder::new()
        .memory_size(WASM_MEMORY_LIMIT_BYTES)
        .instances(1)
        .memories(1)
        .tables(2)
        .trap_on_grow_failure(true)
        .build()
}

fn wasm_host_call(mut caller: Caller<'_, WasmStoreState>, ptr: i32, len: i32) -> i64 {
    let response = match read_host_request(&mut caller, ptr, len)
        .and_then(|request| handle_host_request(caller.data(), request))
    {
        Ok(payload) => HostResponse::ok(payload),
        Err(err) => HostResponse::error(err.to_string()),
    };
    write_host_response(&mut caller, &response).unwrap_or_default()
}

fn read_host_request(
    caller: &mut Caller<'_, WasmStoreState>,
    ptr: i32,
    len: i32,
) -> anyhow::Result<HostRequest> {
    if len <= 0 {
        anyhow::bail!("host request length must be positive");
    }
    let len = u32::try_from(len)?;
    if len > cubex_protocol::MAX_FRAME_SIZE {
        anyhow::bail!("{}", cubex_protocol::ProtocolError::FrameTooLarge(len));
    }
    let memory = caller_memory(caller)?;
    let mut bytes = vec![0_u8; len as usize];
    memory.read(caller, ptr as u32 as usize, &mut bytes)?;
    Ok(cubex_protocol::decode(&bytes)?)
}

fn write_host_response(
    caller: &mut Caller<'_, WasmStoreState>,
    response: &HostResponse,
) -> anyhow::Result<i64> {
    let bytes = cubex_protocol::encode(response)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| cubex_protocol::ProtocolError::FrameTooLarge(u32::MAX))?;
    if len > cubex_protocol::MAX_FRAME_SIZE {
        return Err(cubex_protocol::ProtocolError::FrameTooLarge(len).into());
    }
    let len_i32 = i32::try_from(len)?;
    let alloc = caller
        .get_export("cubex_plugin_alloc")
        .and_then(|export| export.into_func())
        .ok_or_else(|| anyhow::anyhow!("missing wasm export `cubex_plugin_alloc`"))?;
    let alloc = alloc.typed::<i32, i32>(&*caller)?;
    let ptr = alloc.call(&mut *caller, len_i32)?;
    caller_memory(caller)?.write(caller, ptr as u32 as usize, &bytes)?;
    Ok(((len as i64) << 32) | (ptr as u32 as i64))
}

fn caller_memory(caller: &mut Caller<'_, WasmStoreState>) -> anyhow::Result<Memory> {
    caller
        .get_export("memory")
        .and_then(|export| export.into_memory())
        .ok_or_else(|| anyhow::anyhow!("missing wasm export `memory`"))
}

fn handle_host_request(
    state: &WasmStoreState,
    request: HostRequest,
) -> anyhow::Result<HostPayload> {
    match request {
        HostRequest::FileRead { path } => {
            let path = checked_path(&state.host, &path, CapabilityKind::FileRead)?;
            Ok(HostPayload::Bytes(std::fs::read(path)?))
        }
        HostRequest::FileWrite { path, bytes } => {
            let path = checked_path(&state.host, &path, CapabilityKind::FileWrite)?;
            write_file_atomically(&path, &bytes)?;
            Ok(HostPayload::Unit)
        }
        HostRequest::TcpRequest {
            addr,
            bytes,
            timeout_ms,
        } => {
            check_addr(&state.host, &addr, CapabilityKind::TcpConnect)?;
            Ok(HostPayload::Bytes(tcp_request(&addr, &bytes, timeout_ms)?))
        }
        HostRequest::TcpEcho {
            addr,
            max_connections,
        } => {
            check_addr(&state.host, &addr, CapabilityKind::TcpListen)?;
            Ok(HostPayload::Text(tcp_echo(&addr, max_connections)?))
        }
        HostRequest::Sleep { millis } => {
            check_timer(&state.host)?;
            if millis > MAX_WASM_SLEEP_MS {
                anyhow::bail!("sleep must be at most {MAX_WASM_SLEEP_MS} ms");
            }
            std::thread::sleep(Duration::from_millis(millis));
            Ok(HostPayload::Unit)
        }
        HostRequest::RecordPut { path, key, message } => {
            let path = checked_path(&state.host, &path, CapabilityKind::RecordStore)?;
            cubex_store::RecordStore::new(path).put(key, message)?;
            Ok(HostPayload::Unit)
        }
        HostRequest::RecordGet { path, key } => {
            let path = checked_path(&state.host, &path, CapabilityKind::RecordStore)?;
            let message = cubex_store::RecordStore::new(path)
                .get(&key)?
                .map(|record| record.message);
            Ok(HostPayload::Message(message))
        }
        HostRequest::RecordDelete { path, key } => {
            let path = checked_path(&state.host, &path, CapabilityKind::RecordStore)?;
            Ok(HostPayload::Bool(
                cubex_store::RecordStore::new(path).delete(&key)?,
            ))
        }
        HostRequest::RecordList { path } => {
            let path = checked_path(&state.host, &path, CapabilityKind::RecordStore)?;
            let keys = cubex_store::RecordStore::new(path)
                .load()?
                .keys()
                .cloned()
                .collect();
            Ok(HostPayload::StringList(keys))
        }
    }
}

#[derive(Copy, Clone)]
enum CapabilityKind {
    FileRead,
    FileWrite,
    TcpConnect,
    TcpListen,
    RecordStore,
}

fn checked_path(
    host: &WasmHostContext,
    requested: &str,
    kind: CapabilityKind,
) -> anyhow::Result<PathBuf> {
    let requested = plugin_path(host, requested)?;
    for capability in &host.capabilities {
        let allowed = match (kind, capability) {
            (CapabilityKind::FileRead, CapabilityConfig::FileRead { path })
            | (CapabilityKind::FileWrite, CapabilityConfig::FileWrite { path })
            | (CapabilityKind::RecordStore, CapabilityConfig::RecordStore { path }) => {
                normalize_host_path(path.clone())?
            }
            _ => continue,
        };
        if requested == allowed {
            return Ok(requested);
        }
    }
    anyhow::bail!(
        "plugin `{}` lacks capability for path {}",
        host.plugin,
        requested.display()
    )
}

fn plugin_path(host: &WasmHostContext, requested: &str) -> anyhow::Result<PathBuf> {
    if requested.trim().is_empty() {
        anyhow::bail!("capability path must not be empty");
    }
    if requested.trim() != requested {
        anyhow::bail!("capability path must not be padded");
    }
    let path = PathBuf::from(requested);
    let path = if path.is_relative() {
        host.working_dir
            .clone()
            .unwrap_or(std::env::current_dir()?)
            .join(path)
    } else {
        path
    };
    normalize_host_path(path)
}

fn normalize_host_path(path: PathBuf) -> anyhow::Result<PathBuf> {
    Ok(std::path::absolute(path)?)
}

fn check_addr(host: &WasmHostContext, requested: &str, kind: CapabilityKind) -> anyhow::Result<()> {
    if requested.trim().is_empty() {
        anyhow::bail!("capability address must not be empty");
    }
    if requested.trim() != requested {
        anyhow::bail!("capability address must not be padded");
    }
    for capability in &host.capabilities {
        let allowed = match (kind, capability) {
            (CapabilityKind::TcpConnect, CapabilityConfig::TcpConnect { addr })
            | (CapabilityKind::TcpListen, CapabilityConfig::TcpListen { addr }) => addr,
            _ => continue,
        };
        if requested == allowed {
            return Ok(());
        }
    }
    anyhow::bail!(
        "plugin `{}` lacks capability for address {}",
        host.plugin,
        requested
    )
}

fn check_timer(host: &WasmHostContext) -> anyhow::Result<()> {
    if host
        .capabilities
        .iter()
        .any(|capability| matches!(capability, CapabilityConfig::Timer))
    {
        Ok(())
    } else {
        anyhow::bail!("plugin `{}` lacks timer capability", host.plugin)
    }
}

fn tcp_request(addr: &str, bytes: &[u8], timeout_ms: u64) -> anyhow::Result<Vec<u8>> {
    if timeout_ms == 0 {
        anyhow::bail!("tcp timeout must be positive");
    }
    if timeout_ms > MAX_WASM_TCP_TIMEOUT_MS {
        anyhow::bail!("tcp timeout must be at most {MAX_WASM_TCP_TIMEOUT_MS} ms");
    }
    let timeout = Duration::from_millis(timeout_ms);
    let mut stream = connect_tcp(addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(bytes)?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    Ok(response)
}

fn connect_tcp(addr: &str, timeout: Duration) -> anyhow::Result<TcpStream> {
    let mut resolved = false;
    let mut last_error = None;
    for addr in addr.to_socket_addrs()? {
        resolved = true;
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_error = Some(err),
        }
    }
    if !resolved {
        anyhow::bail!("tcp address did not resolve");
    }
    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("tcp connect failed"))
        .into())
}

fn tcp_echo(addr: &str, max_connections: u64) -> anyhow::Result<String> {
    if max_connections == 0 {
        anyhow::bail!("tcp max connections must be positive");
    }
    if max_connections > MAX_WASM_TCP_ECHO_CONNECTIONS {
        anyhow::bail!("tcp max connections must be at most {MAX_WASM_TCP_ECHO_CONNECTIONS}");
    }
    if addr_requests_ephemeral_port(addr) {
        anyhow::bail!("tcp listen address must not use port 0");
    }
    let max_connections = usize::try_from(max_connections)?;
    let listener = TcpListener::bind(addr)?;
    let local_addr = listener.local_addr()?.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(max_connections) {
            let Ok(mut stream) = stream else {
                break;
            };
            let mut buf = Vec::new();
            if stream.read_to_end(&mut buf).is_ok() {
                let _ = stream.write_all(&buf);
            }
        }
    });
    Ok(local_addr)
}

fn addr_requests_ephemeral_port(addr: &str) -> bool {
    addr.rsplit_once(':').is_some_and(|(_, port)| port == "0")
}

fn write_file_atomically(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = write_host_temp_file(path, bytes)?;
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

fn write_host_temp_file(path: &Path, bytes: &[u8]) -> anyhow::Result<PathBuf> {
    for attempt in 0..1000 {
        let tmp = host_temp_file_path(path, attempt);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
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
    anyhow::bail!("could not create host temporary file")
}

fn host_temp_file_path(path: &Path, attempt: u16) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("cubex-host")),
    );
    name.push(format!(".{attempt}.tmp"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, EngineConfig, PluginConfig, RouteConfig, RouteValue, StoreConfig};
    use cubex_protocol::{PayloadKind, Value};
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn route_matches_only_declared_fields() {
        let route = RouteConfig {
            name: "text-from-a".into(),
            source: Some("a".into()),
            topic: None,
            payload: Some(PayloadKind::Text),
            record: BTreeMap::new(),
            to: vec!["b".into()],
        };

        assert!(route.matches(&Message::new("a", "topic", Payload::Text("ok".into()))));
        assert!(!route.matches(&Message::new("b", "topic", Payload::Text("ok".into()))));
        assert!(!route.matches(&Message::new("a", "topic", Payload::Bytes(vec![1]))));
    }

    #[test]
    fn route_matches_declared_topic() {
        let route = RouteConfig {
            name: "topic-only".into(),
            source: None,
            topic: Some("wanted.topic".into()),
            payload: None,
            record: BTreeMap::new(),
            to: vec!["b".into()],
        };

        assert!(route.matches(&Message::new(
            "a",
            "wanted.topic",
            Payload::Text("ok".into())
        )));
        assert!(!route.matches(&Message::new(
            "a",
            "other.topic",
            Payload::Text("ok".into())
        )));
    }

    #[test]
    fn route_matches_declared_record_fields() {
        let route = RouteConfig {
            name: "alice-records".into(),
            source: None,
            topic: Some("record.put".into()),
            payload: Some(PayloadKind::Record),
            record: BTreeMap::from([
                ("user".into(), RouteValue::String("alice".into())),
                ("priority".into(), RouteValue::I64(7)),
                ("active".into(), RouteValue::Bool(true)),
            ]),
            to: vec!["policy".into()],
        };
        let matching_record = BTreeMap::from([
            ("user".into(), Value::String("alice".into())),
            ("priority".into(), Value::U64(7)),
            ("active".into(), Value::Bool(true)),
        ]);
        let wrong_record = BTreeMap::from([
            ("user".into(), Value::String("bob".into())),
            ("priority".into(), Value::U64(7)),
            ("active".into(), Value::Bool(true)),
        ]);

        assert!(route.matches(&Message::new(
            "source",
            "record.put",
            Payload::Record(matching_record)
        )));
        assert!(!route.matches(&Message::new(
            "source",
            "record.put",
            Payload::Record(wrong_record)
        )));
        assert!(!route.matches(&Message::new(
            "source",
            "record.put",
            Payload::Text("alice".into())
        )));
    }

    #[test]
    fn config_parses_record_route_fields() {
        let config: Config = toml::from_str(
            r#"
[[plugins]]
name = "policy"
command = "policy"

[[routes]]
name = "alice-records"
payload = "record"
record = { user = "alice", priority = 7, active = true }
to = ["policy"]
"#,
        )
        .unwrap();

        assert_eq!(
            config.routes[0].record,
            BTreeMap::from([
                ("user".into(), RouteValue::String("alice".into())),
                ("priority".into(), RouteValue::I64(7)),
                ("active".into(), RouteValue::Bool(true)),
            ])
        );
    }

    #[test]
    fn record_route_requires_record_payload_filter() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "bad-record-route".into(),
                source: None,
                topic: None,
                payload: Some(PayloadKind::Text),
                record: BTreeMap::from([("user".into(), RouteValue::String("alice".into()))]),
                to: vec!["print".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message))
                if message == "route `bad-record-route` record match requires payload `record` or no payload filter"
        ));
    }

    #[test]
    fn plugin_error_reason_must_be_readable() {
        assert!(matches!(
            plugin_error("bad", "".into()),
            Error::InvalidPluginMessage { plugin, reason }
                if plugin == "bad" && reason == "error must not be empty"
        ));
        assert!(matches!(
            plugin_error("bad", " boom".into()),
            Error::InvalidPluginMessage { plugin, reason }
                if plugin == "bad" && reason == "error must not have leading or trailing whitespace"
        ));
        assert!(matches!(
            plugin_error("bad", "boom".into()),
            Error::PluginError { plugin, reason } if plugin == "bad" && reason == "boom"
        ));
    }

    #[test]
    fn error_response_must_not_emit_messages() {
        let response = PluginResponse {
            messages: vec![Message::new("bad", "late", Payload::Text("ignored".into()))],
            logs: Vec::new(),
            error: Some("boom".into()),
        };

        assert!(matches!(
            validate_error_response("bad", &response),
            Err(Error::InvalidPluginMessage { plugin, reason })
                if plugin == "bad" && reason == "error response must not emit messages"
        ));
    }

    #[test]
    fn emitted_topics_must_not_be_empty() {
        let mut messages = vec![Message::new("plugin", " ", Payload::Text("bad".into()))];

        assert!(matches!(
            normalize_plugin_messages("plugin", &mut messages),
            Err(Error::InvalidPluginMessage { plugin, reason })
                if plugin == "plugin" && reason == "topic must not be empty"
        ));
    }

    #[test]
    fn emitted_topics_must_not_have_edge_whitespace() {
        let mut messages = vec![Message::new(
            "plugin",
            " topic",
            Payload::Text("bad".into()),
        )];

        assert!(matches!(
            normalize_plugin_messages("plugin", &mut messages),
            Err(Error::InvalidPluginMessage { plugin, reason })
                if plugin == "plugin" && reason == "topic must not have leading or trailing whitespace"
        ));
    }

    #[test]
    fn emitted_messages_must_not_be_control_payloads() {
        let mut messages = vec![Message::new(
            "plugin",
            "system.stop",
            Payload::Control(Control::Stop),
        )];

        assert!(matches!(
            normalize_plugin_messages("plugin", &mut messages),
            Err(Error::InvalidPluginMessage { plugin, reason })
                if plugin == "plugin" && reason == "control payloads are reserved for host messages"
        ));
    }

    #[test]
    fn emitted_message_ids_must_not_be_nil() {
        let mut messages = vec![Message {
            id: uuid::Uuid::nil(),
            source: "plugin".into(),
            topic: "topic".into(),
            payload: Payload::Text("bad".into()),
        }];

        assert!(matches!(
            normalize_plugin_messages("plugin", &mut messages),
            Err(Error::InvalidPluginMessage { plugin, reason })
                if plugin == "plugin" && reason == "id must not be nil"
        ));
    }

    #[test]
    fn duplicate_plugin_names_are_rejected() {
        let config = Config {
            plugins: vec![plugin("print"), plugin("print")],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::DuplicatePlugin(name)) if name == "print"
        ));
    }

    #[test]
    fn plugin_name_must_not_match_engine_name() {
        let config = Config {
            engine: EngineConfig {
                name: "runtime".into(),
                max_messages: 32,
            },
            plugins: vec![plugin("runtime")],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "plugin `runtime` must not use engine.name"
        ));
    }

    #[test]
    fn empty_names_are_rejected() {
        let config = Config {
            engine: EngineConfig {
                name: " ".into(),
                max_messages: 32,
            },
            ..Config::default()
        };
        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));

        let config = Config {
            plugins: vec![plugin(" ")],
            ..Config::default()
        };
        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));

        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: " ".into(),
                source: None,
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec!["print".into()],
            }],
            ..Config::default()
        };
        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn identity_fields_must_not_have_edge_whitespace() {
        for config in [
            Config {
                engine: EngineConfig {
                    name: " cubex".into(),
                    max_messages: 32,
                },
                ..Config::default()
            },
            Config {
                plugins: vec![plugin(" print")],
                ..Config::default()
            },
            Config {
                plugins: vec![plugin("print")],
                routes: vec![RouteConfig {
                    name: " route".into(),
                    source: None,
                    topic: None,
                    payload: None,
                    record: BTreeMap::new(),
                    to: vec!["print".into()],
                }],
                ..Config::default()
            },
            Config {
                plugins: vec![plugin("print")],
                routes: vec![RouteConfig {
                    name: "bad-source".into(),
                    source: Some(" print".into()),
                    topic: None,
                    payload: None,
                    record: BTreeMap::new(),
                    to: vec!["print".into()],
                }],
                ..Config::default()
            },
            Config {
                plugins: vec![plugin("print")],
                routes: vec![RouteConfig {
                    name: "bad-topic".into(),
                    source: None,
                    topic: Some(" topic".into()),
                    payload: None,
                    record: BTreeMap::new(),
                    to: vec!["print".into()],
                }],
                ..Config::default()
            },
            Config {
                plugins: vec![plugin("print")],
                routes: vec![RouteConfig {
                    name: "bad-target".into(),
                    source: None,
                    topic: None,
                    payload: None,
                    record: BTreeMap::new(),
                    to: vec![" print".into()],
                }],
                ..Config::default()
            },
            Config {
                plugins: vec![plugin("print")],
                routes: vec![RouteConfig {
                    name: "bad-record-key".into(),
                    source: None,
                    topic: None,
                    payload: Some(PayloadKind::Record),
                    record: BTreeMap::from([(" user".into(), RouteValue::String("alice".into()))]),
                    to: vec!["print".into()],
                }],
                ..Config::default()
            },
        ] {
            assert!(matches!(
                Engine::from_config(config),
                Err(Error::InvalidConfig(_))
            ));
        }
    }

    #[test]
    fn zero_message_limit_is_rejected() {
        let config = Config {
            engine: EngineConfig {
                name: "test".into(),
                max_messages: 0,
            },
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn replay_requires_store_path() {
        let config = Config {
            store: StoreConfig {
                path: None,
                replay_on_start: true,
            },
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "store.replay_on_start requires store.path"
        ));
    }

    #[test]
    fn replay_rejects_control_payloads() {
        let path =
            std::env::temp_dir().join(format!("cubex-replay-control-{}.bin", uuid::Uuid::new_v4()));
        write_event(
            &path,
            &Message::new("stored", "system.stop", Payload::Control(Control::Stop)),
        );

        let engine = Engine::from_config(Config {
            store: StoreConfig {
                path: Some(path.clone()),
                replay_on_start: true,
            },
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::Store(cubex_store::StoreError::InvalidEventMessage(reason)))
                if reason == "control payloads are reserved for host messages"
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn replay_rejects_unknown_sources() {
        let path =
            std::env::temp_dir().join(format!("cubex-replay-source-{}.bin", uuid::Uuid::new_v4()));
        EventLog::new(&path)
            .append(&Message::new(
                "missing-plugin",
                "stored.topic",
                Payload::Text("stored".into()),
            ))
            .unwrap();

        let engine = Engine::from_config(Config {
            store: StoreConfig {
                path: Some(path.clone()),
                replay_on_start: true,
            },
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::InvalidStoredMessage(reason))
                if reason == "source `missing-plugin` is not configured"
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn replay_rejects_nil_message_ids() {
        let path =
            std::env::temp_dir().join(format!("cubex-replay-nil-id-{}.bin", uuid::Uuid::new_v4()));
        write_event(
            &path,
            &Message {
                id: uuid::Uuid::nil(),
                source: "cubex".into(),
                topic: "stored.topic".into(),
                payload: Payload::Text("stored".into()),
            },
        );

        let engine = Engine::from_config(Config {
            store: StoreConfig {
                path: Some(path.clone()),
                replay_on_start: true,
            },
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::Store(cubex_store::StoreError::InvalidEventMessage(reason)))
                if reason == "id must not be nil"
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_store_path_is_rejected() {
        let mut config = Config {
            store: StoreConfig {
                path: Some(PathBuf::new()),
                replay_on_start: false,
            },
            ..Config::default()
        };
        config.resolve_relative_paths(Path::new("/tmp"));

        assert!(config.store.path.as_ref().unwrap().as_os_str().is_empty());
        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "store.path must not be empty"
        ));
    }

    #[test]
    fn blank_store_path_is_rejected() {
        let config = Config {
            store: StoreConfig {
                path: Some(PathBuf::from(" ")),
                replay_on_start: false,
            },
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "store.path must not be blank"
        ));
    }

    #[test]
    fn empty_plugin_command_is_rejected() {
        let mut plugin = plugin("print");
        plugin.command = PathBuf::new();
        let config = Config {
            plugins: vec![plugin],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn blank_plugin_command_is_rejected() {
        let mut plugin = plugin("print");
        plugin.command = PathBuf::from(" ");
        let config = Config {
            plugins: vec![plugin],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "plugin.command must not be blank"
        ));
    }

    #[test]
    fn plugin_backend_must_be_command_or_wasm() {
        let mut plugin = plugin("print");
        plugin.wasm = Some("plugin.wasm".into());
        let config = Config {
            plugins: vec![plugin],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message))
                if message == "plugin.command and plugin.wasm are mutually exclusive"
        ));
    }

    #[test]
    fn wasm_plugin_config_does_not_load_module_eagerly() {
        let path =
            std::env::temp_dir().join(format!("cubex-missing-{}.wasm", uuid::Uuid::new_v4()));
        let _ = std::fs::remove_file(&path);
        let config = Config {
            plugins: vec![PluginConfig {
                name: "wasm".into(),
                command: PathBuf::new(),
                wasm: Some(path.clone()),
                working_dir: None,
                args: Vec::new(),
                autostart: false,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        };

        assert!(Engine::from_config(config).is_ok());
    }

    #[test]
    fn process_plugin_capabilities_are_rejected() {
        let mut plugin = plugin("print");
        plugin.capabilities.push(CapabilityConfig::Timer);
        let config = Config {
            plugins: vec![plugin],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message))
                if message == "plugin.capabilities require plugin.wasm"
        ));
    }

    #[test]
    fn wasm_host_file_request_path_must_not_be_padded() {
        let dir = std::env::temp_dir().join(format!("cubex-wasm-cap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let state = WasmStoreState {
            limits: wasm_store_limits(),
            host: WasmHostContext {
                plugin: "wasm".into(),
                working_dir: Some(dir.clone()),
                capabilities: vec![CapabilityConfig::FileRead {
                    path: dir.join("input.txt"),
                }],
            },
        };

        let err = handle_host_request(
            &state,
            HostRequest::FileRead {
                path: " input.txt".into(),
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "capability path must not be padded");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wasm_host_address_request_must_not_be_padded() {
        let state = WasmStoreState {
            limits: wasm_store_limits(),
            host: WasmHostContext {
                plugin: "wasm".into(),
                working_dir: None,
                capabilities: vec![CapabilityConfig::TcpConnect {
                    addr: "127.0.0.1:41021".into(),
                }],
            },
        };

        let err = handle_host_request(
            &state,
            HostRequest::TcpRequest {
                addr: "127.0.0.1:41021 ".into(),
                bytes: Vec::new(),
                timeout_ms: 1,
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "capability address must not be padded");
    }

    #[test]
    fn wasm_host_tcp_request_timeout_is_capped() {
        let state = WasmStoreState {
            limits: wasm_store_limits(),
            host: WasmHostContext {
                plugin: "wasm".into(),
                working_dir: None,
                capabilities: vec![CapabilityConfig::TcpConnect {
                    addr: "127.0.0.1:41021".into(),
                }],
            },
        };

        let err = handle_host_request(
            &state,
            HostRequest::TcpRequest {
                addr: "127.0.0.1:41021".into(),
                bytes: Vec::new(),
                timeout_ms: MAX_WASM_TCP_TIMEOUT_MS + 1,
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "tcp timeout must be at most 60000 ms");
    }

    #[test]
    fn wasm_host_tcp_echo_connections_are_capped() {
        let state = WasmStoreState {
            limits: wasm_store_limits(),
            host: WasmHostContext {
                plugin: "wasm".into(),
                working_dir: None,
                capabilities: vec![CapabilityConfig::TcpListen {
                    addr: "127.0.0.1:41021".into(),
                }],
            },
        };

        let err = handle_host_request(
            &state,
            HostRequest::TcpEcho {
                addr: "127.0.0.1:41021".into(),
                max_connections: MAX_WASM_TCP_ECHO_CONNECTIONS + 1,
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "tcp max connections must be at most 1024");
    }

    #[test]
    fn wasm_host_tcp_echo_rejects_ephemeral_port() {
        let state = WasmStoreState {
            limits: wasm_store_limits(),
            host: WasmHostContext {
                plugin: "wasm".into(),
                working_dir: None,
                capabilities: vec![CapabilityConfig::TcpListen {
                    addr: "127.0.0.1:0".into(),
                }],
            },
        };

        let err = handle_host_request(
            &state,
            HostRequest::TcpEcho {
                addr: "127.0.0.1:0".into(),
                max_connections: 1,
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "tcp listen address must not use port 0");
    }

    #[test]
    fn wasm_host_sleep_is_capped() {
        let state = WasmStoreState {
            limits: wasm_store_limits(),
            host: WasmHostContext {
                plugin: "wasm".into(),
                working_dir: None,
                capabilities: vec![CapabilityConfig::Timer],
            },
        };

        let err = handle_host_request(
            &state,
            HostRequest::Sleep {
                millis: MAX_WASM_SLEEP_MS + 1,
            },
        )
        .unwrap_err();

        assert_eq!(err.to_string(), "sleep must be at most 60000 ms");
    }

    #[test]
    fn path_resolution_preserves_empty_plugin_command() {
        let mut config = Config {
            plugins: vec![plugin("print")],
            ..Config::default()
        };
        config.plugins[0].command = PathBuf::new();
        config.resolve_relative_paths(Path::new("/tmp"));

        assert!(config.plugins[0].command.as_os_str().is_empty());
        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn empty_working_dir_is_rejected() {
        let mut config = Config {
            plugins: vec![plugin("print")],
            ..Config::default()
        };
        config.plugins[0].working_dir = Some(PathBuf::new());
        config.resolve_relative_paths(Path::new("/tmp"));

        assert!(
            config.plugins[0]
                .working_dir
                .as_ref()
                .unwrap()
                .as_os_str()
                .is_empty()
        );
        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "plugin.working_dir must not be empty"
        ));
    }

    #[test]
    fn blank_working_dir_is_rejected() {
        let mut config = Config {
            plugins: vec![plugin("print")],
            ..Config::default()
        };
        config.plugins[0].working_dir = Some(PathBuf::from(" "));

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "plugin.working_dir must not be blank"
        ));
    }

    #[test]
    fn path_resolution_preserves_blank_paths_for_validation() {
        let mut config = Config {
            store: StoreConfig {
                path: Some(PathBuf::from(" ")),
                replay_on_start: false,
            },
            plugins: vec![PluginConfig {
                name: "print".into(),
                command: PathBuf::from(" "),
                wasm: None,
                working_dir: Some(PathBuf::from(" ")),
                args: Vec::new(),
                autostart: false,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        };

        config.resolve_relative_paths(Path::new("/tmp"));

        assert_eq!(config.store.path.as_deref(), Some(Path::new(" ")));
        assert_eq!(config.plugins[0].command, PathBuf::from(" "));
        assert_eq!(config.plugins[0].working_dir, Some(PathBuf::from(" ")));
    }

    #[test]
    fn config_file_resolves_relative_paths_from_its_directory() {
        let dir = std::env::temp_dir().join(format!("cubex-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("cubex.toml");
        std::fs::write(
            &path,
            r#"
[store]
path = "events.bin"

[[plugins]]
name = "plugin"
command = "bin/plugin"
working_dir = "work"

[[plugins]]
name = "wasm-plugin"
wasm = "wasm/plugin.wasm"

[[plugins.capabilities]]
kind = "file-read"
path = "data/input.txt"
"#,
        )
        .unwrap();

        let config = Config::from_file(&path).unwrap();

        assert_eq!(
            config.store.path.as_deref(),
            Some(dir.join("events.bin").as_path())
        );
        assert_eq!(config.plugins[0].command, dir.join("bin/plugin"));
        assert_eq!(
            config.plugins[0].working_dir.as_deref(),
            Some(dir.join("work").as_path())
        );
        assert_eq!(
            config.plugins[1].wasm.as_deref(),
            Some(dir.join("wasm/plugin.wasm").as_path())
        );
        let CapabilityConfig::FileRead { path } = &config.plugins[1].capabilities[0] else {
            panic!("expected file-read capability");
        };
        assert_eq!(path, &dir.join("data/input.txt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_config_fields_are_rejected() {
        for text in [
            "unexpected = true",
            "[engine]\nname = \"test\"\nunexpected = true",
            "[store]\npath = \"events.bin\"\nunexpected = true",
            "[[plugins]]\nname = \"plugin\"\ncommand = \"bin/plugin\"\nunexpected = true",
            "[[routes]]\nname = \"route\"\nto = [\"plugin\"]\nunexpected = true",
        ] {
            assert!(toml::from_str::<Config>(text).is_err());
        }
    }

    #[test]
    fn invalid_route_payload_kind_is_rejected() {
        let text = r#"
[[routes]]
name = "route"
payload = "json"
to = ["plugin"]
"#;

        assert!(toml::from_str::<Config>(text).is_err());
    }

    #[test]
    fn duplicate_route_names_are_rejected() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![
                RouteConfig {
                    name: "same".into(),
                    source: None,
                    topic: None,
                    payload: None,
                    record: BTreeMap::new(),
                    to: vec!["print".into()],
                },
                RouteConfig {
                    name: "same".into(),
                    source: None,
                    topic: None,
                    payload: None,
                    record: BTreeMap::new(),
                    to: vec!["print".into()],
                },
            ],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn route_targets_must_exist() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "bad-route".into(),
                source: None,
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec!["missing".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::UnknownRouteTarget { route, target })
                if route == "bad-route" && target == "missing"
        ));
    }

    #[test]
    fn route_targets_must_not_be_empty() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "bad-target".into(),
                source: None,
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec![" ".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn duplicate_route_targets_are_rejected() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "dupe-target".into(),
                source: None,
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec!["print".into(), "print".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(_))
        ));
    }

    #[test]
    fn route_sources_must_exist() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "bad-source".into(),
                source: Some("missing".into()),
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec!["print".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::UnknownRouteSource { route, source_name })
                if route == "bad-source" && source_name == "missing"
        ));
    }

    #[test]
    fn route_source_must_not_be_empty() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "bad-source".into(),
                source: Some(" ".into()),
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec!["print".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "route `bad-source` source must not be empty"
        ));
    }

    #[test]
    fn route_topic_must_not_be_empty() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "bad-topic".into(),
                source: None,
                topic: Some(" ".into()),
                payload: None,
                record: BTreeMap::new(),
                to: vec!["print".into()],
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::InvalidConfig(message)) if message == "route `bad-topic` topic must not be empty"
        ));
    }

    #[test]
    fn route_source_can_be_engine_name() {
        let config = Config {
            engine: EngineConfig {
                name: "engine".into(),
                max_messages: 32,
            },
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "engine-source".into(),
                source: Some("engine".into()),
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: vec!["print".into()],
            }],
            ..Config::default()
        };

        assert!(Engine::from_config(config).is_ok());
    }

    #[test]
    fn routes_need_at_least_one_target() {
        let config = Config {
            plugins: vec![plugin("print")],
            routes: vec![RouteConfig {
                name: "empty-route".into(),
                source: None,
                topic: None,
                payload: None,
                record: BTreeMap::new(),
                to: Vec::new(),
            }],
            ..Config::default()
        };

        assert!(matches!(
            Engine::from_config(config),
            Err(Error::EmptyRouteTargets(name)) if name == "empty-route"
        ));
    }

    #[test]
    fn poisoned_plugin_child_lock_is_error() {
        let plugin = ProcessPlugin::new(plugin("poisoned")).unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = plugin.child.lock().unwrap();
            panic!("poison child lock");
        }));
        assert!(panic.is_err());

        let request = PluginRequest {
            plugin: "poisoned".into(),
            message: Message::new("test", "topic", Payload::Text("x".into())),
        };

        assert!(matches!(
            plugin.call(request),
            Err(Error::PluginState { plugin, reason })
                if plugin == "poisoned" && reason == "child lock poisoned"
        ));
    }

    #[test]
    fn spawn_error_names_plugin() {
        let mut config = plugin("missing-command");
        config.command = PathBuf::from("/definitely/not/a/cubex/plugin");
        let plugin = ProcessPlugin::new(config).unwrap();
        let request = PluginRequest {
            plugin: "missing-command".into(),
            message: Message::new("test", "topic", Payload::Text("x".into())),
        };

        assert!(matches!(
            plugin.call(request),
            Err(Error::PluginState { plugin, reason })
                if plugin == "missing-command" && reason.contains("failed to spawn")
        ));
    }

    #[test]
    fn run_keeps_primary_error_after_cleanup() {
        let path = std::env::temp_dir().join(format!("cubex-test-{}.bin", uuid::Uuid::new_v4()));
        let store = EventLog::new(&path);
        store
            .append(&Message::new("test", "topic", Payload::Text("x".into())))
            .unwrap();
        store
            .append(&Message::new("test", "topic", Payload::Text("y".into())))
            .unwrap();

        let engine = Engine::from_config(Config {
            engine: EngineConfig {
                name: "test".into(),
                max_messages: 1,
            },
            store: StoreConfig {
                path: Some(path.clone()),
                replay_on_start: true,
            },
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(engine.run(), Err(Error::MessageLimit(1))));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn run_reports_cleanup_error_when_main_run_succeeds() {
        let engine = Engine::from_config(Config {
            plugins: vec![plugin("poisoned")],
            ..Config::default()
        })
        .unwrap();
        let RuntimePlugin::Process(plugin) = engine.plugins.get("poisoned").unwrap() else {
            panic!("expected process plugin");
        };
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = plugin.child.lock().unwrap();
            panic!("poison child lock");
        }));
        assert!(panic.is_err());

        assert!(matches!(
            engine.run(),
            Err(Error::PluginState { plugin, reason })
                if plugin == "poisoned" && reason == "child lock poisoned"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_reads_and_validates_stop_response() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("cubex-plugin-stop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let start_response_path = dir.join("start-response.bin");
        let bad_response_path = dir.join("bad-response.bin");
        let stopped_path = dir.join("stopped.txt");
        let script_path = dir.join("plugin.sh");

        let mut start_response_file = std::fs::File::create(&start_response_path).unwrap();
        cubex_protocol::write_frame(&mut start_response_file, &PluginResponse::default()).unwrap();
        std::fs::write(&bad_response_path, [1_u8, 0, 0, 0, 0]).unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/bad-response.bin\"\nwait\nsleep 0.2\nprintf done > \"$(dirname \"$0\")/stopped.txt\"\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            plugins: vec![PluginConfig {
                name: "bad-stop".into(),
                command: script_path,
                wasm: None,
                working_dir: None,
                args: Vec::new(),
                autostart: true,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::Protocol(cubex_protocol::ProtocolError::Codec(_)))
        ));
        assert_eq!(std::fs::read_to_string(stopped_path).unwrap(), "done");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_continues_after_plugin_stop_error() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "cubex-plugin-stop-continue-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&dir).unwrap();
        let start_response_path = dir.join("start-response.bin");
        let good_stop_response_path = dir.join("good-stop-response.bin");
        let bad_response_path = dir.join("bad-response.bin");
        let good_script_path = dir.join("good-plugin.sh");
        let bad_script_path = dir.join("bad-plugin.sh");

        let mut start_response_file = std::fs::File::create(&start_response_path).unwrap();
        cubex_protocol::write_frame(&mut start_response_file, &PluginResponse::default()).unwrap();
        let mut good_stop_response_file = std::fs::File::create(&good_stop_response_path).unwrap();
        cubex_protocol::write_frame(
            &mut good_stop_response_file,
            &PluginResponse {
                messages: Vec::new(),
                logs: vec!["good stopped".into()],
                error: None,
            },
        )
        .unwrap();
        std::fs::write(&bad_response_path, [1_u8, 0, 0, 0, 0]).unwrap();

        let mut good_script = std::fs::File::create(&good_script_path).unwrap();
        good_script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/good-stop-response.bin\"\nwait\n",
            )
            .unwrap();
        let mut bad_script = std::fs::File::create(&bad_script_path).unwrap();
        bad_script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/bad-response.bin\"\nwait\n",
            )
            .unwrap();
        for path in [&good_script_path, &bad_script_path] {
            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions).unwrap();
        }

        let engine = Engine::from_config(Config {
            plugins: vec![
                PluginConfig {
                    name: "good-stop".into(),
                    command: good_script_path,
                    wasm: None,
                    working_dir: None,
                    args: Vec::new(),
                    autostart: true,
                    capabilities: Vec::new(),
                },
                PluginConfig {
                    name: "bad-stop".into(),
                    command: bad_script_path,
                    wasm: None,
                    working_dir: None,
                    args: Vec::new(),
                    autostart: true,
                    capabilities: Vec::new(),
                },
            ],
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::Protocol(cubex_protocol::ProtocolError::Codec(_)))
        ));
        let RuntimePlugin::Process(plugin) = engine.plugins.get("good-stop").unwrap() else {
            panic!("expected process plugin");
        };
        assert!(plugin.child.lock().unwrap().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_logs_are_reported() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("cubex-plugin-stop-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let start_response_path = dir.join("start-response.bin");
        let stop_response_path = dir.join("stop-response.bin");
        let script_path = dir.join("plugin.sh");

        let mut start_response_file = std::fs::File::create(&start_response_path).unwrap();
        cubex_protocol::write_frame(&mut start_response_file, &PluginResponse::default()).unwrap();
        let mut stop_response_file = std::fs::File::create(&stop_response_path).unwrap();
        cubex_protocol::write_frame(
            &mut stop_response_file,
            &PluginResponse {
                messages: Vec::new(),
                logs: vec!["stopped cleanly".into()],
                error: None,
            },
        )
        .unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/stop-response.bin\"\nwait\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            plugins: vec![PluginConfig {
                name: "stop-log".into(),
                command: script_path,
                wasm: None,
                working_dir: None,
                args: Vec::new(),
                autostart: true,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        })
        .unwrap();

        let report = engine.run().unwrap();
        assert_eq!(report.logs, vec!["stopped cleanly"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_rejects_nonzero_exit_status() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("cubex-plugin-stop-exit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let start_response_path = dir.join("start-response.bin");
        let stop_response_path = dir.join("stop-response.bin");
        let script_path = dir.join("plugin.sh");

        let mut start_response_file = std::fs::File::create(&start_response_path).unwrap();
        cubex_protocol::write_frame(&mut start_response_file, &PluginResponse::default()).unwrap();
        let mut stop_response_file = std::fs::File::create(&stop_response_path).unwrap();
        cubex_protocol::write_frame(&mut stop_response_file, &PluginResponse::default()).unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/stop-response.bin\"\nwait\nexit 7\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            plugins: vec![PluginConfig {
                name: "bad-exit".into(),
                command: script_path,
                wasm: None,
                working_dir: None,
                args: Vec::new(),
                autostart: true,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::PluginState { plugin, reason })
                if plugin == "bad-exit" && reason.contains("exited with")
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_rejects_emitted_messages() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "cubex-plugin-stop-message-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&dir).unwrap();
        let start_response_path = dir.join("start-response.bin");
        let stop_response_path = dir.join("stop-response.bin");
        let script_path = dir.join("plugin.sh");

        let mut start_response_file = std::fs::File::create(&start_response_path).unwrap();
        cubex_protocol::write_frame(&mut start_response_file, &PluginResponse::default()).unwrap();
        let mut stop_response_file = std::fs::File::create(&stop_response_path).unwrap();
        cubex_protocol::write_frame(
            &mut stop_response_file,
            &PluginResponse {
                messages: vec![Message::new(
                    "ignored",
                    "late.topic",
                    Payload::Text("late".into()),
                )],
                logs: Vec::new(),
                error: None,
            },
        )
        .unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/stop-response.bin\"\nwait\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            plugins: vec![PluginConfig {
                name: "late-plugin".into(),
                command: script_path,
                wasm: None,
                working_dir: None,
                args: Vec::new(),
                autostart: true,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::InvalidPluginMessage { plugin, reason })
                if plugin == "late-plugin"
                    && reason == "system.stop response must not emit messages"
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn plugin_error_responses_fail_the_run() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("cubex-plugin-error-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let error_response_path = dir.join("error-response.bin");
        let stop_response_path = dir.join("stop-response.bin");
        let script_path = dir.join("plugin.sh");

        let mut error_response_file = std::fs::File::create(&error_response_path).unwrap();
        cubex_protocol::write_frame(
            &mut error_response_file,
            &PluginResponse {
                messages: Vec::new(),
                logs: Vec::new(),
                error: Some("boom".into()),
            },
        )
        .unwrap();
        let mut stop_response_file = std::fs::File::create(&stop_response_path).unwrap();
        cubex_protocol::write_frame(&mut stop_response_file, &PluginResponse::default()).unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/error-response.bin\"\ncat \"$(dirname \"$0\")/stop-response.bin\"\nwait\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            plugins: vec![PluginConfig {
                name: "bad-plugin".into(),
                command: script_path,
                wasm: None,
                working_dir: None,
                args: Vec::new(),
                autostart: true,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        })
        .unwrap();

        assert!(matches!(
            engine.run(),
            Err(Error::PluginError { plugin, reason })
                if plugin == "bad-plugin" && reason == "boom"
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn replay_start_and_derived_messages_are_not_persisted() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("cubex-replay-derived-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let log_path = dir.join("events.bin");
        let start_response_path = dir.join("start-response.bin");
        let response_path = dir.join("response.bin");
        let script_path = dir.join("plugin.sh");

        let log = EventLog::new(&log_path);
        log.append(&Message::new(
            "seed",
            "seed.topic",
            Payload::Text("one".into()),
        ))
        .unwrap();

        let start_response = PluginResponse {
            messages: vec![Message::new(
                "ignored",
                "started.topic",
                Payload::Text("start".into()),
            )],
            logs: Vec::new(),
            error: None,
        };
        let mut start_response_file = std::fs::File::create(&start_response_path).unwrap();
        cubex_protocol::write_frame(&mut start_response_file, &start_response).unwrap();

        let response = PluginResponse {
            messages: vec![Message::new(
                "ignored",
                "derived.topic",
                Payload::Text("two".into()),
            )],
            logs: Vec::new(),
            error: None,
        };
        let mut response_file = std::fs::File::create(&response_path).unwrap();
        cubex_protocol::write_frame(&mut response_file, &response).unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/start-response.bin\"\ncat \"$(dirname \"$0\")/response.bin\"\nwait\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            engine: EngineConfig {
                name: "test".into(),
                max_messages: 8,
            },
            store: StoreConfig {
                path: Some(log_path.clone()),
                replay_on_start: true,
            },
            plugins: vec![
                plugin("seed"),
                PluginConfig {
                    name: "deriver".into(),
                    command: script_path,
                    wasm: None,
                    working_dir: None,
                    args: Vec::new(),
                    autostart: false,
                    capabilities: Vec::new(),
                },
            ],
            routes: vec![RouteConfig {
                name: "seed-to-deriver".into(),
                source: Some("seed".into()),
                topic: Some("seed.topic".into()),
                payload: Some(PayloadKind::Text),
                record: BTreeMap::new(),
                to: vec!["deriver".into()],
            }],
        })
        .unwrap();

        let report = engine.run().unwrap();
        assert_eq!(report.replayed, 1);
        assert_eq!(EventLog::new(&log_path).read_all().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn broken_pipe_from_exited_plugin_is_plugin_exited() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("cubex-plugin-exit-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let script_path = dir.join("plugin.sh");
        let mut script = std::fs::File::create(&script_path).unwrap();
        script.write_all(b"#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let plugin = ProcessPlugin::new(PluginConfig {
            name: "dead-plugin".into(),
            command: script_path,
            wasm: None,
            working_dir: None,
            args: Vec::new(),
            autostart: false,
            capabilities: Vec::new(),
        })
        .unwrap();
        let mut child = plugin.spawn().unwrap();
        let _ = child.child.wait();
        let error = child
            .call(
                "dead-plugin",
                PluginRequest {
                    plugin: "dead-plugin".into(),
                    message: Message::new("test", "topic", Payload::Text("x".into())),
                },
            )
            .unwrap_err();

        assert!(matches!(error, Error::PluginExited { name } if name == "dead-plugin"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn partial_response_from_exited_plugin_is_plugin_exited() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("cubex-plugin-partial-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let script_path = dir.join("plugin.sh");
        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(b"#!/bin/sh\ncat >/dev/null &\nprintf '\\001\\000'\nwait\n")
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let plugin = ProcessPlugin::new(PluginConfig {
            name: "partial-plugin".into(),
            command: script_path,
            wasm: None,
            working_dir: None,
            args: Vec::new(),
            autostart: false,
            capabilities: Vec::new(),
        })
        .unwrap();
        let error = plugin
            .call(PluginRequest {
                plugin: "partial-plugin".into(),
                message: Message::new("test", "topic", Payload::Text("x".into())),
            })
            .unwrap_err();

        assert!(matches!(error, Error::PluginExited { name } if name == "partial-plugin"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn emitted_sources_are_host_assigned() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("cubex-plugin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let response_path = dir.join("response.bin");
        let script_path = dir.join("plugin.sh");

        let response = PluginResponse {
            messages: vec![Message::new(
                "spoofed-source",
                "spoofed.topic",
                Payload::Text("payload".into()),
            )],
            logs: Vec::new(),
            error: None,
        };
        let mut response_file = std::fs::File::create(&response_path).unwrap();
        cubex_protocol::write_frame(&mut response_file, &response).unwrap();

        let mut script = std::fs::File::create(&script_path).unwrap();
        script
            .write_all(
                b"#!/bin/sh\ncat >/dev/null &\ncat \"$(dirname \"$0\")/response.bin\"\nwait\n",
            )
            .unwrap();
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();

        let engine = Engine::from_config(Config {
            engine: EngineConfig {
                name: "test".into(),
                max_messages: 8,
            },
            plugins: vec![PluginConfig {
                name: "actual-plugin".into(),
                command: script_path,
                wasm: None,
                working_dir: None,
                args: Vec::new(),
                autostart: true,
                capabilities: Vec::new(),
            }],
            ..Config::default()
        })
        .unwrap();

        let report = engine.run().unwrap();
        assert_eq!(report.emitted[0].source, "actual-plugin");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn plugin(name: &str) -> PluginConfig {
        PluginConfig {
            name: name.into(),
            command: "unused".into(),
            wasm: None,
            working_dir: None,
            args: Vec::new(),
            autostart: false,
            capabilities: Vec::new(),
        }
    }

    fn write_event(path: &Path, message: &Message) {
        let mut file = std::fs::File::create(path).unwrap();
        cubex_protocol::write_frame(&mut file, message).unwrap();
    }
}
