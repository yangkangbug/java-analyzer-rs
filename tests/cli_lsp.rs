#![cfg(feature = "cli")]

use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

struct Session {
    child: Child,
    input: Option<ChildStdin>,
    messages: Receiver<Value>,
}
impl Session {
    fn start() -> Self {
        Self::start_manifest(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/examples/cli_project/project.json"
        )))
    }
    fn start_manifest(manifest: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_java-analyzer"))
            .args(["lsp", "--manifest"])
            .arg(manifest)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (send, messages) = mpsc::sync_channel(16);
        thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut length = None;
                loop {
                    let mut line = String::new();
                    if output.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.strip_prefix("Content-Length: ") {
                        length = value.trim().parse::<usize>().ok();
                    }
                }
                let Some(length) = length.filter(|n| *n <= 16 * 1024 * 1024) else { return };
                let mut bytes = vec![0; length];
                if output.read_exact(&mut bytes).is_err() {
                    return;
                }
                let Ok(value) = serde_json::from_slice(&bytes) else { return };
                if send.send(value).is_err() {
                    return;
                }
            }
        });
        Self { child, input: Some(input), messages }
    }
    fn send(&mut self, value: Value) {
        let body = serde_json::to_vec(&value).unwrap();
        let input = self.input.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
        input.flush().unwrap();
    }
    fn initialize(&mut self) {
        self.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}));
        assert!(self.response(1).get("result").is_some());
        self.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    }
    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(std::time::Instant::now() < deadline, "server failed to exit within bounded deadline");
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn response(&self, id: i32) -> Value {
        loop {
            let value = self.messages.recv_timeout(Duration::from_secs(10)).expect("bounded LSP response");
            if value.get("id") == Some(&json!(id)) {
                return value;
            }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Keep these fixtures entirely native: the class bytes describe only Object's
// signature, as in tests/cli.rs, and the broker/worker inherit an empty environment.
struct ReferenceCallFixture {
    root: std::path::PathBuf,
}
impl ReferenceCallFixture {
    const BASE: &str = "package p;\npublic class Base {}\n";
    const DERIVED: &str = "package p;\npublic class Derived extends Base {}\n";
    const RECEIVER: &str = "package p;\npublic class Receiver {\n    public void m(Base value) {}\n    public void m(Derived value) {}\n    public void only(Derived value) {}\n}\n";
    const USE: &str = "package p;\npublic class Use {\n    public void run(Receiver receiver, Derived value) {\n        receiver.m(value);\n    }\n}\n";

    fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let root = std::env::temp_dir().join(format!(
            "lsp-reference-calls-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(root.join("src/p")).unwrap();
        std::fs::create_dir_all(root.join("platform/java/lang")).unwrap();
        let fixture = Self { root: root.canonicalize().unwrap() };
        for (name, text) in
            [("Base", Self::BASE), ("Derived", Self::DERIVED), ("Receiver", Self::RECEIVER), ("Use", Self::USE)]
        {
            std::fs::write(fixture.source_path(name), text).unwrap();
        }
        let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
        object.extend_from_slice(b"java/lang/Object");
        object.extend_from_slice(&[7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        std::fs::write(fixture.root.join("platform/java/lang/Object.class"), object).unwrap();
        std::fs::write(
            fixture.root.join("project.json"),
            serde_json::to_vec(&json!({"schema_version":1,"compilations":[{
                "id":{"project":"p","source_set":"main"},"source_roots":["src"],
                "classpath":[{"kind":"directory","path":"platform"}]
            }]}))
            .unwrap(),
        )
        .unwrap();
        fixture
    }
    fn source_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join(format!("src/p/{name}.java"))
    }
    fn uri(&self, name: &str) -> String {
        format!("file://{}", self.source_path(name).to_str().unwrap())
    }
    fn session(&self) -> Session {
        let mut session = Session::start_manifest(&self.root.join("project.json"));
        session.initialize();
        session
    }
    fn assert_sources_unchanged(&self) {
        for (name, text) in
            [("Base", Self::BASE), ("Derived", Self::DERIVED), ("Receiver", Self::RECEIVER), ("Use", Self::USE)]
        {
            assert_eq!(std::fs::read_to_string(self.source_path(name)).unwrap(), text);
        }
    }
}
impl Drop for ReferenceCallFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn source_position(text: &str, marker: &str) -> Value {
    let prefix = &text[..text.find(marker).expect("unique source marker")];
    json!({"line":prefix.bytes().filter(|b| *b == b'\n').count(),
        "character":prefix.rsplit('\n').next().unwrap().encode_utf16().count()})
}

fn method_location(uri: &str, text: &str, declaration: &str, name: &str) -> Value {
    assert!(declaration.starts_with(name));
    let start = source_position(text, declaration);
    let mut end = start.clone();
    end["character"] = json!(start["character"].as_u64().unwrap() + name.encode_utf16().count() as u64);
    json!({"uri":uri,"range":{"start":start,"end":end}})
}

fn open_source(session: &mut Session, uri: &str, text: &str) {
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
        "textDocument":{"uri":uri,"languageId":"java","version":1,"text":text}}}));
}

