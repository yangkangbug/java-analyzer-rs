//! Full-text buffer identities. Java is never parsed in this module.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub(super) const MAX_OVERLAYS: usize = 1024;
pub(super) const MAX_OVERLAY_BYTES: usize = 1024 * 1024;
const MAX_LABEL: u64 = 9_007_199_254_740_991;
const MAX_URI: usize = 8192;

#[derive(Clone)]
struct Document {
    compilation: Value,
    uris: BTreeSet<String>,
    version: i32,
    incarnation: u64,
    label: u64,
    text: String,
    desynchronized: bool,
    conflicted: bool,
}

pub(super) struct Workspace {
    pub frozen: Value,
    pub generation: u64,
    sources: BTreeMap<String, Value>,
    documents: BTreeMap<String, Document>,
    uris: BTreeMap<String, String>,
    broken: bool,
}
impl Workspace {
    pub fn from_inspect(result: &Value) -> Result<Self, String> {
        let frozen =
            result.get("frozen").filter(|v| v.is_object()).ok_or("inspection omitted frozen configuration")?.clone();
        let entries = result.get("sources").and_then(Value::as_array).ok_or("inspection omitted source inventory")?;
        let mut sources = BTreeMap::new();
        for entry in entries {
            let file = entry.get("file").and_then(Value::as_str).ok_or("invalid source inventory path")?;
            let id = entry
                .get("compilation")
                .filter(|v| v["project"].is_string() && v["source_set"].is_string())
                .ok_or("invalid source inventory owner")?;
            if !PathBuf::from(file).is_absolute() || sources.insert(file.to_owned(), id.clone()).is_some() {
                return Err("invalid or duplicate source inventory".into());
            }
        }
        Ok(Self { frozen, generation: 0, sources, documents: BTreeMap::new(), uris: BTreeMap::new(), broken: false })
    }
    fn advance(&mut self) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .filter(|n| *n <= MAX_LABEL)
            .ok_or("workspace revision exhausted; restart required")?;
        Ok(())
    }
    fn poison(&mut self, uri: Option<&str>, reason: &str) -> Result<(), String> {
        self.advance()?;
        if let Some(path) = uri.and_then(|uri| self.uris.get(uri)) {
            if let Some(document) = self.documents.get_mut(path) {
                document.desynchronized = true;
            }
        } else {
            self.broken = true;
        }
        Err(reason.into())
    }
    pub fn notify(&mut self, method: &str, params: &Value) -> Result<(), String> {
        let uri = params.get("textDocument").and_then(|d| d.get("uri")).and_then(Value::as_str);
        let Some(uri) = uri else {
            return self.poison(None, "synchronization notification omitted a valid URI; restart required");
        };
        if uri.len() > MAX_URI {
            return self.poison(None, "document URI limit exceeded; restart required");
        }
        match method {
            "textDocument/didOpen" => self.open(uri, params),
            "textDocument/didChange" => self.change(uri, params),
            "textDocument/didClose" => {
                // Exact URI identity is required. A rejected/unseen alias cannot
                // close a different URI merely because canonical paths match.
                if let Some(path) = self.uris.remove(uri) {
                    self.advance()?;
                    let document = self.documents.get_mut(&path).expect("registered URI");
                    document.uris.remove(uri);
                    if document.uris.is_empty() {
                        self.documents.remove(&path);
                    }
                }
                Ok(())
            }
            "textDocument/didSave" => {
                if self.uris.contains_key(uri) {
                    self.advance()?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    fn open(&mut self, uri: &str, params: &Value) -> Result<(), String> {
        if let Some(path) = self.uris.get(uri).cloned() {
            self.advance()?;
            let document = self.documents.get_mut(&path).expect("registered URI");
            document.desynchronized = true;
            document.conflicted = true;
            return Err("duplicate active URI; close it and reopen before using a changed path".into());
        }
        let path = uri_path(uri)?.canonicalize().map_err(|_| "document path cannot be resolved")?;
        let path = path.to_str().ok_or("document path must be UTF-8")?.to_owned();
        let compilation = self
            .sources
            .get(&path)
            .ok_or("document is outside the frozen source inventory; restart after configuration/source changes")?
            .clone();
        if self.uris.len() >= MAX_OVERLAYS && !self.uris.contains_key(uri) {
            return self.poison(None, "open-document limit exceeded; restart required");
        }
        self.advance()?;
        if let Some(document) = self.documents.get_mut(&path) {
            document.uris.insert(uri.to_owned());
            document.desynchronized = true;
            document.conflicted = true;
            self.uris.insert(uri.to_owned(), path);
            return Err("duplicate source open; close every alias and reopen once".into());
        }
        let value = &params["textDocument"];
        let version = value.get("version").and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok());
        let text = value.get("text").and_then(Value::as_str);
        let valid =
            version.is_some() && text.is_some() && value.get("languageId").and_then(Value::as_str) == Some("java");
        self.uris.insert(uri.to_owned(), path.clone());
        self.documents.insert(
            path.clone(),
            Document {
                compilation,
                uris: BTreeSet::from([uri.to_owned()]),
                version: version.unwrap_or(i32::MIN),
                incarnation: self.generation,
                label: self.generation,
                text: if valid { text.unwrap_or("").to_owned() } else { String::new() },
                desynchronized: !valid,
                conflicted: false,
            },
        );
        if !valid {
            return Err("invalid didOpen; source is desynchronized".into());
        }
        if self.overlay_bytes()? > MAX_OVERLAY_BYTES {
            let document = self.documents.get_mut(&path).expect("inserted document");
            document.text.clear();
            document.desynchronized = true;
            return Err("serialized overlays exceed 1 MiB; source is desynchronized".into());
        }
        Ok(())
    }
    fn change(&mut self, uri: &str, params: &Value) -> Result<(), String> {
        let Some(path) = self.uris.get(uri).cloned() else {
            return self.poison(None, "didChange for a document that is not open; restart required");
        };
        let version = params["textDocument"].get("version").and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok());
        let current = self.documents.get(&path).expect("registered URI");
        let Some(version) = version.filter(|n| *n > current.version) else {
            return self
                .poison(Some(uri), "document versions must be signed i32 and increase; source is desynchronized");
        };
        self.documents.get_mut(&path).expect("registered URI").version = version;
        let Some(changes) = params.get("contentChanges").and_then(Value::as_array) else {
            return self.poison(Some(uri), "contentChanges must be an array of full replacements");
        };
        let mut text = None;
        for change in changes {
            if change.get("range").is_some() || change.get("rangeLength").is_some() {
                return self.poison(Some(uri), "incremental changes are unsupported; source is desynchronized");
            }
            let Some(replacement) = change.get("text").and_then(Value::as_str) else {
                return self.poison(Some(uri), "full replacement omitted text; source is desynchronized");
            };
            text = Some(replacement);
        }
        self.advance()?;
        let document = self.documents.get_mut(&path).expect("registered URI");
        let old = text.map(|text| std::mem::replace(&mut document.text, text.to_owned()));
        document.version = version;
        document.label = self.generation;
        document.desynchronized = document.conflicted || (text.is_none() && document.desynchronized);
        if self.overlay_bytes()? > MAX_OVERLAY_BYTES {
            let document = self.documents.get_mut(&path).expect("registered URI");
            if let Some(old) = old {
                document.text = old;
            }
            document.desynchronized = true;
            return Err("serialized overlays exceed 1 MiB; source is desynchronized".into());
        }
        if self.documents[&path].conflicted {
            return Err("conflicting source aliases require closing every alias and reopening once".into());
        }
        Ok(())
    }
    fn overlays(&self) -> Value {
        let files: Vec<_> = self.documents.iter().map(|(path, document)| {
            json!({"compilation":document.compilation,"file":path,"version":document.label,"text":document.text})
        }).collect();
        json!({"schema_version":1,"revision":self.generation,"files":files})
    }
    fn overlay_bytes(&self) -> Result<usize, String> {
        serde_json::to_vec(&self.overlays()).map(|b| b.len()).map_err(|e| e.to_string())
    }
    pub fn definition(&self, params: &Value) -> Result<Value, String> {
        if self.broken || self.documents.values().any(|d| d.desynchronized) {
            return Err("workspace has desynchronized buffers; repair them or restart".into());
        }
        let uri = params["textDocument"]["uri"].as_str().ok_or("definition requires a document URI")?;
        let path = self.uris.get(uri).ok_or("definition requires an open synchronized owned document")?;
        let document = &self.documents[path];
        let coordinate = |name| {
            params["position"][name]
                .as_u64()
                .filter(|n| *n <= i32::MAX as u64)
                .ok_or("position must contain nonnegative LSP uintegers")
        };
        let (line, character) = (coordinate("line")?, coordinate("character")?);
        let _ = document.incarnation; // Captured transitively by globally unique generation.
        Ok(json!({"schema_version":1,"frozen":self.frozen,"generation":self.generation,
            "overlays":self.overlays(),"query":{"compilation":document.compilation,"file":path,"line":line,"character":character}}))
    }
    pub fn target_uri(&self, path: &str) -> Result<String, String> {
        if let Some(document) = self.documents.get(path)
            && !document.desynchronized
            && document.uris.len() == 1
        {
            return Ok(document.uris.first().expect("one URI").clone());
        }
        file_uri(path)
    }
}

pub(super) fn uri_path(uri: &str) -> Result<PathBuf, String> {
    if uri.len() > MAX_URI || uri.contains(['?', '#']) {
        return Err("unsupported document URI".into());
    }
    let prefix = uri.get(..7).ok_or("expected a local file URI")?;
    if !prefix.eq_ignore_ascii_case("file://") {
        return Err("only local file URIs are supported".into());
    }
    let rest = &uri[7..];
    let slash = rest.find('/').ok_or("file URI requires an absolute path")?;
    let authority = &rest[..slash];
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        return Err("remote file URI authorities are unsupported".into());
    }
    let encoded = &rest.as_bytes()[slash..];
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut at = 0;
    while at < encoded.len() {
        let byte = if encoded[at] == b'%' {
            let digits = encoded.get(at + 1..at + 3).ok_or("invalid URI escape")?;
            let hex = |b: u8| (b as char).to_digit(16).map(|n| n as u8).ok_or("invalid URI escape");
            at += 3;
            hex(digits[0])? * 16 + hex(digits[1])?
        } else {
            let byte = encoded[at];
            at += 1;
            byte
        };
        if byte == 0 || byte < 0x20 || byte == 0x7f {
            return Err("invalid control byte in file URI".into());
        }
        bytes.push(byte);
    }
    let path = String::from_utf8(bytes).map_err(|_| "file URI path must be UTF-8")?;
    if cfg!(windows) {
        return Err("this preview's file URI adapter is Unix-only".into());
    }
    Ok(PathBuf::from(path))
}
fn file_uri(path: &str) -> Result<String, String> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err("source path is not a supported absolute path".into());
    }
    let mut uri = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(byte as char);
        } else {
            use std::fmt::Write;
            write!(uri, "%{byte:02X}").expect("string write");
        }
    }
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uri_round_trip_reserved_and_unicode() {
        let path = "/tmp/a # %/中😀.java";
        assert_eq!(uri_path(&file_uri(path).unwrap()).unwrap(), PathBuf::from(path));
        for uri in [
            "http:///tmp/A.java",
            "file://server/tmp/A.java",
            "file:///tmp/A.java?x",
            "file:///tmp/%00",
            "file:///tmp/%XY",
        ] {
            assert!(uri_path(uri).is_err());
        }
    }
    struct Fixture {
        root: PathBuf,
        workspace: Workspace,
        one: String,
        two: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "lsp-state-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            let root = root.canonicalize().unwrap();
            let one = root.join("A.java");
            let two = root.join("B.java");
            std::fs::write(&one, "class A {}").unwrap();
            std::fs::write(&two, "class B {}").unwrap();
            let workspace = Workspace::from_inspect(&json!({"frozen":{},"sources":[
                {"file":one.to_str().unwrap(),"compilation":{"project":"p","source_set":"main"}},
                {"file":two.to_str().unwrap(),"compilation":{"project":"p","source_set":"main"}}
            ]}))
            .unwrap();
            Self {
                root,
                workspace,
                one: file_uri(one.to_str().unwrap()).unwrap(),
                two: file_uri(two.to_str().unwrap()).unwrap(),
            }
        }
        fn open(&mut self, uri: &str, version: i32) -> Result<(), String> {
            self.workspace.notify(
                "textDocument/didOpen",
                &json!({"textDocument":{"uri":uri,"languageId":"java","version":version,"text":"class A {}"}}),
            )
        }
        fn change(&mut self, uri: &str, version: i32, changes: Value) -> Result<(), String> {
            self.workspace.notify(
                "textDocument/didChange",
                &json!({"textDocument":{"uri":uri,"version":version},"contentChanges":changes}),
            )
        }
        fn query(&self, uri: &str) -> Result<Value, String> {
            self.workspace.definition(&json!({"textDocument":{"uri":uri},"position":{"line":0,"character":6}}))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn empty_changes_cannot_repair_poisoned_dependency() {
        let mut f = Fixture::new();
        let a = f.one.clone();
        let b = f.two.clone();
        f.open(&a, -4).unwrap();
        f.open(&b, -10).unwrap();
        assert!(f.change(&b, 3, json!([{"range":{"start":{},"end":{}},"text":"bad"}])).is_err());
        assert!(f.query(&a).is_err());
        f.change(&b, 7, json!([])).unwrap();
        assert!(f.query(&a).is_err(), "no replacement text was supplied");
        f.change(&b, 8, json!([{"text":"old"},{"text":"class B {}"}])).unwrap();
        assert!(f.query(&a).is_ok());
    }
    #[test]
    fn invalid_open_never_retains_unbounded_text() {
        let mut f = Fixture::new();
        assert!(f.workspace.notify("textDocument/didOpen", &json!({"textDocument":{"uri":f.one,"languageId":"wrong","version":0,"text":"x".repeat(2*MAX_OVERLAY_BYTES)}})).is_err());
        assert!(f.workspace.overlay_bytes().unwrap() < MAX_OVERLAY_BYTES);
        assert!(f.query(&f.one).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn duplicate_open_does_not_remap_retargeted_active_uri() {
        let mut f = Fixture::new();
        let link = f.root.join("alias.java");
        std::os::unix::fs::symlink(f.root.join("A.java"), &link).unwrap();
        let uri = file_uri(link.to_str().unwrap()).unwrap();
        f.open(&uri, 1).unwrap();
        let original = f.workspace.uris[&uri].clone();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(f.root.join("B.java"), &link).unwrap();
        assert!(f.open(&uri, 2).is_err());
        assert_eq!(f.workspace.uris[&uri], original);
        assert!(f.query(&uri).is_err());
        f.workspace.notify("textDocument/didClose", &json!({"textDocument":{"uri":uri}})).unwrap();
        assert!(f.workspace.documents.is_empty());
    }
    #[test]
    fn duplicate_open_of_disappeared_file_poisoned_original() {
        let mut f = Fixture::new();
        let uri = f.one.clone();
        f.open(&uri, 1).unwrap();
        let before = f.workspace.generation;
        std::fs::remove_file(f.root.join("A.java")).unwrap();
        assert!(f.open(&uri, 2).is_err());
        assert!(f.workspace.generation > before);
        assert!(f.query(&uri).is_err());
    }
}
