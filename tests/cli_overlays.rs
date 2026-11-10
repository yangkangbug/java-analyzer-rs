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
        let f = Self(std::env::temp_dir().join(format!(
            "java-analyzer-overlay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir_all(&f.0).unwrap();
        f.write("project.json", json!({"schema_version":1,"compilations":[{"id":{"project":"app","source_set":"main"},"source_roots":["src"]}]}).to_string());
        f.write("src/A.java", "class A { int old; void run() { old++; } }");
        f
    }
    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn run(&self, command: &str, args: &[&str], success: bool) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_java-analyzer"))
            .args([command, "--manifest", self.0.join("project.json").to_str().unwrap()])
            .args(args)
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
        let data = if success {
            assert!(out.stderr.is_empty());
            out.stdout
        } else {
            assert!(out.stdout.is_empty());
            out.stderr
        };
        serde_json::from_slice(&data).unwrap()
    }
    fn overlay(&self, text: &str) -> PathBuf {
        self.write("input/overlays.json", json!({"schema_version":1,"revision":7,"files":[{"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","version":11,"text":text}]}).to_string())
    }
    fn requests(&self, position: Value) -> PathBuf {
        self.write("input/requests.json", json!({"schema_version":1,"queries":[{"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","position":position}]}).to_string())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn unsaved_full_text_and_utf16_line_query_are_supervised_without_java() {
    let f = Fixture::new();
    let text = "// 😀\r\nclass A { int fresh;\r\n  void run() { fresh++; }\r\n}\r\n";
    let overlays = f.overlay(text);
    let out = f.run(
        "declaration",
        &[
            "--overlays",
            overlays.to_str().unwrap(),
            "--project",
            "app",
            "--source-set",
            "main",
            "--file",
            "src/A.java",
            "--line",
            "2",
            "--character",
            "15",
        ],
        true,
    );
    let q = &out["result"]["queries"][0];
    assert_eq!(q["status"], "Resolved");
    assert_eq!(q["position"], json!({"encoding":"utf16_line","line":2,"character":15}));
    assert_eq!(q["targets"][0]["name"], "fresh");
    assert_eq!(q["targets"][0]["range"]["start_byte"], text.find("fresh").unwrap());
    assert_eq!(
        out["result"]["overlay_state"],
        json!({"revision":7,"files":[{"compilation":{"project":"app","source_set":"main"},"file":f.0.join("src/A.java").canonicalize().unwrap(),"version":11}]})
    );
    assert_eq!(fs::read_to_string(f.0.join("src/A.java")).unwrap(), "class A { int old; void run() { old++; } }");
    let requests = f.requests(json!({"encoding":"utf16_line","line":2,"character":15}));
    let batch = f.run(
        "declaration",
        &["--overlays", overlays.to_str().unwrap(), "--requests", requests.to_str().unwrap()],
        true,
    );
    assert_eq!(batch["result"]["queries"], out["result"]["queries"]);
}

#[test]
fn line_positions_reject_surrogates_crlf_interior_and_eof_without_clamping() {
    let f = Fixture::new();
    let overlays = f.overlay("// 😀\r\nclass A {}\r\n");
    for position in [
        json!({"encoding":"utf16_line","line":0,"character":4}),
        json!({"encoding":"utf16_line","line":0,"character":6}),
        json!({"encoding":"utf16_line","line":1,"character":11}),
        json!({"encoding":"utf16_line","line":2,"character":0}),
        json!({"encoding":"utf16_line","line":3,"character":0}),
    ] {
        let requests = f.requests(position);
        let out = f.run(
            "declaration",
            &["--overlays", overlays.to_str().unwrap(), "--requests", requests.to_str().unwrap()],
            false,
        );
        assert_eq!(out["error"]["code"], "invalid_position");
        assert_eq!(out["error"]["query_index"], 0);
    }
}

#[test]
fn malformed_unsaved_text_reports_parser_gaps_and_legacy_ranges_stay_exact() {
    let f = Fixture::new();
    let overlays = f.overlay("class A { void broken( {");
    let out = f.run("parser-report", &["--overlays", overlays.to_str().unwrap()], true);
    assert!(out["result"]["report_count"].as_u64().unwrap() > 0);
    assert_eq!(out["result"]["overlay_state"]["revision"], 7);
    let legacy = f.run(
        "declaration",
        &["--project", "app", "--source-set", "main", "--file", "src/A.java", "--byte", "32"],
        true,
    );
    assert!(legacy["result"].get("overlay_state").is_none());
    assert_eq!(
        legacy["result"]["queries"][0]["targets"][0]["range"],
        json!({"start_byte":14,"end_byte":17,"start_utf16":14,"end_utf16":17})
    );
}

#[test]
fn overlay_shape_labels_and_canonical_duplicates_are_strict() {
    let f = Fixture::new();
    let base = json!({"schema_version":1,"revision":1,"files":[{"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","version":2,"text":"class A {}"}]});
    for (pointer, value) in [
        ("/revision", json!(-1)),
        ("/revision", json!(9007199254740992u64)),
        ("/files/0/version", json!(1.5)),
        ("/files/0/version", json!("1")),
        ("/files/0/file", json!("")),
        ("/files/0/file", json!("a\u{0}b")),
    ] {
        let mut input = base.clone();
        *input.pointer_mut(pointer).unwrap() = value;
        let path = f.write("bad.json", input.to_string());
        let out = f.run("inspect", &["--overlays", path.to_str().unwrap()], false);
        assert!(
            matches!(out["error"]["code"].as_str(), Some("invalid_json" | "invalid_overlay" | "invalid_path")),
            "{out}"
        );
    }
    let mut input = base.clone();
    let mut alias = input["files"][0].clone();
    alias["file"] = json!("src/./A.java");
    input["files"].as_array_mut().unwrap().push(alias);
    let path = f.write("duplicate.json", input.to_string());
    assert_eq!(f.run("inspect", &["--overlays", path.to_str().unwrap()], false)["error"]["code"], "invalid_overlay");
    for raw in [
        "{\"schema_version\":1,\"revision\":1,\"revision\":2,\"files\":[]}",
        "{\"schema_version\":1,\"revision\":1,\"files\":[],\"extra\":true}",
    ] {
        let path = f.write("strict.json", raw);
        assert_eq!(f.run("inspect", &["--overlays", path.to_str().unwrap()], false)["error"]["code"], "invalid_json");
    }
    let path = f.write("empty.json", "{\"schema_version\":1,\"revision\":0,\"files\":[]}");
    assert_eq!(
        f.run("inspect", &["--overlays", path.to_str().unwrap()], true)["result"]["overlay_state"],
        json!({"revision":0,"files":[]})
    );
}

#[test]
fn simultaneous_library_and_application_buffers_preserve_visibility_and_owner_ranges() {
    let f = Fixture::new();
    f.write("project.json", json!({"schema_version":1,"compilations":[
        {"id":{"project":"app","source_set":"main"},"source_roots":["src"],"dependencies":[{"project":"lib","source_set":"main"}]},
        {"id":{"project":"app","source_set":"test"},"source_roots":["test"],"dependencies":[{"project":"app","source_set":"main"}]},
        {"id":{"project":"lib","source_set":"main"},"source_roots":["lib"]}
    ]}).to_string());
    f.write("src/A.java", "class A extends Lib { void run() { value++; } }");
    f.write("lib/Lib.java", "public class Lib { public int value; }");
    f.write("test/Test.java", "class Test extends A { void run() { value++; } }");
    let app = "// 😀\r\nclass A extends Lib { void run() { editedValue++; } }";
    let lib = "// 😀\r\npublic class Lib { public int editedValue; }";
    let files = json!([
        {"compilation":{"project":"lib","source_set":"main"},"file":"lib/Lib.java","version":2,"text":lib},
        {"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","version":3,"text":app}
    ]);
    let overlays = f.write("overlays.json", json!({"schema_version":1,"revision":23,"files":files}).to_string());
    let queries = ["main", "test"].map(|set| json!({"compilation":{"project":"app","source_set":set},"file":"src/A.java","position":{"encoding":"utf16_line","line":1,"character":35}}));
    let requests = f.write("requests.json", json!({"schema_version":1,"queries":queries}).to_string());
    let out = f.run(
        "declaration",
        &["--overlays", overlays.to_str().unwrap(), "--requests", requests.to_str().unwrap()],
        true,
    );
    assert_eq!(out["result"]["index_builds"], 1);
    for q in out["result"]["queries"].as_array().unwrap() {
        assert_eq!(q["status"], "Resolved");
        assert_eq!(q["source_compilation"]["source_set"], "main");
        assert_eq!(q["targets"][0]["compilation"]["project"], "lib");
        let start = lib.find("editedValue").unwrap();
        assert_eq!(
            q["targets"][0]["range"],
            json!({"start_byte":start,"end_byte":start+11,"start_utf16":lib[..start].encode_utf16().count(),"end_utf16":lib[..start+11].encode_utf16().count()})
        );
    }
    f.write("overlays.json", json!({"schema_version":1,"revision":24,"files":[files[1].clone()]}).to_string());
    let partial = f.run(
        "declaration",
        &["--overlays", overlays.to_str().unwrap(), "--requests", requests.to_str().unwrap()],
        true,
    );
    assert_ne!(partial["result"]["queries"][0]["status"], "Resolved");
    let reverse = f.write("requests.json", json!({"schema_version":1,"queries":[queries[0].clone(),{"compilation":{"project":"app","source_set":"main"},"file":"test/Test.java","position":{"encoding":"utf8","offset":6}}]}).to_string());
    let out = f.run(
        "declaration",
        &["--overlays", overlays.to_str().unwrap(), "--requests", reverse.to_str().unwrap()],
        false,
    );
    assert_eq!(out["error"]["code"], "invisible_source");
    assert_eq!(out["error"]["query_index"], 1);
    let mut wrong_owner = files[0].clone();
    wrong_owner["compilation"]["project"] = json!("app");
    f.write(
        "overlays.json",
        json!({"schema_version":1,"revision":25,"files":[files[1].clone(),wrong_owner]}).to_string(),
    );
    assert_eq!(
        f.run(
            "declaration",
            &["--overlays", overlays.to_str().unwrap(), "--requests", requests.to_str().unwrap()],
            false
        )["error"]["code"],
        "invalid_overlay"
    );
    assert_eq!(fs::read_to_string(f.0.join("lib/Lib.java")).unwrap(), "public class Lib { public int value; }");
}

#[test]
fn position_shapes_and_single_flags_reject_mixed_duplicate_and_overflow_values() {
    let f = Fixture::new();
    for p in [
        json!({"encoding":"utf8","offset":6,"line":0}),
        json!({"encoding":"utf16_line","line":0,"character":6,"offset":6}),
        json!({"encoding":"utf16_line","line":0}),
        json!({"encoding":"utf16_line","line":4294967296u64,"character":0}),
        json!({"encoding":"utf16_line","line":0,"character":4294967296u64}),
        json!({"encoding":"utf16_line","line":-1,"character":0}),
        json!({"encoding":"utf16_line","line":0,"character":1.5}),
    ] {
        let requests = f.requests(p);
        assert_eq!(
            f.run("declaration", &["--requests", requests.to_str().unwrap()], false)["error"]["code"],
            "invalid_json"
        );
    }
    for position in [
        vec!["--line", "0"],
        vec!["--character", "6"],
        vec!["--line", "0", "--character", "6", "--byte", "6"],
        vec!["--line", "0", "--line", "0", "--character", "6"],
    ] {
        let mut args = vec!["--project", "app", "--source-set", "main", "--file", "src/A.java"];
        args.extend(position);
        assert_eq!(f.run("declaration", &args, false)["error"]["code"], "usage");
    }
    let out = f.run(
        "declaration",
        &["--project", "app", "--source-set", "main", "--file", "src/A.java", "--line", "0", "--character", "32"],
        true,
    );
    assert_eq!(out["result"]["queries"][0]["targets"][0]["name"], "old");
    assert!(out["result"].get("overlay_state").is_none());
    for raw in [
        "{\"encoding\":\"utf16_line\",\"line\":0,\"line\":1,\"character\":6}",
        "{\"encoding\":\"utf8\",\"encoding\":\"utf16\",\"offset\":6}",
    ] {
        let requests = f.write("duplicate-position.json", format!("{{\"schema_version\":1,\"queries\":[{{\"compilation\":{{\"project\":\"app\",\"source_set\":\"main\"}},\"file\":\"src/A.java\",\"position\":{raw}}}]}}"));
        assert_eq!(
            f.run("declaration", &["--requests", requests.to_str().unwrap()], false)["error"]["code"],
            "invalid_json"
        );
    }
}

#[test]
fn overlay_document_count_and_original_source_budgets_remain_bounded() {
    let f = Fixture::new();
    let mut document = "{\"schema_version\":1,\"revision\":0,\"files\":[]}".to_owned();
    document.extend(std::iter::repeat_n(' ', 1024 * 1024 - document.len()));
    let path = f.write("boundary.json", &document);
    f.run("inspect", &["--overlays", path.to_str().unwrap()], true);
    document.push(' ');
    f.write("boundary.json", document);
    assert_eq!(f.run("inspect", &["--overlays", path.to_str().unwrap()], false)["error"]["code"], "document_limit");
    let entry = json!({"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","version":0,"text":""});
    let path = f.write("many.json", json!({"schema_version":1,"revision":0,"files":vec![entry;1025]}).to_string());
    assert_eq!(f.run("inspect", &["--overlays", path.to_str().unwrap()], false)["error"]["code"], "overlay_limit");
    let manifest = json!({"schema_version":1,"compilations":[{"id":{"project":"app","source_set":"main"},"source_roots":["src"],"limits":{"max_file_bytes":64}}]});
    f.write("project.json", manifest.to_string());
    let path = f.overlay(&format!("class A {{ /* {} */ }}", "a".repeat(100)));
    assert_eq!(f.run("inspect", &["--overlays", path.to_str().unwrap()], false)["error"]["code"], "overlay_limit");
}

#[test]
fn overlays_cannot_create_or_steal_sources_and_invalid_later_query_has_no_prefix() {
    let f = Fixture::new();
    f.write("outside/B.java", "class B {}");
    for file in ["outside/B.java", "missing.java"] {
        let overlays = f.write("unknown.json", json!({"schema_version":1,"revision":0,"files":[{"compilation":{"project":"app","source_set":"main"},"file":file,"version":0,"text":"class B {}"}]}).to_string());
        let out = f.run("inspect", &["--overlays", overlays.to_str().unwrap()], false);
        assert!(matches!(out["error"]["code"].as_str(), Some("invalid_overlay" | "io")));
    }
    let overlays = f.overlay("class A { int fresh; void run() { fresh++; } }");
    let requests = f.write("late-invalid.json", json!({"schema_version":1,"queries":[
        {"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","position":{"encoding":"utf8","offset":33}},
        {"compilation":{"project":"app","source_set":"main"},"file":"src/A.java","position":{"encoding":"utf16_line","line":100,"character":0}}
    ]}).to_string());
    let out = f.run(
        "declaration",
        &["--overlays", overlays.to_str().unwrap(), "--requests", requests.to_str().unwrap()],
        false,
    );
    assert_eq!(out["error"]["code"], "invalid_position");
    assert_eq!(out["error"]["query_index"], 1);
}

#[cfg(unix)]
#[test]
fn canonical_symlink_alias_must_retain_the_owner() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    symlink(f.0.join("src/A.java"), f.0.join("alias.java")).unwrap();
    let overlays = f.write("alias-overlay.json", json!({"schema_version":1,"revision":1,"files":[{"compilation":{"project":"app","source_set":"main"},"file":"alias.java","version":2,"text":"class A {}"}]}).to_string());
    let out = f.run("inspect", &["--overlays", overlays.to_str().unwrap()], true);
    assert_eq!(
        out["result"]["overlay_state"]["files"][0]["file"],
        f.0.join("src/A.java").canonicalize().unwrap().to_str().unwrap()
    );
}

#[test]
fn maximum_overlay_count_and_json_integer_labels_are_accepted() {
    let f = Fixture::new();
    let mut files = Vec::new();
    for index in 0..1024 {
        let file = format!("src/F{index}.java");
        f.write(&file, format!("class F{index} {{}}"));
        files.push(json!({"compilation":{"project":"app","source_set":"main"},"file":file,"version":9007199254740991u64,"text":""}));
    }
    let overlays =
        f.write("max.json", json!({"schema_version":1,"revision":9007199254740991u64,"files":files}).to_string());
    let out = f.run("inspect", &["--overlays", overlays.to_str().unwrap()], true);
    assert_eq!(out["result"]["overlay_state"]["revision"], 9007199254740991u64);
    assert_eq!(out["result"]["overlay_state"]["files"].as_array().unwrap().len(), 1024);
}

#[test]
fn compilation_and_workspace_overlay_byte_limits_apply_to_the_final_state() {
    let f = Fixture::new();
    let old = "class A { int old; void run() { old++; } }";
    let replacement = "class A { int fresh; void run() { fresh++; } }";
    let overlays = f.overlay(replacement);
    for workspace_limit in [false, true] {
        let mut manifest = json!({"schema_version":1,"compilations":[{"id":{"project":"app","source_set":"main"},"source_roots":["src"]}]});
        if workspace_limit {
            manifest["limits"] = json!({"max_input_bytes":old.len()});
        } else {
            manifest["compilations"][0]["limits"] = json!({"max_input_bytes":old.len()});
        }
        f.write("project.json", manifest.to_string());
        let out = f.run("inspect", &["--overlays", overlays.to_str().unwrap()], false);
        assert_eq!(out["error"]["code"], "overlay_limit");
        f.run("inspect", &[], true);
    }
}

#[test]
fn nested_overlay_fields_reject_unknown_duplicate_and_missing_values_before_loading() {
    let f = Fixture::new();
    let base = "{\"schema_version\":1,\"revision\":1,\"files\":[{\"compilation\":{\"project\":\"app\",\"source_set\":\"main\"},\"file\":\"src/A.java\",\"version\":1,\"text\":\"class A {}\"}]}";
    for document in [
        base.replace("\"version\":1", "\"version\":1,\"version\":2"),
        base.replace("\"text\":\"class A {}\"", "\"text\":\"class A {}\",\"text\":\"\""),
        base.replace("\"version\":1", "\"version\":18446744073709551616"),
        base.replace("\"version\":1", "\"version\":null"),
        base.replace("\"version\":1,", ""),
        base.replace("\"text\":\"class A {}\"", "\"text\":\"class A {}\",\"extra\":0"),
        base.replace("\"project\":\"app\"", "\"project\":\"app\",\"project\":\"other\""),
        base.replace("\"project\":\"app\"", "\"project\":\"app\",\"extra\":0"),
    ] {
        let path = f.write("invalid.json", document);
        assert_eq!(f.run("inspect", &["--overlays", path.to_str().unwrap()], false)["error"]["code"], "invalid_json");
    }
    for value in ["+1", "-1", " 1", "1.0", "4294967296", ""] {
        assert_eq!(
            f.run(
                "declaration",
                &[
                    "--project",
                    "app",
                    "--source-set",
                    "main",
                    "--file",
                    "src/A.java",
                    "--line",
                    value,
                    "--character",
                    "0"
                ],
                false
            )["error"]["code"],
            "invalid_position"
        );
    }
}

#[test]
fn overlay_work_remains_subject_to_real_native_timeout_and_stdin_cancellation() {
    let f = Fixture::new();
    let overlays = f.overlay(&format!("class A {{ {} }}", "int field;".repeat(30000)));
    let out = f.run("parser-report", &["--overlays", overlays.to_str().unwrap(), "--timeout-ms", "1"], false);
    assert_eq!(out["error"]["code"], "execution_timeout");
    assert_eq!(out["error"]["worker_reaped"], true);
    let out = f.run("parser-report", &["--overlays", overlays.to_str().unwrap(), "--cancel-on-stdin-close"], false);
    assert_eq!(out["error"]["code"], "execution_cancelled");
    assert_eq!(out["error"]["worker_reaped"], true);
}

#[test]
fn legacy_absolute_offsets_still_accept_crlf_interior_and_surrogate_pairs_are_strict() {
    let f = Fixture::new();
    let text = "// 😀\r\nclass A { int value; void run() { value++; } }\n";
    f.write("src/A.java", text);
    for position in [json!({"encoding":"utf8","offset":8}), json!({"encoding":"utf16","offset":6})] {
        let requests = f.requests(position.clone());
        let out = f.run("declaration", &["--requests", requests.to_str().unwrap()], true);
        assert_eq!(out["result"]["queries"][0]["position"], position);
        assert_eq!(out["result"]["queries"][0]["byte_offset"], 8);
    }
    for position in [json!({"encoding":"utf8","offset":4}), json!({"encoding":"utf16","offset":4})] {
        let requests = f.requests(position);
        assert_eq!(
            f.run("declaration", &["--requests", requests.to_str().unwrap()], false)["error"]["code"],
            "invalid_position"
        );
    }
}

#[test]
fn source_exclusions_cannot_be_reintroduced_by_unsaved_overlays() {
    let f = Fixture::new();
    f.write(
        "project.json",
        json!({"schema_version":1,"compilations":[{
            "id":{"project":"app","source_set":"main"},"source_roots":["src"],"source_exclusions":["A.java"]
        }]})
        .to_string(),
    );
    let inspected = f.run("inspect", &["--details"], true);
    assert_eq!(inspected["result"]["compilations"][0]["source_paths"], json!([]));
    let overlay = f.overlay("class A { int fresh; }");
    let result = f.run("inspect", &["--overlays", overlay.to_str().unwrap()], false);
    assert_eq!(result["error"]["code"], "invalid_overlay");
}