fn replace_source(session: &mut Session, uri: &str, version: i32, text: &str) {
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}}));
}

fn source_definition(session: &mut Session, id: i32, uri: &str, text: &str, marker: &str) -> Value {
    session.send(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/definition",
        "params":{"textDocument":{"uri":uri},"position":source_position(text, marker)}}));
    session.response(id)
}

#[test]
fn native_reference_calls_follow_unsaved_argument_and_dependency_replacements() {
    let fixture = ReferenceCallFixture::new();
    let receiver_uri = fixture.uri("Receiver");
    let use_uri = fixture.uri("Use");
    let base_uri = fixture.uri("Base");
    let mut session = fixture.session();
    open_source(&mut session, &receiver_uri, ReferenceCallFixture::RECEIVER);
    open_source(&mut session, &use_uri, ReferenceCallFixture::USE);
    open_source(&mut session, &base_uri, ReferenceCallFixture::BASE);

    let initial = source_definition(&mut session, 10, &use_uri, ReferenceCallFixture::USE, "m(value)");
    assert_eq!(
        initial.get("result"),
        Some(&method_location(&receiver_uri, ReferenceCallFixture::RECEIVER, "m(Derived value)", "m")),
        "a public qualified reference overload must have one proved target: {initial}"
    );

    // Both files differ from disk. The consumer's explicit argument type changes
    // the selected overload; the target's independent edit moves its name range.
    // Astral comments precede the query and target on the same CRLF-delimited line.
    let dirty_receiver = format!(
        "// 😀 unsaved dependency\r\n{}",
        ReferenceCallFixture::RECEIVER.replace('\n', "\r\n").replace("public void m(", "/* 😀 */ public void m(")
    );
    let dirty_use = format!(
        "// 😀 unsaved consumer\r\n{}",
        ReferenceCallFixture::USE
            .replace('\n', "\r\n")
            .replace("Derived value", "Base value")
            .replace("receiver.m(value)", "/* 😀 */ receiver.m(value)")
    );
    replace_source(&mut session, &receiver_uri, 2, &dirty_receiver);
    replace_source(&mut session, &use_uri, 2, &dirty_use);
    let base_location = method_location(&receiver_uri, &dirty_receiver, "m(Base value)", "m");
    let switched = source_definition(&mut session, 11, &use_uri, &dirty_use, "m(value)");
    assert_eq!(switched.get("result"), Some(&base_location), "{switched}");
    assert_ne!(switched["result"], initial["result"]);

    // A separate hierarchy dependency loses a required ancestor. The old exact
    // answer is no longer justified, even though the call text itself is unchanged.
    let missing_ancestor = "package p;\r\npublic class Base extends Missing {}\r\n";
    replace_source(&mut session, &base_uri, 2, missing_ancestor);
    let refused = source_definition(&mut session, 12, &use_uri, &dirty_use, "m(value)");
    assert_eq!(refused.get("result"), Some(&Value::Null), "missing ancestor must not navigate: {refused}");
    replace_source(&mut session, &base_uri, 3, ReferenceCallFixture::BASE);
    let repaired = source_definition(&mut session, 13, &use_uri, &dirty_use, "m(value)");
    assert_eq!(repaired.get("result"), Some(&base_location), "{repaired}");

    // Equal and old dependency versions poison the whole workspace rather than
    // allowing either the disk target or the previously proved overlay target.
    for (id, version) in [(14, 2), (15, 1)] {
        replace_source(&mut session, &receiver_uri, version, ReferenceCallFixture::RECEIVER);
        let refused = source_definition(&mut session, id, &use_uri, &dirty_use, "m(value)");
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
        assert!(refused["error"]["message"].as_str().unwrap().contains("desynchronized"));
        assert!(refused.get("result").is_none(), "a rejected version must never expose a stale Location");
    }
    let repaired_receiver = format!("// newer repair\r\n{dirty_receiver}");
    let repaired_use = dirty_use.replace("Base value", "Derived value");
    replace_source(&mut session, &receiver_uri, 3, &repaired_receiver);
    replace_source(&mut session, &use_uri, 3, &repaired_use);
    let derived_location = method_location(&receiver_uri, &repaired_receiver, "m(Derived value)", "m");
    let repaired = source_definition(&mut session, 16, &use_uri, &repaired_use, "m(value)");
    assert_eq!(repaired.get("result"), Some(&derived_location), "{repaired}");
    assert_ne!(repaired["result"], initial["result"]);
    assert_ne!(repaired["result"], switched["result"]);

    // The consumer's stale versions are equally unsafe. A later full-text
    // replacement recovers without reverting either unsaved dependency buffer.
    for (id, version) in [(17, 3), (18, 2)] {
        replace_source(&mut session, &use_uri, version, ReferenceCallFixture::USE);
        let refused = source_definition(&mut session, id, &use_uri, &repaired_use, "m(value)");
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
        assert!(refused["error"]["message"].as_str().unwrap().contains("desynchronized"));
        assert!(refused.get("result").is_none());
    }
    replace_source(&mut session, &use_uri, 4, &repaired_use);
    let recovered = source_definition(&mut session, 19, &use_uri, &repaired_use, "m(value)");
    assert_eq!(recovered.get("result"), Some(&derived_location), "{recovered}");

    session.send(json!({"jsonrpc":"2.0","id":20,"method":"shutdown","params":null}));
    assert_eq!(session.response(20)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
    fixture.assert_sources_unchanged();
}

#[test]
fn native_strict_reference_calls_exclude_scalar_competitor_without_stale_targets() {
    let fixture = ReferenceCallFixture::new();
    let mut session = fixture.session();
    let use_uri = fixture.uri("Use");
    let receiver_uri = fixture.uri("Receiver");
    // Derived -> Base is a proved strict reference conversion. The int overload
    // is inapplicable without unboxing, which this native resolver does not infer.
    let receiver =
        "package p;\r\npublic class Receiver { public void m(int value) {} public void m(Base value) {} }\r\n";
    let consumer = ReferenceCallFixture::USE.replace("receiver.m(value)", "/* 😀 */ receiver.m(value)");
    open_source(&mut session, &receiver_uri, receiver);
    open_source(&mut session, &use_uri, &consumer);
    let initial_location = method_location(&receiver_uri, receiver, "m(Base value)", "m");
    let initial = source_definition(&mut session, 70, &use_uri, &consumer, "m(value)");
    assert_eq!(initial.get("result"), Some(&initial_location), "{initial}");

    let dirty_receiver =
        format!("// unsaved target 🐬\r\n{}", receiver.replace("public void m(Base", "/* 😀 */ public void m(Base"));
    replace_source(&mut session, &receiver_uri, 2, &dirty_receiver);
    let location = method_location(&receiver_uri, &dirty_receiver, "m(Base value)", "m");
    let moved = source_definition(&mut session, 71, &use_uri, &consumer, "m(value)");
    assert_eq!(moved.get("result"), Some(&location), "{moved}");
    assert_ne!(location, initial_location);

    // Removing the only strict candidate must not retain either its overlay or
    // disk Location, nor choose an unsupported primitive conversion instead.
    let scalar_only = dirty_receiver.replace("m(Base value)", "m(long value)");
    replace_source(&mut session, &receiver_uri, 3, &scalar_only);
    let refused = source_definition(&mut session, 72, &use_uri, &consumer, "m(value)");
    assert_eq!(refused.get("result"), Some(&Value::Null), "{refused}");
    replace_source(&mut session, &receiver_uri, 4, &dirty_receiver);
    let repaired = source_definition(&mut session, 73, &use_uri, &consumer, "m(value)");
    assert_eq!(repaired.get("result"), Some(&location), "{repaired}");

    // Inspect the full overload set: a newer more-specific reference candidate
    // must replace the old answer even with the scalar competitor still present.
    let expanded = dirty_receiver.replace("m(int value)", "m(Derived value) {} public void m(int value)");
    replace_source(&mut session, &receiver_uri, 5, &expanded);
    let specific = method_location(&receiver_uri, &expanded, "m(Derived value)", "m");
    let switched = source_definition(&mut session, 74, &use_uri, &consumer, "m(value)");
    assert_eq!(switched.get("result"), Some(&specific), "{switched}");
    assert_ne!(specific, location);
    for (id, version) in [(75, 5), (76, 4)] {
        replace_source(&mut session, &receiver_uri, version, &dirty_receiver);
        let stale = source_definition(&mut session, id, &use_uri, &consumer, "m(value)");
        assert_eq!(stale["error"]["code"], -32602, "{stale}");
        assert!(stale["error"]["message"].as_str().unwrap().contains("desynchronized"));
        assert!(stale.get("result").is_none());
    }
    replace_source(&mut session, &receiver_uri, 6, &dirty_receiver);
    let repaired = source_definition(&mut session, 77, &use_uri, &consumer, "m(value)");
    assert_eq!(repaired.get("result"), Some(&location), "{repaired}");
    session.send(json!({"jsonrpc":"2.0","id":78,"method":"shutdown","params":null}));
    assert_eq!(session.response(78)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
    fixture.assert_sources_unchanged();
}

#[test]
fn native_reference_calls_refuse_unique_wrong_arity_and_type_qualified_instance() {
    let fixture = ReferenceCallFixture::new();
    let use_uri = fixture.uri("Use");
    let text = "package p;\npublic class Use {\n    public void run(Receiver receiver, Derived value) {\n        receiver.only();\n        Receiver.only(value);\n    }\n}\n";
    let mut session = fixture.session();
    open_source(&mut session, &use_uri, text);
    // These are deliberately compiler-invalid, despite the unique visible name.
    // Read both independent responses before asserting, retaining both failures.
    let wrong_arity = source_definition(&mut session, 10, &use_uri, text, "only()");
    let instance_on_type = source_definition(&mut session, 11, &use_uri, text, "only(value)");
    assert_eq!(
        [wrong_arity.get("result"), instance_on_type.get("result")],
        [Some(&Value::Null), Some(&Value::Null)],
        "unsupported calls must not become source Locations: {wrong_arity}; {instance_on_type}"
    );
    session.send(json!({"jsonrpc":"2.0","id":12,"method":"shutdown","params":null}));
    assert_eq!(session.response(12)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
    fixture.assert_sources_unchanged();
}

#[test]
fn initialize_and_shutdown_real_native_server() {
    let mut session = Session::start();
    session.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "processId":null,"rootUri":null,"capabilities":{},"unknownFutureField":true
    }}));
    let reply = session.response(1);
    assert_eq!(reply["result"]["capabilities"]["positionEncoding"], "utf-16");
    assert_eq!(reply["result"]["capabilities"]["textDocumentSync"]["change"], 1);
    assert_eq!(reply["result"]["capabilities"]["definitionProvider"], true);
    assert!(reply["result"]["capabilities"].get("completionProvider").is_none());
    session.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}));
    session.send(json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}));
    assert_eq!(session.response(2)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
}

