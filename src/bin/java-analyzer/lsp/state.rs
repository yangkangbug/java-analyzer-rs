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

#[derive(Debug, PartialEq, Eq)]
enum OpenOutcome {
    Admitted,
    NotSelected,
}
pub(super) struct Admission {
    outcome: OpenOutcome,
    uri: String,
    canonical_path: String,
    version: i32,
}
impl Admission {
    pub fn response(self) -> Value {
        let status = match self.outcome {
            OpenOutcome::Admitted => "admitted",
            OpenOutcome::NotSelected => "not-selected",
        };
        json!({"status":status,"uri":self.uri,"canonicalPath":self.canonical_path,"version":self.version})
    }
}

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
    fn poison<T>(&mut self, uri: Option<&str>, reason: &str) -> Result<T, String> {
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
    /// Acknowledge one candidate against the initialized inventory. This is a
    /// synchronous state transition, not a transactional or cancellable probe.
    pub fn admit(&mut self, params: &Value) -> Result<Admission, String> {
        let value = &params["textDocument"];
        let uri = value.get("uri").and_then(Value::as_str).ok_or("admission requires a document URI")?;
        if uri.len() > MAX_URI {
            return Err("document URI limit exceeded".into());
        }
        let expected = params
            .get("expectedCanonicalPath")
            .and_then(Value::as_str)
            .filter(|p| p.len() <= MAX_URI && PathBuf::from(p).is_absolute() && !p.chars().any(char::is_control))
            .ok_or("admission requires a bounded absolute expectedCanonicalPath")?;
        let version = value
            .get("version")
            .and_then(Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
            .ok_or("admission version must be signed i32")?;
        if value.get("text").and_then(Value::as_str).is_none()
            || value.get("languageId").and_then(Value::as_str) != Some("java")
        {
            return Err("admission requires Java full text".into());
        }
        // Canonicalize exactly once, then verify identity before ANY mutation,
        // including duplicate-URI poisoning. Never preflight and recanonicalize.
        let path = resolve_uri(uri)?;
        if path != expected {
            return Err("document canonical identity differs from expectedCanonicalPath".into());
        }
        let outcome = self.open_resolved(uri, path.clone(), params)?;
        Ok(Admission { outcome, uri: uri.to_owned(), canonical_path: path, version })
    }
    fn reject_duplicate_uri(&mut self, uri: &str) -> Result<(), String> {
        if let Some(path) = self.uris.get(uri).cloned() {
            self.advance()?;
            let document = self.documents.get_mut(&path).expect("registered URI");
            document.desynchronized = true;
            document.conflicted = true;
            return Err("duplicate active URI; close it and reopen before using a changed path".into());
        }
        Ok(())
    }
    fn open(&mut self, uri: &str, params: &Value) -> Result<(), String> {
        // Preserve didOpen's duplicate ordering even if the path has vanished.
        self.reject_duplicate_uri(uri)?;
        let path = resolve_uri(uri)?;
        match self.open_resolved(uri, path, params)? {
            OpenOutcome::Admitted => Ok(()),
            OpenOutcome::NotSelected => {
                Err("document is outside the frozen source inventory; restart after configuration/source changes"
                    .into())
            }
        }
    }
    fn open_resolved(&mut self, uri: &str, path: String, params: &Value) -> Result<OpenOutcome, String> {
        self.reject_duplicate_uri(uri)?;
        let Some(compilation) = self.sources.get(&path).cloned() else {
            // The only benign negative, reached before mutation. A retargeted
            // already-registered URI above must remain a conflict, not a skip.
            return Ok(OpenOutcome::NotSelected);
        };
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
        Ok(OpenOutcome::Admitted)
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

fn resolve_uri(uri: &str) -> Result<String, String> {
    let path = uri_path(uri)?.canonicalize().map_err(|_| "document path cannot be resolved")?;
    path.to_str().map(str::to_owned).ok_or_else(|| "document path must be UTF-8".into())
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
    fn admission_params(uri: &str, expected: &str, version: i32) -> Value {
        json!({"expectedCanonicalPath":expected,"textDocument":{"uri":uri,"languageId":"java","version":version,"text":"class Dirty {}"}})
    }
    #[test]
    fn admission_checks_identity_and_fields_before_not_selected_or_mutation() {
        let mut f = Fixture::new();
        let outside = f.root.join("Outside.java");
        std::fs::write(&outside, "class Outside {}").unwrap();
        let uri = file_uri(outside.to_str().unwrap()).unwrap();
        let params = admission_params(&uri, outside.to_str().unwrap(), -4);
        for (field, value) in
            [("version", json!(2147483648_i64)), ("text", json!(false)), ("languageId", json!("other"))]
        {
            let mut invalid = params.clone();
            invalid["textDocument"][field] = value;
            assert!(f.workspace.admit(&invalid).is_err());
        }
        let mut invalid = params.clone();
        invalid["expectedCanonicalPath"] = json!("x".repeat(MAX_URI + 1));
        assert!(f.workspace.admit(&invalid).is_err());
        invalid["expectedCanonicalPath"] = json!(outside);
        invalid["textDocument"]["uri"] = json!("x".repeat(MAX_URI + 1));
        assert!(f.workspace.admit(&invalid).is_err());
        assert!(f.workspace.admit(&admission_params(&f.one, f.root.join("B.java").to_str().unwrap(), 1)).is_err());
        assert_eq!(f.workspace.generation, 0);
        assert!(f.workspace.uris.is_empty());
        assert_eq!(f.workspace.admit(&params).unwrap().response()["status"], "not-selected");
        assert_eq!(f.workspace.generation, 0);
        assert!(f.workspace.documents.is_empty());
        std::fs::remove_file(outside).unwrap();
        assert!(f.workspace.admit(&params).is_err(), "vanished path is an error, not not-selected");
        assert_eq!(f.workspace.generation, 0);
    }
    #[test]
    fn admission_uses_exact_frozen_owner_for_nested_paths() {
        let mut f = Fixture::new();
        let nested = f.root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let path = nested.join("N.java");
        std::fs::write(&path, "class N {}").unwrap();
        let owner = json!({"project":"nested","source_set":"test"});
        f.workspace.sources.insert(path.to_str().unwrap().to_owned(), owner.clone());
        let uri = file_uri(path.to_str().unwrap()).unwrap();
        let receipt = f.workspace.admit(&admission_params(&uri, path.to_str().unwrap(), i32::MIN)).unwrap().response();
        assert_eq!(receipt, json!({"status":"admitted","uri":uri,"canonicalPath":path,"version":i32::MIN}));
        assert_eq!(f.query(&uri).unwrap()["query"]["compilation"], owner);
        let unknown = nested.join("New.java");
        std::fs::write(&unknown, "class New {}").unwrap();
        let unknown_uri = file_uri(unknown.to_str().unwrap()).unwrap();
        assert_eq!(
            f.workspace.admit(&admission_params(&unknown_uri, unknown.to_str().unwrap(), 1)).unwrap().response()["status"],
            "not-selected"
        );
    }
    #[cfg(unix)]
    #[test]
    fn admission_canonicalizes_once_and_later_sync_keeps_registered_identity() {
        let mut f = Fixture::new();
        let link = f.root.join("alias.java");
        let original = f.root.join("A.java");
        std::os::unix::fs::symlink(&original, &link).unwrap();
        let uri = file_uri(link.to_str().unwrap()).unwrap();
        let params = admission_params(&uri, original.to_str().unwrap(), -2);
        f.workspace.admit(&params).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(f.root.join("B.java"), &link).unwrap();
        let before = f.workspace.generation;
        assert!(f.workspace.admit(&params).is_err(), "mismatched expected identity is rejected before mutation");
        assert_eq!(f.workspace.generation, before);
        f.change(&uri, -1, json!([{"text":"class Updated {}"}])).unwrap();
        assert_eq!(f.query(&uri).unwrap()["query"]["file"], json!(original));
        std::fs::remove_file(&link).unwrap();
        f.change(&uri, 0, json!([{"text":"class DeletedPath {}"}])).unwrap();
        f.workspace.notify("textDocument/didClose", &json!({"textDocument":{"uri":uri}})).unwrap();
        assert!(f.workspace.documents.is_empty());
        // The shared transition must use its verified identity, not resolve the URI a second time.
        std::os::unix::fs::symlink(&original, &link).unwrap();
        let resolved = resolve_uri(&uri).unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(f.root.join("B.java"), &link).unwrap();
        assert_eq!(f.workspace.open_resolved(&uri, resolved, &params).unwrap(), OpenOutcome::Admitted);
        assert_eq!(f.query(&uri).unwrap()["query"]["file"], json!(original));
    }
    #[cfg(unix)]
    #[test]
    fn admission_duplicate_registered_retarget_to_unselected_is_error_not_false() {
        let mut f = Fixture::new();
        let link = f.root.join("alias.java");
        let original = f.root.join("A.java");
        std::os::unix::fs::symlink(&original, &link).unwrap();
        let uri = file_uri(link.to_str().unwrap()).unwrap();
        f.workspace.admit(&admission_params(&uri, original.to_str().unwrap(), 1)).unwrap();
        let outside = f.root.join("Outside.java");
        std::fs::write(&outside, "class Outside {}").unwrap();
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let before = f.workspace.generation;
        assert!(f.workspace.admit(&admission_params(&uri, outside.to_str().unwrap(), 2)).is_err());
        assert!(f.workspace.generation > before);
        assert_eq!(f.workspace.uris[&uri], original.to_str().unwrap());
        assert!(f.query(&uri).is_err());
    }
    #[test]
    fn admission_duplicate_alias_and_overlay_errors_remain_fail_closed() {
        let mut f = Fixture::new();
        let path = f.root.join("A.java");
        let uri = f.one.clone();
        let params = admission_params(&uri, path.to_str().unwrap(), 1);
        f.workspace.admit(&params).unwrap();
        let before = f.workspace.generation;
        assert!(f.workspace.admit(&params).is_err());
        assert!(f.workspace.generation > before);
        assert!(f.query(&uri).is_err());
        f.workspace.notify("textDocument/didClose", &json!({"textDocument":{"uri":uri}})).unwrap();
        f.workspace.admit(&params).unwrap();
        let alias = uri.replacen("file://", "file://localhost", 1);
        assert!(f.workspace.admit(&admission_params(&alias, path.to_str().unwrap(), 1)).is_err());
        assert!(f.workspace.uris.contains_key(&alias));
        assert!(f.query(&uri).is_err());
        for close_uri in [&uri, &alias] {
            f.workspace.notify("textDocument/didClose", &json!({"textDocument":{"uri":close_uri}})).unwrap();
        }
        let mut oversize = params.clone();
        oversize["textDocument"]["text"] = json!("x".repeat(MAX_OVERLAY_BYTES));
        assert!(f.workspace.admit(&oversize).is_err());
        assert!(f.workspace.uris.contains_key(&uri));
        assert!(f.workspace.overlay_bytes().unwrap() < MAX_OVERLAY_BYTES);
        assert!(f.query(&uri).is_err());
    }
    #[test]
    fn admission_retains_document_and_safe_generation_limits() {
        let mut f = Fixture::new();
        for n in 0..MAX_OVERLAYS {
            f.workspace.uris.insert(format!("file:///fake/{n}"), String::new());
        }
        let path = f.root.join("A.java");
        assert!(f.workspace.admit(&admission_params(&f.one, path.to_str().unwrap(), 1)).is_err());
        assert!(f.workspace.broken);
        let mut f = Fixture::new();
        f.workspace.generation = MAX_LABEL;
        let path = f.root.join("A.java");
        assert!(f.workspace.admit(&admission_params(&f.one, path.to_str().unwrap(), 1)).is_err());
        assert_eq!(f.workspace.generation, MAX_LABEL);
        assert!(f.workspace.documents.is_empty());
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
