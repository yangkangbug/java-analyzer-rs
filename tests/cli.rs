#![cfg(feature = "cli")]

use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "java-analyzer-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, data: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }
    fn manifest(&self) -> PathBuf {
        self.write("project.json", json!({"schema_version":1,"compilations":[{"id":{"project":"app","source_set":"main"},"source_roots":["src"]}]}).to_string())
    }
    fn run(&self, args: &[&str], success: bool) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_java-analyzer"))
            .args(args)
            .current_dir(std::env::temp_dir())
            .env_clear()
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if success { 0 } else { 2 }),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let data = if success {
            assert!(output.stderr.is_empty(), "{:?}", output.stderr);
            output.stdout
        } else {
            assert!(output.stdout.is_empty(), "{:?}", output.stdout);
            output.stderr
        };
        serde_json::from_slice(&data).unwrap()
    }
    fn declaration(&self, file: &str, byte: usize) -> Value {
        self.run(
            &[
                "declaration",
                "--manifest",
                self.0.join("project.json").to_str().unwrap(),
                "--project",
                "app",
                "--source-set",
                "main",
                "--file",
                file,
                "--byte",
                &byte.to_string(),
            ],
            true,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn manifest_relative_subprocess_query_has_exact_source_range() {
    let f = Fixture::new();
    let text = "class A { int count; void run() { count++; } }";
    let path = f.write("src/A.java", text);
    let manifest = f.manifest();
    let inspection = f.run(&["inspect", "--manifest", manifest.to_str().unwrap()], true);
    assert_eq!(
        inspection["result"]["compilations"][0]["source_paths"][0],
        path.canonicalize().unwrap().to_str().unwrap()
    );
    let response = f.declaration("src/A.java", text.rfind("count").unwrap());
    assert_eq!(response["schema_version"], 1);
    assert_eq!(response["snapshot"]["identity"], "invocation_local");
    let result = &response["result"]["queries"][0];
    assert_eq!(result["status"], "Resolved");
    let target = &result["targets"][0];
    assert_eq!(target["origin"], "source");
    assert_eq!(target["range"]["start_byte"], text.find("count").unwrap());
    assert_eq!(target["range"]["end_byte"], text.find("count").unwrap() + 5);
    assert_eq!(target["compilation"]["project"], "app");
}

#[test]
fn unknown_manifest_fields_and_invalid_queries_are_json_errors() {
    let f = Fixture::new();
    f.write("bad.json", r#"{"schema_version":1,"compilations":[],"typo":true}"#);
    let response = f.run(&["inspect", "--manifest", f.0.join("bad.json").to_str().unwrap()], false);
    assert_eq!(response["error"]["code"], "invalid_json");
    f.write("src/A.java", "class A {}");
    f.manifest();
    let response = f.run(
        &[
            "declaration",
            "--manifest",
            f.0.join("project.json").to_str().unwrap(),
            "--project",
            "app",
            "--source-set",
            "main",
            "--file",
            "src/A.java",
            "--byte",
            "999",
        ],
        false,
    );
    assert_eq!(response["error"]["code"], "invalid_position");
}

fn query(file: &str, text: &str, needle: &str, project: &str, set: &str) -> Value {
    json!({"compilation":{"project":project,"source_set":set},"file":file,
        "position":{"encoding":"utf8","offset":text.rfind(needle).unwrap()}})
}
impl Fixture {
    fn batch(&self, queries: Vec<Value>, success: bool) -> Value {
        let requests = self.write("elsewhere/requests.json", json!({"schema_version":1,"queries":queries}).to_string());
        self.run(
            &[
                "declaration",
                "--manifest",
                self.0.join("project.json").to_str().unwrap(),
                "--requests",
                requests.to_str().unwrap(),
            ],
            success,
        )
    }
    fn inspect_error(&self, manifest: Value) -> Value {
        let path = self.write("bad.json", manifest.to_string());
        self.run(&["inspect", "--manifest", path.to_str().unwrap()], false)
    }
}
fn minimal_manifest() -> Value {
    json!({"schema_version":1,"compilations":[{"id":{"project":"app","source_set":"main"},"source_roots":["src"],"target_release":17}]})
}

#[test]
fn prepared_batch_preserves_order_and_owner_context_without_rebuilding() {
    let f = Fixture::new();
    let text = "class A { int count; void run() { count++; } }";
    f.write("src/A.java", text);
    f.manifest();
    let q = query("src/./A.java", text, "count", "app", "main");
    let result = f.batch(vec![q; 8], true);
    assert_eq!(result["result"]["index_builds"], 1);
    let results = result["result"]["queries"].as_array().unwrap();
    assert_eq!(results.len(), 8);
    for (index, item) in results.iter().enumerate() {
        assert_eq!(item["query_index"], index);
        assert_eq!(item["targets"], results[0]["targets"]);
        assert_eq!(item["source_compilation"]["source_set"], "main");
    }
    assert_eq!(result["capabilities"]["method_argument_applicability"], false);
    assert_eq!(result["capabilities"]["persistent_server_reload_overlays"], false);
}

#[test]
fn source_sets_and_modules_are_explicit_and_invisible_paths_are_errors() {
    let f = Fixture::new();
    let app = "package app; class App extends lib.Lib { void run() { value++; } Test hidden; Other absent; }";
    let test = "package app; class Test extends App { void run() { value++; } }";
    let lib = "package lib; public class Lib { public int value; }";
    f.write("app/main/App.java", app);
    f.write("app/test/Test.java", test);
    f.write("lib/main/Lib.java", lib);
    f.write("other/main/Other.java", "package app; class Other {}");
    let manifest = json!({"schema_version":1,"compilations":[
        {"id":{"project":"app","source_set":"main"},"source_roots":["app/main"],"dependencies":[{"project":"lib","source_set":"main"}]},
        {"id":{"project":"app","source_set":"test"},"source_roots":["app/test"],"dependencies":[{"project":"app","source_set":"main"}]},
        {"id":{"project":"lib","source_set":"main"},"source_roots":["lib/main"]},
        {"id":{"project":"other","source_set":"main"},"source_roots":["other/main"]}
    ]});
    f.write("project.json", manifest.to_string());
    let result = f.batch(
        vec![
            query("app/main/App.java", app, "value", "app", "main"),
            query("app/test/Test.java", test, "value", "app", "test"),
            query("app/main/App.java", app, "Test", "app", "main"),
            query("app/main/App.java", app, "Other", "app", "main"),
            query("lib/main/Lib.java", lib, "value", "app", "test"),
        ],
        true,
    );
    let results = result["result"]["queries"].as_array().unwrap();
    for i in [0, 1, 4] {
        assert_eq!(results[i]["status"], "Resolved");
        assert_eq!(results[i]["targets"][0]["compilation"]["project"], "lib");
    }
    for i in [2, 3] {
        assert_eq!(results[i]["status"], "Unresolved");
    }
    assert_eq!(results[4]["source_compilation"]["project"], "lib");
    for (file, text, project, set) in [
        ("app/test/Test.java", test, "app", "main"),
        ("app/main/App.java", app, "lib", "main"),
        ("other/main/Other.java", "Other", "app", "main"),
    ] {
        let q = if text == "Other" {
            query(file, text, "Other", project, set)
        } else {
            query(file, text, "value", project, set)
        };
        assert_eq!(f.batch(vec![q], false)["error"]["code"], "invisible_source");
    }
}

#[test]
fn utf8_utf16_astral_crlf_positions_are_explicit() {
    let f = Fixture::new();
    let text = "// 😀\r\nclass A { int café; void run() { café++; } }\r\n";
    f.write("src/A.java", text);
    f.manifest();
    let byte = text.rfind("café").unwrap();
    let mut utf16 = query("src/A.java", text, "café", "app", "main");
    utf16["position"] = json!({"encoding":"utf16","offset":text[..byte].encode_utf16().count()});
    let result = f.batch(vec![query("src/A.java", text, "café", "app", "main"), utf16], true);
    let queries = &result["result"]["queries"];
    assert_eq!(queries[0]["targets"], queries[1]["targets"]);
    assert_eq!(queries[1]["byte_offset"], byte);
    let target = &queries[0]["targets"][0];
    let start = text.find("café").unwrap();
    assert_eq!(target["range"]["start_byte"], start);
    assert_eq!(target["range"]["start_utf16"], text[..start].encode_utf16().count());
    for (encoding, offset) in [
        ("utf8", 4),
        ("utf16", 4),
        ("utf8", text.len()),
        ("utf16", text.encode_utf16().count()),
        ("utf8", u32::MAX as usize),
    ] {
        let mut q = query("src/A.java", text, "café", "app", "main");
        q["position"] = json!({"encoding":encoding,"offset":offset});
        assert_eq!(f.batch(vec![q], false)["error"]["code"], "invalid_position");
    }
}

#[test]
fn invalid_requests_fail_before_loading_and_never_return_partial_batch() {
    let f = Fixture::new();
    let text = "class A { int n; void f() { n++; } }";
    f.write("src/A.java", text);
    f.manifest();
    let valid = query("src/A.java", text, "n++", "app", "main");
    let mut bad = valid.clone();
    bad["position"]["offset"] = json!(9999);
    let response = f.batch(vec![valid.clone(), bad], false);
    assert_eq!(response["error"]["query_index"], 1);
    assert!(response.get("result").is_none());
    // Malformed/oversized query documents fail before a nonexistent manifest is opened.
    let path = f.write("invalid-queries.json", r#"{"schema_version":1,"queries":[],"typo":1}"#);
    let response = f.run(
        &["declaration", "--manifest", "/definitely/missing/project.json", "--requests", path.to_str().unwrap()],
        false,
    );
    assert_eq!(response["error"]["code"], "invalid_json");
    for queries in [vec![], vec![valid; 1025]] {
        assert_eq!(f.batch(queries, false)["error"]["code"], "request_limit");
    }
}

#[test]
fn malformed_unknown_duplicate_and_invalid_configuration_is_rejected() {
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    for document in [
        "{",
        r#"{"schema_version":1,"schema_version":1,"compilations":[]}"#,
        r#"{"schema_version":1,"compilations":[{"id":{"project":"a","source_set":"main","typo":0},"source_roots":[]}]}"#,
    ] {
        let path = f.write("bad.json", document);
        assert_eq!(f.run(&["inspect", "--manifest", path.to_str().unwrap()], false)["error"]["code"], "invalid_json");
    }
    for (pointer, value, code) in [
        ("/schema_version", json!(2), "unsupported_version"),
        ("/compilations/0/id/project", json!("bad id"), "invalid_configuration"),
        ("/compilations/0/source_roots/0", json!(""), "invalid_path"),
        ("/compilations/0/target_release", json!(0), "invalid_configuration"),
    ] {
        let mut manifest = minimal_manifest();
        *manifest.pointer_mut(pointer).unwrap() = value;
        assert_eq!(f.inspect_error(manifest)["error"]["code"], code);
    }
    for limits in [
        json!({"max_file_bytes":0}),
        json!({"max_file_bytes":4294967296_u64}),
        json!({"max_input_bytes":-1}),
        json!({"unknown":1}),
    ] {
        let mut manifest = minimal_manifest();
        manifest["compilations"][0]["limits"] = limits;
        let response = f.inspect_error(manifest);
        assert!(response["error"]["message"].is_string());
    }
    for limits in [json!({"max_compilations":129}), json!({"max_dependency_edges":1025})] {
        let mut manifest = minimal_manifest();
        manifest["limits"] = limits;
        assert_eq!(f.inspect_error(manifest)["error"]["code"], "invalid_configuration");
    }
}

#[test]
fn load_failures_name_paths_and_graph_failures_do_not_discover_dependencies() {
    let f = Fixture::new();
    f.write("src/A.java", "class A { String absent; }");
    let manifest = f.manifest();
    let result = f.declaration("src/A.java", 10);
    assert_eq!(result["result"]["queries"][0]["status"], "Unresolved");
    let mut value = minimal_manifest();
    value["compilations"][0]["classpath"] = json!([{"kind":"jar","path":"missing.jar"}]);
    let error = f.inspect_error(value);
    assert_eq!(error["error"]["code"], "load_failed");
    assert!(error["error"]["path"].as_str().unwrap().ends_with("missing.jar"));
    let mut value = minimal_manifest();
    value["compilations"][0]["dependencies"] = json!([{"project":"missing","source_set":"main"}]);
    assert!(f.inspect_error(value)["error"]["message"].as_str().unwrap().contains("UnknownDependency"));
    f.write("src/A.java", [0xff, 0xfe]);
    let error = f.run(&["inspect", "--manifest", manifest.to_str().unwrap()], false);
    assert!(error["error"]["message"].as_str().unwrap().contains("InvalidUtf8"));
    assert!(error["error"]["path"].as_str().unwrap().ends_with("src/A.java"));
}

#[test]
fn binary_candidates_preserve_origin_without_fake_source_range() {
    let f = Fixture::new();
    let text = "class A extends lib.Lib { void use() { run(); } }";
    f.write("src/A.java", text);
    f.write("deps/library.jar", include_bytes!("fixtures/project/deps/library.jar"));
    let mut manifest = minimal_manifest();
    manifest["compilations"][0]["classpath"] = json!([{"kind":"jar","path":"deps/library.jar"}]);
    f.write("project.json", manifest.to_string());
    let result = f.declaration("src/A.java", text.rfind("run").unwrap());
    let result = &result["result"]["queries"][0];
    assert_eq!(result["status"], "Resolved");
    let target = &result["targets"][0];
    assert_eq!(target["origin"], "binary");
    assert_eq!(target["source_available"], false);
    assert_eq!(target["binary_name"], "lib/Lib");
    assert!(target["path"].as_str().unwrap().ends_with("deps/library.jar!/lib/Lib.class"));
    assert!(target.get("range").is_none());
    assert!(target.get("start_byte").is_none());
    assert!(target["linking_context"].is_null());
    assert_eq!(target["loaded_contexts"][0]["project"], "app");
}

#[test]
fn overloaded_ambiguous_and_package_results_are_not_fabricated_declarations() {
    let f = Fixture::new();
    let text = "class A { void m() {} void m(int n) {} void use() { m(1); lib.deep.Deep.f(); } }";
    f.write("src/A.java", text);
    f.write("src/Deep.java", "package lib.deep; public class Deep { public static void f() {} }");
    f.write("src/B1.java", "class B {}");
    f.write("src/B2.java", "class B {}");
    let ambiguous = "class Use { B b; }";
    f.write("src/Use.java", ambiguous);
    f.manifest();
    let result = f.batch(
        vec![
            query("src/A.java", text, "m(1)", "app", "main"),
            query("src/Use.java", ambiguous, "B b", "app", "main"),
            query("src/A.java", text, "deep.Deep", "app", "main"),
        ],
        true,
    );
    let q = &result["result"]["queries"];
    assert_eq!(q[0]["status"], "OverloadNotResolved");
    assert_eq!(q[0]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(q[1]["status"], "Ambiguous");
    assert_eq!(q[1]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(q[2]["status"], "Package");
    assert_eq!(q[2]["targets"][0]["origin"], "package");
    assert!(q[2]["targets"][0].get("range").is_none());
    assert!(q[2]["targets"][0].get("path").is_none());
}

#[test]
fn parser_reports_never_claim_semantic_errors_and_filter_owned_sources() {
    let f = Fixture::new();
    f.write("src/A.java", "class A { void f(int n) { switch(n) { default: break; } } }");
    let manifest = f.manifest();
    let output = f.run(
        &["parser-report", "--manifest", manifest.to_str().unwrap(), "--project", "app", "--source-set", "main"],
        true,
    );
    let result = &output["result"];
    assert_eq!(result["classification"], "parser_limit_or_invalid_syntax");
    assert_eq!(result["java_semantic_diagnostics"], false);
    assert_eq!(result["source_count"], 1);
    assert_eq!(result["files_with_reports"], 1);
    assert!(result["report_count"].as_u64().unwrap() > 0);
    assert_eq!(result["scope"], "owned_sources_only");
    for report in result["files"][0]["reports"].as_array().unwrap() {
        assert_eq!(report["classification"], "parser_limit_or_invalid_syntax");
        assert!(report["range"]["start_utf16"].is_number());
    }
}

#[test]
fn document_and_response_limits_are_errors_not_partial_json() {
    let f = Fixture::new();
    let oversized = f.write("big.json", vec![b' '; 1024 * 1024 + 1]);
    assert_eq!(
        f.run(&["inspect", "--manifest", oversized.to_str().unwrap()], false)["error"]["code"],
        "document_limit"
    );
    let invalid_utf8 = f.write("utf8.json", [0xff]);
    assert_eq!(
        f.run(&["inspect", "--manifest", invalid_utf8.to_str().unwrap()], false)["error"]["code"],
        "invalid_json"
    );
    // Error-node inventory is bounded even when source input bytes are within limits.
    f.write("src/A.java", format!("class A {{ void f() {{ {} }} }}", "@ ".repeat(11000)));
    let manifest = f.manifest();
    assert_eq!(
        f.run(&["parser-report", "--manifest", manifest.to_str().unwrap()], false)["error"]["code"],
        "output_limit"
    );
}

#[test]
fn help_usage_and_missing_optional_values_have_machine_outputs() {
    let f = Fixture::new();
    assert_eq!(f.run(&["--help"], true)["capabilities"]["experimental"], true);
    assert!(f.run(&["--version"], true)["version"].is_string());
    for args in [
        vec![],
        vec!["unknown"],
        vec!["inspect", "--manifest"],
        vec!["inspect", "--bogus", "x"],
        vec!["inspect", "--manifest", "x", "--manifest", "x"],
        vec!["parser-report", "--manifest", "x", "--project", "p"],
    ] {
        assert_eq!(f.run(&args, false)["error"]["code"], "usage");
    }
}

#[test]
fn partial_archive_limits_keep_jmod_defaults_and_core_limits_can_be_lowered() {
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    let mut value = minimal_manifest();
    value["compilations"][0]["jmod_limits"] = json!({"max_entries":23});
    let path = f.write("project.json", value.to_string());
    let result = f.run(&["inspect", "--manifest", path.to_str().unwrap()], true);
    assert_eq!(
        result["result"]["compilations"][0]["configuration"]["jmod_limits"]["max_entry_bytes"],
        64 * 1024 * 1024
    );
    value["compilations"][0]["limits"] = json!({"max_file_bytes":1});
    assert!(f.inspect_error(value)["error"]["message"].as_str().unwrap().contains("LimitExceeded"));
}

#[cfg(unix)]
#[test]
fn source_and_document_symlinks_are_rejected_but_query_aliases_are_canonical() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let text = "class A { int n; void f() { n++; } }";
    let source = f.write("src/A.java", text);
    let manifest = f.manifest();
    symlink(&source, f.0.join("alias.java")).unwrap();
    assert_eq!(
        f.declaration("alias.java", text.rfind("n++").unwrap())["result"]["queries"][0]["file"],
        source.to_str().unwrap()
    );
    symlink(&manifest, f.0.join("alias.json")).unwrap();
    assert_eq!(
        f.run(&["inspect", "--manifest", f.0.join("alias.json").to_str().unwrap()], false)["error"]["code"],
        "invalid_path"
    );
    symlink(&source, f.0.join("src/Alias.java")).unwrap();
    assert!(
        f.run(&["inspect", "--manifest", manifest.to_str().unwrap()], false)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("symlink")
    );
}

#[test]
fn shipped_example_runs_as_a_real_subprocess_without_java_environment() {
    let f = Fixture::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/cli_project");
    let output = f.run(
        &[
            "declaration",
            "--manifest",
            root.join("project.json").to_str().unwrap(),
            "--requests",
            root.join("requests.json").to_str().unwrap(),
        ],
        true,
    );
    let q = &output["result"]["queries"];
    assert_eq!(q[0]["status"], "Resolved");
    assert_eq!(q[1]["status"], "Resolved");
    assert_eq!(q[2]["status"], "OverloadNotResolved");
    assert_eq!(q[0]["targets"][0]["name"], "value");
    assert_eq!(q[0]["targets"][0]["compilation"]["project"], "lib");
}

#[test]
fn inspect_summary_handles_shared_platforms_above_detailed_output_cap() {
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};
    let f = Fixture::new();
    // 101 contexts x 100 native fixture classes = 10100 binary instances.
    // This stays a bounded test without generating classes through javac.
    let template = include_bytes!("fixtures/classes/Lib.class");
    let offset = template.windows(7).position(|w| w == b"lib/Lib").unwrap();
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..100 {
        let name = format!("lib/L{index:02}");
        let mut class = template.to_vec();
        class[offset..offset + 7].copy_from_slice(name.as_bytes());
        archive.start_file(format!("{name}.class"), SimpleFileOptions::default()).unwrap();
        archive.write_all(&class).unwrap();
    }
    f.write("platform.jar", archive.finish().unwrap().into_inner());
    let compilations: Vec<_> = (0..101)
        .map(|index| {
            json!({"id":{"project":format!("p{index:03}"),"source_set":"main"},
        "source_roots":[],"platform":[{"kind":"jar","path":"platform.jar"}]})
        })
        .collect();
    let manifest = f.write("project.json", json!({"schema_version":1,"compilations":compilations}).to_string());
    let result = f.run(&["inspect", "--manifest", manifest.to_str().unwrap()], true);
    assert_eq!(result["result"]["detail_mode"], "summary");
    assert_eq!(result["result"]["compilations"].as_array().unwrap().len(), 101);
    for compilation in result["result"]["compilations"].as_array().unwrap() {
        assert_eq!(compilation["class_count"], 100);
        assert!(compilation.get("class_origins").is_none());
    }
    let error = f.run(&["inspect", "--manifest", manifest.to_str().unwrap(), "--details"], false);
    assert_eq!(error["error"]["code"], "output_limit");
    f.write("project.json", json!({"schema_version":1,"compilations":[compilations[0].clone()]}).to_string());
    let result = f.run(&["inspect", "--details", "--manifest", manifest.to_str().unwrap()], true);
    assert_eq!(result["result"]["detail_mode"], "full");
    assert_eq!(result["result"]["compilations"][0]["class_count"], 100);
    assert_eq!(result["result"]["compilations"][0]["class_origins"].as_array().unwrap().len(), 100);
    for args in [
        vec!["declaration", "--details"],
        vec!["parser-report", "--details"],
        vec!["inspect", "--details", "--details"],
        vec!["inspect", "--details", "yes"],
    ] {
        assert_eq!(f.run(&args, false)["error"]["code"], "usage");
    }
}

#[test]
fn execution_options_are_strict_and_capabilities_describe_real_bounds() {
    let f = Fixture::new();
    for value in ["0", "300001", "-1", "+1", "1.5", "", " 5", "5 ", "999999999999999999999"] {
        let result = f.run(&["--help", "--timeout-ms", value], false);
        assert_eq!(result["error"]["code"], "usage", "{value}");
    }
    for args in [
        vec!["--help", "--timeout-ms"],
        vec!["--help", "--timeout-ms", "5", "--timeout-ms", "5"],
        vec!["--help", "--cancel-on-stdin-close", "--cancel-on-stdin-close"],
    ] {
        assert_eq!(f.run(&args, false)["error"]["code"], "usage");
    }
    let result = f.run(&["--help", "--timeout-ms", "300000"], true);
    assert_eq!(result["capabilities"]["process_timeout_isolation"], true);
    assert_eq!(result["capabilities"]["execution"]["default_timeout_ms"], 30000);
    assert_eq!(result["capabilities"]["execution"]["memory_limit"], false);
    assert_eq!(result["capabilities"]["execution"]["final_output_backpressure_bounded"], false);
    // A data value is not accidentally swallowed as a supervisor flag.
    assert_eq!(f.run(&["inspect", "--manifest", "--timeout-ms"], false)["error"]["code"], "io");
}

#[test]
fn actual_project_timeout_and_stdin_cancellation_return_one_json_error() {
    let f = Fixture::new();
    f.write("src/Large.java", format!("class Large {{ {} }}", "int field;".repeat(30000)));
    let manifest = f.manifest();
    let started = std::time::Instant::now();
    let result = f.run(&["inspect", "--manifest", manifest.to_str().unwrap(), "--timeout-ms", "1"], false);
    assert_eq!(result["error"]["code"], "execution_timeout");
    assert_eq!(result["error"]["worker_reaped"], true);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    // Command::output closes stdin. Cancellation is opt-in; other fixture runs use the same closed stdin.
    let result = f.run(&["inspect", "--manifest", manifest.to_str().unwrap(), "--cancel-on-stdin-close"], false);
    assert_eq!(result["error"]["code"], "execution_cancelled");
    assert_eq!(result["error"]["worker_reaped"], true);
}

#[test]
fn deeply_recursive_project_cannot_abort_supervisor_or_leak_panic_text() {
    let f = Fixture::new();
    f.write(
        "src/Deep.java",
        format!("class Deep {{ void run() {{ {}1{}; }} }}", "(".repeat(100000), ")".repeat(100000)),
    );
    let manifest = f.manifest();
    let result = f.run(&["parser-report", "--manifest", manifest.to_str().unwrap(), "--timeout-ms", "5000"], false);
    assert_eq!(result["error"]["code"], "worker_failed");
    assert_eq!(result["error"]["worker_reaped"], true);
    #[cfg(target_os = "linux")]
    assert!(
        result["error"]["message"].as_str().unwrap().contains("signal: 6"),
        "expected actual SIGABRT from worker stack exhaustion"
    );
}

#[test]
fn internal_worker_spelling_remains_an_opaque_data_value() {
    let f = Fixture::new();
    let id = "__java_analyzer_worker_v1";
    let source = "class A { int field; void run() { field++; } }";
    f.write("src/A.java", source);
    let manifest = f.write(
        id,
        json!({"schema_version":1,"compilations":[{"id":{"project":id,"source_set":id},"source_roots":["src"]}]})
            .to_string(),
    );
    let result = f.run(
        &[
            "declaration",
            "--manifest",
            manifest.to_str().unwrap(),
            "--project",
            id,
            "--source-set",
            id,
            "--file",
            "src/A.java",
            "--byte",
            &source.rfind("field").unwrap().to_string(),
        ],
        true,
    );
    assert_eq!(result["result"]["queries"][0]["status"], "Resolved");
    assert_eq!(result["result"]["queries"][0]["targets"][0]["name"], "field");
}

#[test]
fn unique_source_contradictions_are_refused_without_platform_data() {
    for text in [
        "class A { static void m() {} static void use() { m(1); } }",
        "class A { static void m(int x) {} static void use(boolean x) { m(x); } }",
        "class A { static void m(float x) {} static void use(double x) { m(x); } }",
    ] {
        let f = Fixture::new();
        f.write("src/A.java", text);
        f.manifest();
        let response = f.declaration("src/A.java", text.rfind("m(").unwrap());
        let query = &response["result"]["queries"][0];
        assert_eq!(query["status"], "CallNotResolved");
        assert_eq!(query["targets"].as_array().unwrap().len(), 1);
        let target = &query["targets"][0];
        assert_eq!(target["origin"], "source");
        assert_eq!(target["range"]["start_byte"], text.find("m(").unwrap());
        assert_eq!(target["range"]["end_byte"], text.find("m(").unwrap() + 1);
        assert_eq!(response["capabilities"]["method_argument_applicability"], false);
        assert_eq!(response["capabilities"]["limited_call_selection"]["source_primitive_strict_phase"], true);
        assert_eq!(response["capabilities"]["limited_call_selection"]["single_source_contradiction_guard"], true);
        assert_eq!(response["capabilities"]["limited_call_selection"]["general_applicability"], false);
    }
}

#[test]
fn proven_nested_call_result_survives_cli_and_fresh_source_load() {
    let f = Fixture::new();
    // A minimal Object signature, assembled as JVMS bytes without Java tools.
    let mut object = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 3, 1, 0, 16];
    object.extend_from_slice(b"java/lang/Object");
    object.extend_from_slice(&[7, 0, 1, 0, 0x21, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    f.write("platform/java/lang/Object.class", object);
    let text = "class A { static float inner(float x) { return x; } static double inner(double x) { return x; } static void outer(float x) {} static void outer(double x) {} static void use(float x) { outer(inner(x)); } }";
    f.write("src/A.java", text);
    let mut manifest: Value = serde_json::from_slice(&fs::read(f.manifest()).unwrap()).unwrap();
    manifest["compilations"][0]["classpath"] = json!([{"kind":"directory","path":"platform"}]);
    f.write("project.json", manifest.to_string());
    let initial = f.declaration("src/A.java", text.rfind("outer(").unwrap());
    let query = &initial["result"]["queries"][0];
    assert_eq!(query["status"], "Resolved");
    assert_eq!(query["targets"].as_array().unwrap().len(), 1);
    assert_eq!(query["targets"][0]["range"]["start_byte"], text.find("outer(float").unwrap());
    assert_eq!(initial["capabilities"]["limited_call_selection"]["primitive_source_call_returns"], true);
    assert_eq!(initial["capabilities"]["method_argument_applicability"], false);

    // Same inner parameter signature, changed declared return: outer must change.
    let changed = text.replace("float inner(float", "double inner(float");
    f.write("src/A.java", &changed);
    let after = f.declaration("src/A.java", changed.rfind("outer(").unwrap());
    assert_eq!(after["result"]["queries"][0]["status"], "Resolved");
    assert_eq!(
        after["result"]["queries"][0]["targets"][0]["range"]["start_byte"],
        changed.find("outer(double").unwrap()
    );
    assert_eq!(query["targets"][0]["range"]["start_byte"], text.find("outer(float").unwrap());

    manifest["compilations"][0]["classpath"] = json!([]);
    f.write("project.json", manifest.to_string());
    let omitted = f.declaration("src/A.java", changed.rfind("outer(").unwrap());
    assert_eq!(omitted["result"]["queries"][0]["status"], "OverloadNotResolved");
    assert_eq!(omitted["result"]["queries"][0]["targets"].as_array().unwrap().len(), 2);
}