#[test]
fn native_session_eof_and_exit_are_bounded() {
    let mut session = Session::start();
    session.initialize();
    session.input.take();
    assert!(session.wait_exit().success());
    let mut early = Session::start();
    early.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert_eq!(early.wait_exit().code(), Some(1));
}

#[test]
fn native_lifecycle_and_unknown_method_errors() {
    let mut session = Session::start();
    session.send(json!({"jsonrpc":"2.0","id":42,"method":"textDocument/definition","params":{}}));
    assert_eq!(session.response(42)["error"]["code"], -32002);
    session.initialize();
    session.send(json!({"jsonrpc":"2.0","method":"$/setTrace","params":{"value":"off"}}));
    session.send(json!({"jsonrpc":"2.0","id":43,"method":"textDocument/rename","params":{}}));
    assert_eq!(session.response(43)["error"]["code"], -32601);
    session.send(json!({"jsonrpc":"2.0","id":44,"method":"initialize","params":{"capabilities":{}}}));
    assert_eq!(session.response(44)["error"]["code"], -32600);
    session.input.take();
    assert!(session.wait_exit().success());
}

#[test]
fn malformed_sync_and_unrecoverable_framing_stop_native_session() {
    let mut session = Session::start();
    session.initialize();
    session.send(json!({"jsonrpc":"1.0","method":"textDocument/didChange","params":{}}));
    assert_eq!(session.wait_exit().code(), Some(2));
    let mut session = Session::start();
    session.input.as_mut().unwrap().write_all(b"Content-Length: 2097153\r\n\r\n").unwrap();
    assert_eq!(session.wait_exit().code(), Some(2));
    let mut session = Session::start();
    session.input.as_mut().unwrap().write_all(b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!(session.wait_exit().code(), Some(2));
}

#[test]
fn native_cancel_then_reuse_request_id_has_one_terminal_reply_per_request() {
    let mut session = Session::start();
    session.initialize();
    let path =
        std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/cli_project/app/main/app/App.java"))
            .unwrap();
    let uri = format!("file://{}", path.to_str().unwrap());
    let text = std::fs::read_to_string(path).unwrap();
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"java","version":-3,"text":text}}}));
    for _ in 0..8 {
        session.send(json!({"jsonrpc":"2.0","id":7,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":4,"character":8}}}));
        session.send(json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":7}}));
        let result = session.response(7);
        assert!(result["error"]["code"] == -32800 || result.get("result").is_some(), "{result}");
    }
    session.send(json!({"jsonrpc":"2.0","id":8,"method":"shutdown"}));
    assert_eq!(session.response(8)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
}

#[test]
fn native_local_type_containment_survives_unsaved_replacements_and_stale_versions() {
    fn position(text: &str, marker: &str) -> Value {
        let prefix = &text[..text.find(marker).expect("unique source marker")];
        json!({"line":prefix.bytes().filter(|b| *b == b'\n').count(),
            "character":prefix.rsplit('\n').next().unwrap().encode_utf16().count()})
    }
    fn definition(session: &mut Session, id: i32, uri: &str, text: &str, marker: &str) -> Value {
        session.send(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/definition",
            "params":{"textDocument":{"uri":uri},"position":position(text, marker)}}));
        session.response(id)
    }
    fn expected(uri: &str, text: &str, name: &str) -> Value {
        let start = position(text, &format!("{name} ="));
        let mut end = start.clone();
        end["character"] = json!(start["character"].as_u64().unwrap() + name.encode_utf16().count() as u64);
        json!({"uri":uri,"range":{"start":start,"end":end}})
    }
    let path =
        std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/cli_project/app/main/app/App.java"))
            .unwrap();
    let disk_text = std::fs::read_to_string(&path).unwrap();
    let uri = format!("file://{}", path.to_str().unwrap());
    let original = "package app;\nclass App { void run() {\n    class Local { int hidden; void nested() { hidden++; } }\n    int trueLocal = 1;\n    trueLocal++;\n} }\n";
    let updated = format!(
        "// 😀 unsaved replacement\r\n{}",
        original
            .replace("\n", "\r\n")
            .replace("int trueLocal", "/* 😀 */ int renamedLocal")
            .replace("trueLocal", "renamedLocal")
    );
    let mut session = Session::start(); // env_clear: the broker and workers need no Java runtime.
    session.initialize();
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
        "textDocument":{"uri":uri,"languageId":"java","version":1,"text":original}}}));
    assert_eq!(
        definition(&mut session, 10, &uri, original, "trueLocal++")["result"],
        expected(&uri, original, "trueLocal")
    );
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":uri,"version":3},"contentChanges":[{"text":updated}]}}));
    assert_eq!(
        definition(&mut session, 11, &uri, &updated, "renamedLocal++")["result"],
        expected(&uri, &updated, "renamedLocal")
    );
    let refused = definition(&mut session, 12, &uri, &updated, "hidden++");
    assert_eq!(refused.get("result"), Some(&Value::Null), "{refused}");

    // Nonincreasing versions poison synchronization rather than silently making
    // either the old or updated buffer queryable. A newer full replacement repairs it.
    for (id, version) in [(13, 3), (14, 2)] {
        session.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
            "textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":original}]}}));
        let refused = definition(&mut session, id, &uri, &updated, "renamedLocal++");
        assert_eq!(refused["error"]["code"], -32602, "{refused}");
        assert!(refused["error"]["message"].as_str().unwrap().contains("desynchronized"));
    }
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{
        "textDocument":{"uri":uri,"version":4},"contentChanges":[{"text":updated}]}}));
    assert_eq!(
        definition(&mut session, 15, &uri, &updated, "renamedLocal++")["result"],
        expected(&uri, &updated, "renamedLocal")
    );
    session.send(json!({"jsonrpc":"2.0","id":16,"method":"shutdown","params":null}));
    assert_eq!(session.response(16)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), disk_text);
}

