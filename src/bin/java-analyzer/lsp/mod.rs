//! Experimental definition-only LSP broker. Analysis stays in native subprocesses.
mod backend;
mod protocol;
mod state;

use backend::{Files, Job};
use protocol::{Incoming, Transport};
use serde_json::{Value, json};
use state::Workspace;
use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{RecvTimeoutError, TryRecvError},
    },
    time::{Duration, Instant},
};

const MAX_QUEUED: usize = 8;
const ADMIT_DOCUMENT: &str = "javaAnalyzer/admitDocument";
const ADMISSION_VERSION: u32 = 1;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Id {
    Integer(i32),
    String(String),
}
impl Id {
    fn parse(value: &Value) -> Option<Self> {
        if let Some(value) = value.as_i64().and_then(|n| i32::try_from(n).ok()) {
            return Some(Self::Integer(value));
        }
        value.as_str().filter(|s| s.len() <= 256).map(|s| Self::String(s.to_owned()))
    }
    fn value(&self) -> Value {
        match self {
            Self::Integer(n) => json!(n),
            Self::String(s) => json!(s),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    AwaitInitialized,
    Ready,
    ShuttingDown,
    Shutdown,
}
struct Request {
    id: Id,
    generation: u64,
    input: Value,
}
enum Kind {
    Initialize(Id),
    Definition(Request),
}
impl Kind {
    fn id(&self) -> &Id {
        match self {
            Self::Initialize(id) => id,
            Self::Definition(request) => &request.id,
        }
    }
}
struct Active {
    job: Job,
    kind: Kind,
    terminal: bool,
}
struct Server {
    transport: Transport,
    // Drop the active job before removing its immutable private input directory.
    active: Option<Active>,
    files: Files,
    manifest: PathBuf,
    phase: Phase,
    workspace: Option<Workspace>,
    generation: Arc<AtomicU64>,
    pending: VecDeque<Request>,
    ids: BTreeSet<Id>,
    shutdown: Option<Id>,
    #[cfg(test)]
    barrier: Option<Arc<std::sync::Mutex<backend::Barrier>>>,
}

/// Called before the CLI's one-shot execution routing. No Java analysis runs here.
pub(super) fn run(args: &[String]) -> i32 {
    if args.len() != 2 || args[0] != "--manifest" || args[1].is_empty() || args[1].contains('\0') {
        eprintln!("usage: java-analyzer lsp --manifest FILE");
        return 2;
    }
    let result = (|| {
        let mut server = Server {
            transport: Transport::stdio().map_err(|e| e.to_string())?,
            active: None,
            files: Files::new()?,
            manifest: PathBuf::from(&args[1]),
            phase: Phase::New,
            workspace: None,
            generation: Arc::new(AtomicU64::new(0)),
            pending: VecDeque::new(),
            ids: BTreeSet::new(),
            shutdown: None,
            #[cfg(test)]
            barrier: None,
        };
        server.serve()
    })();
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("java-analyzer LSP stopped: {message}");
            2
        }
    }
}
impl Server {
    fn serve(&mut self) -> Result<i32, String> {
        loop {
            self.transport.check()?;
            match self.transport.incoming.recv_timeout(Duration::from_millis(5)) {
                Ok(event) => {
                    if let Some(code) = self.event(event)? {
                        return Ok(code);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(0),
            }
            // Bound dispatch work per turn so input floods cannot starve cleanup.
            for _ in 0..7 {
                match self.transport.incoming.try_recv() {
                    Ok(event) => {
                        if let Some(code) = self.event(event)? {
                            return Ok(code);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(0),
                }
            }
            self.poll()?;
        }
    }
    fn event(&mut self, event: Incoming) -> Result<Option<i32>, String> {
        match event {
            Incoming::End(Ok(())) => Ok(Some(0)),
            Incoming::End(Err(error)) => Err(error),
            Incoming::Message(Err(_)) => {
                self.transport.send(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON-RPC message"}}),
                )?;
                // We cannot identify which buffer a malformed update concerned.
                // Stop rather than continue with potentially stale source state.
                self.fail_after_flush("invalid JSON-RPC body; restart required")?;
                Ok(Some(2))
            }
            Incoming::Message(Ok(value)) => self.message(value),
        }
    }
    fn fail_after_flush(&mut self, reason: &str) -> Result<(), String> {
        if let Some(active) = &mut self.active {
            active.job.cancel();
        }
        let end = Instant::now() + Duration::from_millis(100);
        while !self.transport.drained() && Instant::now() < end {
            self.transport.check()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        eprintln!("java-analyzer LSP: {reason}");
        Ok(())
    }
    fn message(&mut self, value: Value) -> Result<Option<i32>, String> {
        let id = value.get("id").and_then(Id::parse);
        if value["jsonrpc"] != "2.0" || value.get("id").is_some() && id.is_none() || !value["method"].is_string() {
            self.transport.send(json!({"jsonrpc":"2.0","id":id.as_ref().map(Id::value),"error":{"code":-32600,"message":"Invalid JSON-RPC request"}}))?;
            self.fail_after_flush("invalid JSON-RPC envelope; restart required")?;
            return Ok(Some(2));
        }
        let method = value["method"].as_str().expect("validated method");
        let params = value.get("params").unwrap_or(&Value::Null);
        if let Some(id) = id {
            if ["textDocument/didOpen", "textDocument/didChange", "textDocument/didClose", "textDocument/didSave"]
                .contains(&method)
            {
                self.error(&id, -32600, "Synchronization methods must be notifications")?;
                self.fail_after_flush("invalid synchronization request; restart required")?;
                return Ok(Some(2));
            }
            if self.ids.contains(&id) {
                return Err("duplicate outstanding request ID; correlation is unsafe".into());
            }
            self.request(id, method, params)?;
        } else {
            if method == "exit" {
                return Ok(Some(if self.phase == Phase::Shutdown { 0 } else { 1 }));
            }
            self.notification(method, params)?;
        }
        Ok(None)
    }
    fn request(&mut self, id: Id, method: &str, params: &Value) -> Result<(), String> {
        if method == "initialize" {
            if self.phase != Phase::New {
                return self.error(&id, -32600, "initialize may run only once in an active session");
            }
            if !params.is_object() || !params["capabilities"].is_object() {
                return self.error(&id, -32602, "initialize requires a capabilities object");
            }
            self.active = Some(Active {
                job: Job::inspect(&self.manifest)?,
                kind: Kind::Initialize(id.clone()),
                terminal: false,
            });
            self.ids.insert(id);
            self.phase = Phase::Initializing;
            return Ok(());
        }
        if self.phase == Phase::New || self.phase == Phase::Initializing {
            return self.error(&id, -32002, "Server not initialized");
        }
        if self.phase == Phase::Shutdown || self.phase == Phase::ShuttingDown {
            return self.error(&id, -32600, "Server is shutting down");
        }
        match method {
            "shutdown" => {
                if !params.is_null() {
                    return self.error(&id, -32602, "shutdown takes no parameters");
                }
                self.finish_pending(-32803, "Server is shutting down")?;
                if let Some(mut active) = self.active.take() {
                    if !active.terminal {
                        self.error(active.kind.id(), -32803, "Server is shutting down")?;
                        self.ids.remove(active.kind.id());
                        active.terminal = true;
                    }
                    active.job.cancel();
                    self.active = Some(active);
                }
                self.ids.insert(id.clone());
                self.shutdown = Some(id);
                self.phase = Phase::ShuttingDown;
            }
            ADMIT_DOCUMENT => {
                if self.phase != Phase::Ready {
                    return self.error(&id, -32002, "Send initialized before admitting documents");
                }
                let workspace = self.workspace.as_mut().expect("ready workspace");
                let before = workspace.generation;
                let result = workspace.admit(params);
                // Errors from the shared open transition can desynchronize state.
                // Publish and obsolete exactly as for synchronization notifications.
                self.publish_generation(before)?;
                match result {
                    Ok(admission) => self.result(&id, admission.response())?,
                    Err(message) => self.error(&id, -32602, &message)?,
                }
            }
            "textDocument/definition" => {
                if self.phase != Phase::Ready {
                    return self.error(&id, -32002, "Send initialized before requesting definitions");
                }
                let workspace = self.workspace.as_ref().expect("initialized workspace");
                let input = match workspace.definition(params) {
                    Ok(input) => input,
                    Err(message) => return self.error(&id, -32602, &message),
                };
                if self.pending.len() >= MAX_QUEUED {
                    return self.error(&id, -32803, "Definition queue is full (one active and eight queued)");
                }
                let request = Request { id: id.clone(), generation: workspace.generation, input };
                self.ids.insert(id);
                self.pending.push_back(request);
            }
            _ => self.error(&id, -32601, "Method not supported by this definition-only preview")?,
        }
        Ok(())
    }
    fn notification(&mut self, method: &str, params: &Value) -> Result<(), String> {
        if method == "$/cancelRequest" {
            if let Some(id) = params.get("id").and_then(Id::parse) {
                self.cancel(&id)?;
            }
            return Ok(());
        }
        if method == "initialized" {
            if self.phase == Phase::AwaitInitialized {
                self.phase = Phase::Ready;
            }
            return Ok(());
        }
        if self.phase != Phase::Ready {
            return Ok(());
        }
        if ["textDocument/didOpen", "textDocument/didChange", "textDocument/didClose", "textDocument/didSave"]
            .contains(&method)
        {
            let workspace = self.workspace.as_mut().expect("ready workspace");
            let before = workspace.generation;
            let result = workspace.notify(method, params);
            self.publish_generation(before)?;
            if let Err(message) = result {
                self.log(&message)?;
            }
        }
        // Includes $/setTrace. The Neovim adapter uses this harmless notification
        // to flush all client buffers before its definition request.
        Ok(())
    }
    fn publish_generation(&mut self, before: u64) -> Result<(), String> {
        let generation = self.workspace.as_ref().expect("ready workspace").generation;
        self.generation.store(generation, Ordering::SeqCst);
        if generation != before {
            self.obsolete()?;
        }
        Ok(())
    }
    fn cancel(&mut self, id: &Id) -> Result<(), String> {
        if let Some(position) = self.pending.iter().position(|r| &r.id == id) {
            let request = self.pending.remove(position).expect("found pending request");
            self.error(&request.id, -32800, "Request cancelled")?;
            self.ids.remove(&request.id);
        }
        if self.active.as_ref().is_some_and(|active| active.kind.id() == id && !active.terminal) {
            let mut active = self.active.take().expect("matching active request");
            self.error(id, -32800, "Request cancelled")?;
            self.ids.remove(id);
            active.terminal = true;
            active.job.cancel();
            self.active = Some(active);
        }
        Ok(())
    }
    fn obsolete(&mut self) -> Result<(), String> {
        let mut changed = false;
        while let Some(request) = self.pending.pop_front() {
            self.result(&request.id, Value::Null)?;
            self.ids.remove(&request.id);
            changed = true;
        }
        if self.active.as_ref().is_some_and(|active| matches!(active.kind, Kind::Definition(_)) && !active.terminal) {
            let mut active = self.active.take().expect("matching active request");
            self.result(active.kind.id(), Value::Null)?;
            self.ids.remove(active.kind.id());
            active.terminal = true;
            active.job.cancel();
            self.active = Some(active);
            changed = true;
        }
        if changed {
            self.log("Definition discarded because the workspace generation changed")?;
        }
        Ok(())
    }
    fn poll(&mut self) -> Result<(), String> {
        if let Some(active) = &mut self.active
            && let Some(response) = active.job.poll()?
        {
            let active = self.active.take().expect("completed active request");
            if !active.terminal {
                self.complete(&active.kind, &response)?;
                self.ids.remove(active.kind.id());
            } else if matches!(active.kind, Kind::Initialize(_)) {
                self.phase = Phase::New;
            }
            // Terminal cancellation/obsolescence already released this ID. A
            // client may have reused it while old-process cleanup was pending.
            drop(active);
        }
        if self.active.is_none() && self.phase == Phase::ShuttingDown {
            let id = self.shutdown.take().expect("shutdown request");
            self.result(&id, Value::Null)?;
            self.ids.remove(&id);
            self.phase = Phase::Shutdown;
        }
        if self.active.is_none()
            && self.phase == Phase::Ready
            && let Some(request) = self.pending.pop_front()
        {
            if request.generation != self.generation.load(Ordering::SeqCst) {
                self.result(&request.id, Value::Null)?;
                self.ids.remove(&request.id);
            } else {
                #[cfg(test)]
                let job = match &self.barrier {
                    Some(barrier) => Job::fixture(barrier.clone()),
                    None => Job::definition(&mut self.files, &request.input)?,
                };
                #[cfg(not(test))]
                let job = Job::definition(&mut self.files, &request.input)?;
                self.active = Some(Active { job, kind: Kind::Definition(request), terminal: false });
            }
        }
        Ok(())
    }
    fn complete(&mut self, kind: &Kind, response: &Value) -> Result<(), String> {
        if let Some(error) = response.get("error") {
            if matches!(kind, Kind::Initialize(_)) {
                self.phase = Phase::New;
            }
            let code = if error["code"] == "invalid_position" { -32602 } else { -32803 };
            let message = error["message"].as_str().unwrap_or("Native analysis failed");
            return self.error(kind.id(), code, message);
        }
        let result = &response["result"];
        match kind {
            Kind::Initialize(id) => {
                let workspace = Workspace::from_inspect(result)?;
                self.workspace = Some(workspace);
                self.phase = Phase::AwaitInitialized;
                self.result(id, json!({"serverInfo":{"name":"java-analyzer-rs","version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":1,"save":{"includeText":false}},"definitionProvider":true,
                    "experimental":{"javaAnalyzer":{"definitionOnly":true,"fullTextOnly":true,"frozenConfiguration":true,"semanticDiagnostics":false,"jvm":false,"maxOverlayBytes":state::MAX_OVERLAY_BYTES,"maxOpenDocuments":state::MAX_OVERLAYS,"documentAdmission":{"version":ADMISSION_VERSION,"method":ADMIT_DOCUMENT}}}}}))
            }
            Kind::Definition(request) => {
                if result["generation"].as_u64() != Some(request.generation) {
                    return Err("native result generation does not match its immutable job".into());
                }
                if request.generation != self.generation.load(Ordering::SeqCst) {
                    self.log("Definition discarded because the workspace generation changed")?;
                    return self.result(&request.id, Value::Null);
                }
                if result["status"] == "Resolved" && result["location"].is_object() {
                    let location = &result["location"];
                    let path = location["path"].as_str().ok_or("native location omitted source path")?;
                    let uri = self.workspace.as_ref().expect("ready workspace").target_uri(path)?;
                    let range = &location["range"];
                    for edge in ["start", "end"] {
                        for coordinate in ["line", "character"] {
                            if range[edge][coordinate].as_u64().filter(|n| *n <= i32::MAX as u64).is_none() {
                                return Err("native source range is not representable in LSP".into());
                            }
                        }
                    }
                    self.transport.send_guarded(
                        json!({"jsonrpc":"2.0","id":request.id.value(),"result":{"uri":uri,"range":range}}),
                        request.generation,
                        self.generation.clone(),
                    )
                } else {
                    let reason = result["refusal"]
                        .as_str()
                        .or_else(|| result["status"].as_str())
                        .unwrap_or("unsupported navigation outcome");
                    self.log(&format!("No source navigation: {reason}"))?;
                    self.result(&request.id, Value::Null)
                }
            }
        }
    }
    fn finish_pending(&mut self, code: i32, message: &str) -> Result<(), String> {
        while let Some(request) = self.pending.pop_front() {
            self.error(&request.id, code, message)?;
            self.ids.remove(&request.id);
        }
        Ok(())
    }
    fn result(&self, id: &Id, result: Value) -> Result<(), String> {
        self.transport.send(json!({"jsonrpc":"2.0","id":id.value(),"result":result}))
    }
    fn error(&self, id: &Id, code: i32, message: &str) -> Result<(), String> {
        let message: String = message.chars().take(1024).collect();
        self.transport.send(json!({"jsonrpc":"2.0","id":id.value(),"error":{"code":code,"message":message}}))
    }
    fn log(&self, message: &str) -> Result<(), String> {
        let message: String = message.chars().take(1024).collect();
        self.transport.send(json!({"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":message}}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Cursor, sync::Mutex};
    struct Fixture {
        server: Server,
        root: PathBuf,
        uri: String,
        dependency_uri: String,
        output: Arc<Mutex<Vec<u8>>>,
        barrier: Arc<Mutex<backend::Barrier>>,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "lsp-schedule-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            let root = root.canonicalize().unwrap();
            let file = root.join("A.java");
            fs::write(&file, "class A {}").unwrap();
            let uri = format!("file://{}", file.to_str().unwrap());
            let dependency = root.join("B.java");
            fs::write(&dependency, "class B {}").unwrap();
            let dependency_uri = format!("file://{}", dependency.to_str().unwrap());
            let mut workspace = Workspace::from_inspect(&json!({"frozen":{},"sources":[
                {"file":file.to_str().unwrap(),"compilation":{"project":"p","source_set":"main"}},
                {"file":dependency.to_str().unwrap(),"compilation":{"project":"dep","source_set":"main"}}
            ]}))
            .unwrap();
            workspace
                .notify(
                    "textDocument/didOpen",
                    &json!({"textDocument":{"uri":uri,"languageId":"java","version":1,"text":"class A {}"}}),
                )
                .unwrap();
            workspace.notify("textDocument/didOpen", &json!({"textDocument":{"uri":dependency_uri,"languageId":"java","version":-1,"text":"class B {}"}})).unwrap();
            let (transport, output) = Transport::fixture();
            let barrier = Arc::new(Mutex::new(backend::Barrier::default()));
            let generation = Arc::new(AtomicU64::new(workspace.generation));
            let server = Server {
                transport,
                active: None,
                files: Files::new().unwrap(),
                manifest: file,
                phase: Phase::Ready,
                workspace: Some(workspace),
                generation,
                pending: VecDeque::new(),
                ids: BTreeSet::new(),
                shutdown: None,
                barrier: Some(barrier.clone()),
            };
            Self { server, root, uri, dependency_uri, output, barrier }
        }
        fn request(&mut self, id: i32) {
            self.server.message(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/definition","params":{"textDocument":{"uri":self.uri},"position":{"line":0,"character":6}}})).unwrap();
        }
        fn release(&self) {
            self.barrier.lock().unwrap().result = Some(
                json!({"schema_version":1,"error":{"code":"execution_cancelled","message":"cancelled","worker_reaped":true}}),
            );
        }
        fn messages(&self) -> Vec<Value> {
            let end = Instant::now() + Duration::from_secs(1);
            while !self.server.transport.drained() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(1));
            }
            let mut bytes = Cursor::new(self.output.lock().unwrap().clone());
            let mut messages = Vec::new();
            while let Some(body) = protocol::read_frame(&mut bytes).unwrap() {
                messages.push(serde_json::from_slice(&body).unwrap());
            }
            messages
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn admission(f: &mut Fixture, id: i32, uri: &str, path: &str) {
        f.server.message(json!({"jsonrpc":"2.0","id":id,"method":"javaAnalyzer/admitDocument","params":{
            "expectedCanonicalPath":path,"textDocument":{"uri":uri,"languageId":"java","version":-4,"text":"class B {}"}}})).unwrap();
    }
    #[test]
    fn admission_success_and_mutating_error_obsolete_active_and_queued_definitions() {
        for duplicate in [false, true] {
            let mut f = Fixture::new();
            let uri = f.dependency_uri.clone();
            let path = f.root.join("B.java");
            if !duplicate {
                f.server.notification("textDocument/didClose", &json!({"textDocument":{"uri":uri}})).unwrap();
            }
            f.request(1);
            f.server.poll().unwrap();
            f.request(2);
            let before = f.server.generation.load(Ordering::SeqCst);
            // Cancellation before dispatch does not reserve an ID or undo a later mutation.
            f.server.cancel(&Id::Integer(3)).unwrap();
            admission(&mut f, 3, &uri, path.to_str().unwrap());
            assert!(f.server.generation.load(Ordering::SeqCst) > before);
            assert!(f.barrier.lock().unwrap().cancelled);
            assert!(f.server.pending.is_empty());
            f.server.cancel(&Id::Integer(3)).unwrap();
            f.release();
            f.server.poll().unwrap();
            let messages = f.messages();
            for id in [1, 2, 3] {
                let responses: Vec<_> = messages.iter().filter(|m| m["id"] == id).collect();
                assert_eq!(responses.len(), 1, "one terminal response per request");
                if id != 3 {
                    assert_eq!(responses[0]["result"], Value::Null);
                    assert!(responses[0].get("error").is_none());
                } else if duplicate {
                    assert_eq!(responses[0]["error"]["code"], -32602);
                } else {
                    assert_eq!(responses[0]["result"]["status"], "admitted");
                }
            }
            assert_eq!(f.barrier.lock().unwrap().maximum_active, 1);
        }
    }
    #[test]
    fn admission_not_selected_or_identity_error_does_not_obsolete_definitions() {
        let mut f = Fixture::new();
        f.request(1);
        f.server.poll().unwrap();
        f.request(2);
        let before = f.server.generation.load(Ordering::SeqCst);
        let outside = f.root.join("Outside.java");
        fs::write(&outside, "class Outside {}").unwrap();
        let uri = format!("file://{}", outside.to_str().unwrap());
        admission(&mut f, 3, &uri, outside.to_str().unwrap());
        let mismatch = f.root.join("A.java");
        admission(&mut f, 4, &uri, mismatch.to_str().unwrap());
        assert_eq!(f.server.generation.load(Ordering::SeqCst), before);
        assert!(!f.barrier.lock().unwrap().cancelled);
        assert_eq!(f.server.pending.len(), 1);
        let messages = f.messages();
        assert_eq!(messages.iter().find(|m| m["id"] == 3).unwrap()["result"]["status"], "not-selected");
        assert_eq!(messages.iter().find(|m| m["id"] == 4).unwrap()["error"]["code"], -32602);
        f.server.cancel(&Id::Integer(1)).unwrap();
        f.server.cancel(&Id::Integer(2)).unwrap();
        f.release();
        f.server.poll().unwrap();
    }
    #[test]
    fn cancelled_id_reuse_remains_reserved_for_new_request_after_old_cleanup() {
        let mut f = Fixture::new();
        f.request(1);
        f.server.poll().unwrap();
        f.server.cancel(&Id::Integer(1)).unwrap();
        f.request(1);
        assert!(f.server.ids.contains(&Id::Integer(1)));
        f.release();
        f.server.poll().unwrap();
        assert!(f.server.ids.contains(&Id::Integer(1)), "old terminal job must not erase the new ID incarnation");
        f.server.cancel(&Id::Integer(1)).unwrap();
        f.release();
        f.server.poll().unwrap();
        let responses: Vec<_> = f.messages().into_iter().filter(|m| m["id"] == 1).collect();
        assert_eq!(responses.len(), 2);
        assert!(responses.iter().all(|m| m["error"]["code"] == -32800));
        assert_eq!(f.barrier.lock().unwrap().maximum_active, 1);
    }
    #[test]
    fn malformed_sync_envelope_cannot_continue_using_old_text() {
        let mut f = Fixture::new();
        let outcome = f.server.message(json!({"jsonrpc":"1.0","method":"textDocument/didChange","params":{"textDocument":{"uri":f.uri,"version":2},"contentChanges":[{"text":"new"}]}})).unwrap();
        assert_eq!(outcome, Some(2), "unidentifiable/invalid sync envelopes must stop the session");
    }
    #[test]
    fn queue_and_cancellation_storm_never_start_replacements_before_reaping() {
        let mut f = Fixture::new();
        f.request(1);
        f.server.poll().unwrap();
        for id in 2..=10 {
            f.request(id);
        }
        assert_eq!(f.server.pending.len(), 8);
        assert!(!f.server.ids.contains(&Id::Integer(10)));
        for id in 1..=9 {
            f.server.cancel(&Id::Integer(id)).unwrap();
        }
        for _ in 0..20 {
            f.server.poll().unwrap();
        }
        assert_eq!(f.barrier.lock().unwrap().starts, 1);
        assert!(f.server.pending.is_empty());
        f.release();
        f.server.poll().unwrap();
        assert!(f.server.active.is_none());
        assert_eq!(f.barrier.lock().unwrap().maximum_active, 1);
        let responses: Vec<_> = f.messages().into_iter().filter(|m| m.get("id").is_some()).collect();
        assert_eq!(responses.len(), 10);
        assert_eq!(responses.iter().filter(|m| m["error"]["code"] == -32800).count(), 9);
    }
    #[test]
    fn changed_dependency_generation_cancels_job_and_returns_null_once() {
        let mut f = Fixture::new();
        f.request(1);
        f.server.poll().unwrap();
        f.server
            .notification(
                "textDocument/didChange",
                &json!({"textDocument":{"uri":f.dependency_uri,"version":2},"contentChanges":[{"text":"class B { int x; }"}]}),
            )
            .unwrap();
        assert!(f.barrier.lock().unwrap().cancelled);
        f.release();
        f.server.poll().unwrap();
        let responses: Vec<_> = f.messages().into_iter().filter(|m| m["id"] == 1).collect();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["result"], Value::Null);
        assert!(responses[0].get("error").is_none());
    }
    #[test]
    fn dependency_close_reopen_invalidates_origin_without_origin_edit() {
        let mut f = Fixture::new();
        f.request(1);
        f.server.poll().unwrap();
        f.server.notification("textDocument/didClose", &json!({"textDocument":{"uri":f.dependency_uri}})).unwrap();
        f.server.notification("textDocument/didOpen", &json!({"textDocument":{"uri":f.dependency_uri,"languageId":"java","version":-20,"text":"class B { int newField; }"}})).unwrap();
        assert!(f.barrier.lock().unwrap().cancelled);
        f.release();
        f.server.poll().unwrap();
        let responses: Vec<_> = f.messages().into_iter().filter(|m| m["id"] == 1).collect();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["result"], Value::Null);
    }
    #[test]
    fn uncertain_backend_cleanup_propagates_fatal_error_without_starting_queued_job() {
        let mut f = Fixture::new();
        f.request(1);
        f.server.poll().unwrap();
        f.request(2);
        let barrier = f.barrier.clone();
        barrier.lock().unwrap().error = Some("native worker cleanup unconfirmed; restart required".into());
        assert!(f.server.poll().unwrap_err().contains("cleanup unconfirmed"));
        assert_eq!(barrier.lock().unwrap().starts, 1);
        assert_eq!(f.server.pending.len(), 1);
        // serve() propagates poll errors to run(), which drops this session.
        drop(f);
        assert_eq!(barrier.lock().unwrap().active, 0);
        assert_eq!(barrier.lock().unwrap().starts, 1);
    }
}
