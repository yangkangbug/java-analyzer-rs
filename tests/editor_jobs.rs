#![cfg(feature = "cli")]

use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

const INSPECT: &str = "__editor-inspect-v1";
const DEFINITION: &str = "__editor-definition-v1";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let f = Self(std::env::temp_dir().join(format!(
            "java-analyzer-editor-job-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir_all(&f.0).unwrap();
        f.write("project.json", json!({"schema_version":1,"compilations":[{"id":{"project":"app","source_set":"main"},"source_roots":["src"],"dependencies":[{"project":"lib","source_set":"main"}]},{"id":{"project":"lib","source_set":"main"},"source_roots":["lib"]}]}).to_string());
        f.write("src/A.java", "class A extends Lib { void run() { old++; } }");
        f.write("lib/Lib.java", "public class Lib { public int old; }");
        f
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn run(&self, command: &str, flag: &str, path: &str, success: bool) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_java-analyzer"))
            .args([command, flag, path])
            .env_clear()
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(if success { 0 } else { 2 }),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let bytes = if success {
            assert!(out.stderr.is_empty());
            out.stdout
        } else {
            assert!(out.stdout.is_empty());
            out.stderr
        };
        serde_json::from_slice(&bytes).unwrap()
    }
    fn inspect(&self) -> Value {
        self.run(INSPECT, "--manifest", self.0.join("project.json").to_str().unwrap(), true)
    }
    fn job(&self, text: &str, line: u32, character: u32) -> Value {
        json!({"schema_version":1,"frozen":self.inspect()["result"]["frozen"],"generation":7,"overlays":{"schema_version":1,"revision":7,"files":[{"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","version":2,"text":text}]},"query":{"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","line":line,"character":character}})
    }
    fn definition(&self, job: &Value, success: bool) -> Value {
        let path = self.write("detached/input/job.json", job.to_string());
        self.run(DEFINITION, "--job", path.to_str().unwrap(), success)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inspect_freezes_normalized_config_and_unique_canonical_owners() {
    let f = Fixture::new();
    let out = f.inspect();
    assert_eq!(out["command"], INSPECT);
    assert_eq!(
        out["result"]["frozen"]["manifest_path"],
        f.0.join("project.json").canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(out["result"]["frozen"]["base"], f.0.canonicalize().unwrap().to_str().unwrap());
    assert!(out["result"]["frozen"]["manifest"]["limits"].is_object());
    assert!(out["result"]["frozen"]["manifest"]["compilations"][0]["limits"].is_object());
    let sources = out["result"]["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().any(|source| source["compilation"]["project"] == "lib"
        && source["file"] == f.0.join("lib/Lib.java").canonicalize().unwrap().to_str().unwrap()));
}

#[test]
fn unsaved_query_and_closed_target_use_frozen_config_and_same_loaded_utf16_ranges() {
    let f = Fixture::new();
    f.write("lib/Lib.java", "// 😀\r\npublic class Lib { /* 😀 */ public int fresh; }\r\n");
    let text = "// 😀\r\nclass A extends Lib { void run() { fresh++; } }\r\n";
    let job = f.job(text, 1, 35);
    f.write("project.json", "not JSON any longer");
    f.write("detached/input/project.json", "also not JSON");
    let result = f.definition(&job, true)["result"].clone();
    assert_eq!(result["generation"], 7);
    assert_eq!(result["status"], "Resolved", "{result}");
    assert!(result["refusal"].is_null());
    assert_eq!(result["location"]["path"], f.0.join("lib/Lib.java").canonicalize().unwrap().to_str().unwrap());
    assert_eq!(result["location"]["range"], json!({"start":{"line":1,"character":39},"end":{"line":1,"character":44}}));
    assert_eq!(fs::read_to_string(f.0.join("src/A.java")).unwrap(), "class A extends Lib { void run() { old++; } }");
}

#[test]
fn complete_overlay_set_controls_cross_file_target_ranges() {
    let f = Fixture::new();
    let mut job = f.job("class A extends Lib { void run() { fresh++; } }", 0, 35);
    job["overlays"]["files"].as_array_mut().unwrap().push(json!({"compilation":{"project":"lib","source_set":"main"},"file":"lib/Lib.java","version":9,"text":"// 😀\r\n\r\npublic class Lib { public int fresh; }"}));
    let result = f.definition(&job, true)["result"].clone();
    assert_eq!(result["status"], "Resolved", "{result}");
    assert_eq!(result["location"]["range"], json!({"start":{"line":2,"character":30},"end":{"line":2,"character":35}}));
}

#[test]
fn generation_owner_and_open_query_are_required() {
    let f = Fixture::new();
    let original = f.job("class A extends Lib { void run() { old++; } }", 0, 35);
    for pointer in ["/generation", "/overlays/revision"] {
        let mut job = original.clone();
        *job.pointer_mut(pointer).unwrap() = json!(8);
        assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_overlay");
    }
    let mut job = original.clone();
    job["overlays"]["files"] = json!([]);
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_overlay");
    let mut job = original.clone();
    job["query"]["compilation"]["project"] = json!("lib");
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_overlay");
    let mut job = original;
    job["overlays"]["files"][0]["compilation"]["project"] = json!("lib");
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_overlay");
}

#[test]
fn lsp_only_clamps_excess_columns_and_eof_is_a_null_result() {
    let f = Fixture::new();
    let text = "// 😀\r\nclass A {}\r\n";
    let job = f.job(text, 1, u32::MAX);
    assert!(f.definition(&job, true)["result"]["location"].is_null());
    let mut job = job.clone();
    job["query"]["line"] = json!(2);
    let result = f.definition(&job, true)["result"].clone();
    assert!(result["location"].is_null());
    assert_eq!(result["refusal"], "eof");
    for (line, character) in [(0, 4), (3, 0), (u32::MAX, 0)] {
        job["query"]["line"] = json!(line);
        job["query"]["character"] = json!(character);
        assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_position");
    }
}

#[test]
fn job_shapes_labels_and_document_caps_are_strict() {
    let f = Fixture::new();
    let original = f.job("class A {}", 0, 6);
    for (pointer, value) in
        [("/generation", json!(-1)), ("/query/line", json!(1.5)), ("/query/character", json!(4294967296_u64))]
    {
        let mut job = original.clone();
        *job.pointer_mut(pointer).unwrap() = value;
        assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_json");
    }
    let mut job = original.clone();
    job["unexpected"] = json!(true);
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_json");
    let mut job = original.clone();
    job["overlays"]["files"][0]["text"] = json!(" ".repeat(1024 * 1024));
    assert_eq!(f.definition(&job, false)["error"]["code"], "document_limit");
    let mut job = original;
    job["frozen"]["manifest"]["compilations"][0]["source_roots"] = json!(["x".repeat(1024 * 1024)]);
    assert_eq!(f.definition(&job, false)["error"]["code"], "document_limit");
    let path = f.write("too-big.json", vec![b' '; 4 * 1024 * 1024 + 1]);
    assert_eq!(f.run(DEFINITION, "--job", path.to_str().unwrap(), false)["error"]["code"], "document_limit");
    let path = f.write("duplicate.json", "{\"schema_version\":1,\"schema_version\":1}");
    assert_eq!(f.run(DEFINITION, "--job", path.to_str().unwrap(), false)["error"]["code"], "invalid_json");
}

#[cfg(unix)]
#[test]
fn job_file_must_not_be_a_symlink() {
    let f = Fixture::new();
    let job = f.job("class A {}", 0, 6);
    let path = f.write("real.json", job.to_string());
    let link = f.0.join("link.json");
    std::os::unix::fs::symlink(path, &link).unwrap();
    assert_eq!(f.run(DEFINITION, "--job", link.to_str().unwrap(), false)["error"]["code"], "invalid_path");
}

#[test]
fn source_inventory_changes_require_restart_and_manifest_deletion_is_not_a_reload() {
    let f = Fixture::new();
    let job = f.job("class A extends Lib { void run() { old++; } }", 0, 35);
    fs::remove_file(f.0.join("project.json")).unwrap();
    assert_eq!(f.definition(&job, true)["result"]["status"], "Resolved");
    let added = f.write("src/Added.java", "class Added {}");
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_configuration");
    fs::remove_file(added).unwrap();
    fs::remove_file(f.0.join("lib/Lib.java")).unwrap();
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_configuration");
}

#[test]
fn binary_package_ambiguous_and_overload_results_never_fabricate_source_locations() {
    let f = Fixture::new();
    f.write("deps/library.jar", include_bytes!("fixtures/project/deps/library.jar"));
    let manifest_path = f.0.join("project.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["compilations"][0]["classpath"] = json!([{"kind":"jar","path":"deps/library.jar"}]);
    f.write("project.json", manifest.to_string());
    let text = "class A extends lib.Lib { void use() { run(); } }";
    let job = f.job(text, 0, text.rfind("run").unwrap() as u32);
    let result = f.definition(&job, true)["result"].clone();
    assert_eq!(result["status"], "Resolved");
    assert_eq!(result["refusal"], "binary_source_unavailable");
    assert!(result["location"].is_null());
    f.write("src/Deep.java", "package lib.deep; public class Deep { public static void f() {} }");
    f.write("src/B1.java", "class B {}");
    f.write("src/B2.java", "class B {}");
    for (text, needle, status, refusal) in [
        ("class A { void use() { lib.deep.Deep.f(); } }", "deep.Deep", "Package", "package_source_unavailable"),
        ("class A { B b; }", "B b", "Ambiguous", "not_unique_source_target"),
        (
            "class A { void m() {} void m(int n) {} void use() { m(1); } }",
            "m(1)",
            "OverloadNotResolved",
            "not_unique_source_target",
        ),
    ] {
        let job = f.job(text, 0, text.find(needle).unwrap() as u32);
        let result = f.definition(&job, true)["result"].clone();
        assert_eq!(result["status"], status, "{result}");
        assert_eq!(result["refusal"], refusal);
        assert!(result["location"].is_null());
    }
}

#[test]
fn nested_private_fields_and_frozen_original_base_are_strict() {
    let f = Fixture::new();
    let original = f.job("class A {}", 0, 6);
    for pointer in [
        "/frozen",
        "/frozen/manifest",
        "/frozen/sources/0",
        "/query",
        "/query/compilation",
        "/overlays",
        "/overlays/files/0",
    ] {
        let mut job = original.clone();
        job.pointer_mut(pointer).unwrap()["unexpected"] = json!(true);
        assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_json");
    }
    for pointer in ["/frozen/base", "/frozen/manifest_path"] {
        let mut job = original.clone();
        *job.pointer_mut(pointer).unwrap() = json!("relative");
        assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_configuration");
    }
    let mut job = original;
    job["frozen"]["base"] = json!(std::env::temp_dir().to_str().unwrap());
    assert_eq!(f.definition(&job, false)["error"]["code"], "invalid_configuration");
}