#[test]
fn nonreading_client_cannot_hold_server_alive_with_unbounded_output() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_java-analyzer"))
        .args(["lsp", "--manifest"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/cli_project/project.json"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    // Deliberately retain but never read stdout. Even pre-initialize errors are
    // terminal responses and therefore exercise the real bounded writer path.
    let _output = child.stdout.take().unwrap();
    let writer = thread::spawn(move || {
        for id in 0..20_000 {
            let body = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"unknown"})).unwrap();
            if write!(input, "Content-Length: {}\r\n\r\n", body.len()).and_then(|_| input.write_all(&body)).is_err() {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert_eq!(status.code(), Some(2));
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("nonreading client blocked server shutdown");
        }
        thread::sleep(Duration::from_millis(5));
    }
    writer.join().unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn native_cancel_cycles_and_eof_leave_no_running_observed_children_or_reader_growth() {
    use std::{
        collections::BTreeSet,
        fs,
        path::Path,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };
    fn children(pid: u32) -> Vec<u32> {
        // Some Linux/container procfs implementations omit task/*/children.
        // Read the ordinary PPid stat field instead; do not turn a missing
        // observation interface into a false "no children" success.
        fs::read_dir("/proc")
            .expect("Linux process inventory")
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let child = entry.file_name().to_str()?.parse::<u32>().ok()?;
                let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
                let (_, rest) = stat.rsplit_once(") ")?;
                let parent = rest.split_whitespace().nth(1)?.parse::<u32>().ok()?;
                (parent == pid).then_some(child)
            })
            .collect()
    }
    fn identity(pid: u32) -> Option<(char, String)> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, rest) = stat.rsplit_once(") ")?;
        let fields: Vec<_> = rest.split_whitespace().collect();
        Some((fields.first()?.chars().next()?, fields.get(19)?.to_string()))
    }
    fn observe_chain(pid: u32, observed: &mut BTreeSet<(u32, String)>) -> bool {
        let direct = children(pid);
        let mut grandchild = false;
        for child in direct {
            if let Some((_, start)) = identity(child) {
                observed.insert((child, start));
            }
            for worker in children(child) {
                if let Some((_, start)) = identity(worker) {
                    observed.insert((worker, start));
                    grandchild = true;
                }
            }
        }
        grandchild
    }
    fn no_running(observed: &BTreeSet<(u32, String)>) -> bool {
        observed
            .iter()
            .all(|(pid, start)| identity(*pid).is_none_or(|(state, current)| current != *start || state == 'Z'))
    }
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let root = std::env::temp_dir().join(format!(
        "lsp-native-process-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let _cleanup = Cleanup(root.clone());
    let sources = root.join("src");
    fs::create_dir(&sources).unwrap();
    let query = sources.join("A.java");
    let text = "class A { int value; void run() { value++; } }";
    fs::write(&query, text).unwrap();
    // Native workload, not an injected sleeper: enough ordinary source input to
    // observe the real supervisor/worker chain before requesting cancellation.
    let fields: String = (0..1200).map(|n| format!("int field{n};\n")).collect();
    for n in 0..80 {
        fs::write(sources.join(format!("C{n}.java")), format!("class C{n} {{\n{fields}}}\n")).unwrap();
    }
    let manifest = root.join("project.json");
    fs::write(&manifest, serde_json::to_vec(&json!({"schema_version":1,"compilations":[{"id":{"project":"p","source_set":"main"},"source_roots":["src"]}]})).unwrap()).unwrap();
    let mut session = Session::start_manifest(Path::new(&manifest));
    session.initialize();
    let uri = format!("file://{}", query.to_str().unwrap());
    session.send(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"java","version":1,"text":text}}}));
    let pid = session.child.id();
    let mut observed = BTreeSet::new();
    thread::sleep(Duration::from_millis(20));
    let baseline = fs::read_dir(format!("/proc/{pid}/task")).unwrap().count();
    for id in 10..16 {
        session.send(json!({"jsonrpc":"2.0","id":id,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":0,"character":34}}}));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !observe_chain(pid, &mut observed) {
            assert!(
                Instant::now() < deadline,
                "did not observe actual native worker at request {id}; children={:?}, observed={:?}, response={:?}",
                children(pid),
                observed,
                session.messages.try_recv()
            );
            thread::sleep(Duration::from_millis(1));
        }
        session.send(json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":id}}));
        let response = session.response(id);
        assert!(response["error"]["code"] == -32800 || response.get("result").is_some());
        let deadline = Instant::now() + Duration::from_secs(3);
        while !children(pid).is_empty() || !no_running(&observed) {
            assert!(Instant::now() < deadline, "native cancellation left a running observed descendant");
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Duration::from_millis(10));
        assert!(
            fs::read_dir(format!("/proc/{pid}/task")).unwrap().count() <= baseline,
            "native capture reader threads accumulated"
        );
    }
    session.send(json!({"jsonrpc":"2.0","id":30,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":0,"character":34}}}));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !observe_chain(pid, &mut observed) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    session.input.take();
    assert!(session.wait_exit().success());
    let deadline = Instant::now() + Duration::from_secs(3);
    while !no_running(&observed) {
        assert!(Instant::now() < deadline, "EOF left a running observed descendant");
        thread::sleep(Duration::from_millis(5));
    }
    assert!(observed.len() >= 14, "expected seven independently observed native supervisor/worker pairs");
}

#[test]
fn native_construction_arguments_follow_unsaved_constructor_access_and_abstractness() {
    let fixture = ReferenceCallFixture::new();
    let mut session = fixture.session();
    let use_uri = fixture.uri("Use");
    let derived_uri = fixture.uri("Derived");
    let receiver_uri = fixture.uri("Receiver");
    let consumer =
        "package p;\r\npublic class Use { void run(Receiver receiver) { /* 😀 */ receiver.m(new Derived()); } }\r\n";
    let constructor = "package p;\r\npublic class Derived extends Base { /* 😀 */ public Derived() {} }\r\n";
    let receiver = format!(
        "// 😀 dirty target\r\n{}",
        ReferenceCallFixture::RECEIVER.replace('\n', "\r\n").replace("public void m(", "/* 😀 */ public void m(")
    );
    open_source(&mut session, &use_uri, consumer);
    open_source(&mut session, &derived_uri, constructor);
    open_source(&mut session, &receiver_uri, &receiver);
    let location = method_location(&receiver_uri, &receiver, "m(Derived value)", "m");
    let initial = source_definition(&mut session, 30, &use_uri, consumer, "m(new Derived())");
    assert_eq!(initial.get("result"), Some(&location), "{initial}");

    for (version, changed) in [
        (2, constructor.replace("public Derived()", "private Derived()")),
        (4, constructor.replace("public class Derived", "public abstract class Derived")),
    ] {
        replace_source(&mut session, &derived_uri, version, &changed);
        let refused = source_definition(&mut session, 30 + version, &use_uri, consumer, "m(new Derived())");
        assert_eq!(refused.get("result"), Some(&Value::Null), "{refused}");
        replace_source(&mut session, &derived_uri, version + 1, constructor);
        let repaired = source_definition(&mut session, 31 + version, &use_uri, consumer, "m(new Derived())");
        assert_eq!(repaired.get("result"), Some(&location), "{repaired}");
    }
    replace_source(&mut session, &derived_uri, 5, constructor);
    let stale = source_definition(&mut session, 40, &use_uri, consumer, "m(new Derived())");
    assert_eq!(stale["error"]["code"], -32602, "{stale}");
    assert!(stale.get("result").is_none());
    replace_source(&mut session, &derived_uri, 6, constructor);
    let repaired = source_definition(&mut session, 41, &use_uri, consumer, "m(new Derived())");
    assert_eq!(repaired.get("result"), Some(&location), "{repaired}");
    session.send(json!({"jsonrpc":"2.0","id":42,"method":"shutdown","params":null}));
    assert_eq!(session.response(42)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
    fixture.assert_sources_unchanged();
}

#[test]
fn native_string_arguments_follow_unsaved_literal_and_formal_edits() {
    let fixture = ReferenceCallFixture::new();
    // Signature-only final String data, assembled without Java. Real platform
    // String and its generic ancestry are covered by the optional oracle.
    let mut string = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 5, 1, 0, 16];
    string.extend_from_slice(b"java/lang/String");
    string.extend_from_slice(&[7, 0, 1, 1, 0, 16]);
    string.extend_from_slice(b"java/lang/Object");
    string.extend_from_slice(&[7, 0, 3, 0, 0x31, 0, 2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0]);
    std::fs::write(fixture.root.join("platform/java/lang/String.class"), string).unwrap();
    let mut session = fixture.session();
    let use_uri = fixture.uri("Use");
    let receiver_uri = fixture.uri("Receiver");
    let consumer =
        "package p;\r\npublic class Use { void run(Receiver receiver) { /* 😀 */ receiver.m(\"你好😀\"); } }\r\n";
    let receiver = "package p;\r\npublic class Receiver { /* 😀 */ public void m(java.lang.String value) {} }\r\n";
    open_source(&mut session, &use_uri, consumer);
    open_source(&mut session, &receiver_uri, receiver);
    let location = method_location(&receiver_uri, receiver, "m(java.lang.String value)", "m");
    let initial = source_definition(&mut session, 50, &use_uri, consumer, "m(");
    assert_eq!(initial.get("result"), Some(&location), "{initial}");
    for (version, expression) in [(2, "null"), (4, "\"\\q\""), (6, "'x'")] {
        let changed = consumer.replace("\"你好😀\"", expression);
        replace_source(&mut session, &use_uri, version, &changed);
        let refused = source_definition(&mut session, 50 + version, &use_uri, &changed, "m(");
        assert_eq!(refused.get("result"), Some(&Value::Null), "{refused}");
        replace_source(&mut session, &use_uri, version + 1, consumer);
        let repaired = source_definition(&mut session, 51 + version, &use_uri, consumer, "m(");
        assert_eq!(repaired.get("result"), Some(&location), "{repaired}");
    }
    let changed = receiver.replace("java.lang.String", "Base");
    replace_source(&mut session, &receiver_uri, 2, &changed);
    let refused = source_definition(&mut session, 60, &use_uri, consumer, "m(");
    assert_eq!(refused.get("result"), Some(&Value::Null), "{refused}");
    replace_source(&mut session, &receiver_uri, 3, receiver);
    let repaired = source_definition(&mut session, 61, &use_uri, consumer, "m(");
    assert_eq!(repaired.get("result"), Some(&location), "{repaired}");
    replace_source(&mut session, &use_uri, 7, consumer);
    let stale = source_definition(&mut session, 62, &use_uri, consumer, "m(");
    assert_eq!(stale["error"]["code"], -32602, "{stale}");
    replace_source(&mut session, &use_uri, 8, consumer);
    let repaired = source_definition(&mut session, 63, &use_uri, consumer, "m(");
    assert_eq!(repaired.get("result"), Some(&location), "{repaired}");
    session.send(json!({"jsonrpc":"2.0","id":64,"method":"shutdown","params":null}));
    assert_eq!(session.response(64)["result"], Value::Null);
    session.send(json!({"jsonrpc":"2.0","method":"exit"}));
    assert!(session.wait_exit().success());
    fixture.assert_sources_unchanged();
}
