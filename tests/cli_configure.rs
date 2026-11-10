#![cfg(feature = "cli")]

use serde_json::{Value, json};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
use zip::{ZipWriter, write::SimpleFileOptions};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "java-analyzer-configure-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn write(&self, path: &str, data: impl AsRef<[u8]>) -> PathBuf {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, data).unwrap();
        path
    }
    fn command(&self) -> Command {
        // Explicit test-only override permits proving regressions against a frozen native baseline.
        let binary = std::env::var_os("JAVA_ANALYZER_CONFIGURE_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_java-analyzer").into());
        let mut command = Command::new(binary);
        command.current_dir(&self.0).env_clear().env("PATH", "");
        command
    }
    fn output(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    fn run(&self, args: &[&str], success: bool) -> Value {
        checked(self.output(args), success)
    }
    fn configure(&self, options: &[&str]) -> Output {
        self.output(&[&["configure"][..], options].concat())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn checked(output: Output, success: bool) -> Value {
    assert_eq!(
        output.status.code(),
        Some(if success { 0 } else { 2 }),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let data = if success {
        assert!(output.stderr.is_empty());
        assert!(output.stdout.ends_with(b"\n"));
        output.stdout
    } else {
        assert!(output.stdout.is_empty());
        output.stderr
    };
    serde_json::from_slice(&data).unwrap()
}
fn archive(jmod: bool) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(if jmod { "classes/lib/Lib.class" } else { "lib/Lib.class" }, SimpleFileOptions::default()).unwrap();
    zip.write_all(include_bytes!("fixtures/classes/Lib.class")).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    if jmod { [b"JM\x01\0".as_slice(), &bytes].concat() } else { bytes }
}

#[test]
fn native_output_is_deterministic_relocatable_and_directly_consumable() {
    let f = Fixture::new();
    let source = "class A { int value; int read() { return value; } }";
    let path = f.write("source 空间/A.java", source);
    f.write("existing.json", "keep existing bytes");
    let args = ["--compilation", "app:/opaque", "main:/opaque", "--source-root", "source 空间", "--no-platform"];
    let first = f.configure(&args);
    let bytes = first.stdout.clone();
    let manifest = checked(first, true);
    assert_eq!(bytes, f.configure(&args).stdout);
    assert!(bytes.len() <= 1024 * 1024);
    assert_eq!(manifest["schema_version"], 1);
    assert!(manifest.get("result").is_none());
    assert_eq!(manifest["compilations"][0]["id"], json!({"project":"app:/opaque", "source_set":"main:/opaque"}));
    assert_eq!(manifest["compilations"][0]["source_roots"], json!([f.0.join("source 空间")]));
    assert_eq!(manifest["compilations"][0]["target_release"], 17);
    assert_eq!(manifest["limits"]["max_compilations"], 128);
    assert_eq!(fs::read(f.0.join("existing.json")).unwrap(), b"keep existing bytes");
    assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 2, "configure writes no product files");
    let elsewhere = Fixture::new();
    let saved = elsewhere.write("relocated/project.json", bytes);
    let inspection = checked(
        elsewhere
            .command()
            .current_dir(std::env::temp_dir())
            .args(["inspect", "--manifest"])
            .arg(&saved)
            .output()
            .unwrap(),
        true,
    );
    assert_eq!(inspection["result"]["compilations"][0]["source_paths"], json!([path]));
    let result = elsewhere.run(
        &[
            "declaration",
            "--manifest",
            saved.to_str().unwrap(),
            "--project",
            "app:/opaque",
            "--source-set",
            "main:/opaque",
            "--file",
            path.to_str().unwrap(),
            "--byte",
            &source.rfind("value").unwrap().to_string(),
        ],
        true,
    );
    assert_eq!(result["result"]["queries"][0]["status"], "Resolved");
}

#[test]
fn groups_preserve_order_typed_inputs_and_source_only_dependency_edges() {
    let f = Fixture::new();
    let main = f.write("main/Main.java", "class Main {}");
    let test = f.write("test/Test.java", "class Test extends Main {}");
    f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    f.write("first.jar", archive(false));
    f.write("later.jar", archive(false));
    f.write("platform.jmod", archive(true));
    f.write("platform.jar", archive(false));
    let output = f.configure(&[
        "--compilation",
        "z/app",
        "test:scope",
        "--source-root",
        "test",
        "--depends-on",
        "z/app",
        "main:scope",
        "--no-platform",
        "--compilation",
        "z/app",
        "main:scope",
        "--source-root",
        "main",
        "--target-release",
        "11",
        "--classpath-jar",
        "first.jar",
        "--classpath-dir",
        "classes",
        "--classpath-jar",
        "later.jar",
        "--platform-jmod",
        "platform.jmod",
        "--platform-jar",
        "platform.jar",
    ]);
    let bytes = output.stdout.clone();
    let manifest = checked(output, true);
    let c = &manifest["compilations"];
    assert_eq!(c[0]["id"]["source_set"], "test:scope");
    assert_eq!(c[1]["id"]["source_set"], "main:scope");
    assert_eq!(c[0]["dependencies"], json!([{"project":"z/app", "source_set":"main:scope"}]));
    assert_eq!(
        c[1]["classpath"],
        json!([
        {"kind":"jar", "path":f.0.join("first.jar")}, {"kind":"directory", "path":f.0.join("classes")},
        {"kind":"jar", "path":f.0.join("later.jar")} ])
    );
    assert_eq!(
        c[1]["platform"],
        json!([
        {"kind":"jmod", "path":f.0.join("platform.jmod")}, {"kind":"jar", "path":f.0.join("platform.jar")} ])
    );
    let saved = f.write("generated.json", bytes);
    let inspection = f.run(&["inspect", "--manifest", saved.to_str().unwrap(), "--details"], true);
    let scopes = &inspection["result"]["compilations"];
    assert_eq!(scopes[0]["source_paths"], json!([main.clone()]));
    assert_eq!(scopes[0]["visible_source_paths"], json!([main.clone()]));
    assert_eq!(scopes[1]["source_paths"], json!([test.clone()]));
    assert_eq!(scopes[1]["visible_source_paths"], json!([main, test]));
    assert_eq!(scopes[1]["class_count"], 0, "source edge must not export binary/platform inputs");
    assert_eq!(scopes[0]["class_count"], 1);
    assert!(scopes[0]["class_origins"].to_string().contains("platform.jmod"));
    assert!(scopes[0]["shadowed_classes"].to_string().contains("first.jar"));
    let output = f.configure(&[
        "--compilation",
        "app",
        "main",
        "--binary-only",
        "--no-platform",
        "--classpath-jar",
        "first.jar",
        "--classpath-dir",
        "classes",
        "--classpath-jar",
        "later.jar",
    ]);
    let bytes = output.stdout.clone();
    checked(output, true);
    let saved = f.write("classpath-only.json", bytes);
    let inspection = f.run(&["inspect", "--manifest", saved.to_str().unwrap(), "--details"], true);
    assert!(inspection["result"]["compilations"][0]["class_origins"].to_string().contains("first.jar"));
}

#[test]
fn invalid_grammar_is_atomic_and_preserves_existing_files() {
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    f.write("project.json", "original manifest");
    let cases: Vec<Vec<&str>> = vec![
        vec![],
        vec!["--source-root", "src"],
        vec!["--compilation", "app"],
        vec!["--compilation", "app", "main"],
        vec!["--compilation", "app", "main", "--source-root", "src"],
        vec!["--compilation", "app", "main", "--no-platform"],
        vec!["--compilation", "app", "main", "--binary-only", "--source-root", "src", "--no-platform"],
        vec!["--compilation", "app", "main", "--source-root", "src", "--binary-only", "--no-platform"],
        vec!["--compilation", "app", "main", "--binary-only", "--binary-only", "--no-platform"],
        vec!["--compilation", "app", "main", "--binary-only", "--no-platform", "--no-platform"],
        vec!["--compilation", "app", "main", "--binary-only", "--no-platform", "--platform-jar", "absent.jar"],
        vec!["--compilation", "app", "main", "--binary-only", "--platform-jmod", "absent.jmod", "--no-platform"],
        vec![
            "--compilation",
            "app",
            "main",
            "--binary-only",
            "--no-platform",
            "--target-release",
            "11",
            "--target-release",
            "17",
        ],
        vec!["--compilation", "app", "main", "--binary-only", "--no-platform", "--output", "project.json"],
        vec!["--compilation", "app", "main", "--binary-only", "--no-platform", "--depends-on", "app"],
    ];
    for args in cases {
        let error = checked(f.configure(&args), false);
        assert_eq!(error["error"]["code"], "usage", "{args:?}: {error}");
    }
    for (project, set) in [("", "main"), ("app", ""), ("bad id", "main"), ("app", "test\nname")] {
        let error = checked(f.configure(&["--compilation", project, set, "--binary-only", "--no-platform"]), false);
        assert_eq!(error["error"]["code"], "invalid_configuration");
    }
    for release in ["0", "-1", "+17", "17.0", "65536", "", " 17"] {
        checked(
            f.configure(&[
                "--compilation",
                "app",
                "main",
                "--binary-only",
                "--no-platform",
                "--target-release",
                release,
            ]),
            false,
        );
    }
    assert_eq!(fs::read(f.0.join("project.json")).unwrap(), b"original manifest");
    assert_eq!(fs::read(f.0.join("src/A.java")).unwrap(), b"class A {}");
}

#[test]
fn existing_loader_refusals_are_not_hidden_by_configuration_generation() {
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    f.write("module/module-info.java", "module unsupported {}");
    f.write("bad.jar", "not an archive");
    for options in [
        vec!["--source-root", "absent"],
        vec!["--source-root", "module"],
        vec!["--source-root", "src/A.java"],
        vec!["--binary-only", "--classpath-jar", "src"],
        vec!["--binary-only", "--classpath-dir", "bad.jar"],
        vec!["--binary-only", "--classpath-jar", "bad.jar"],
        vec!["--binary-only", "--depends-on", "missing", "main"],
    ] {
        let args = [&["--compilation", "app", "main", "--no-platform"][..], &options].concat();
        assert_eq!(checked(f.configure(&args), false)["error"]["code"], "load_failed", "{args:?}");
    }
    for tail in [
        vec!["--compilation", "app", "main", "--binary-only", "--no-platform"],
        vec!["--compilation", "app", "test", "--source-root", "src", "--no-platform"],
        vec!["--depends-on", "app", "main"],
        vec![
            "--depends-on",
            "app",
            "test",
            "--depends-on",
            "app",
            "test",
            "--compilation",
            "app",
            "test",
            "--binary-only",
            "--no-platform",
        ],
        vec![
            "--depends-on",
            "app",
            "test",
            "--compilation",
            "app",
            "test",
            "--binary-only",
            "--no-platform",
            "--depends-on",
            "app",
            "main",
        ],
    ] {
        let args = [&["--compilation", "app", "main", "--source-root", "src", "--no-platform"][..], &tail].concat();
        assert_eq!(checked(f.configure(&args), false)["error"]["code"], "load_failed", "{args:?}");
    }
}

#[cfg(unix)]
#[test]
fn leaf_symlinks_are_not_canonicalized_away() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    f.write("classes/lib/Lib.class", include_bytes!("fixtures/classes/Lib.class"));
    f.write("lib.jar", archive(false));
    f.write("java.base.jmod", archive(true));
    for (target, link, flag, binary) in [
        ("src", "src-link", "--source-root", false),
        ("classes", "classes-link", "--classpath-dir", true),
        ("lib.jar", "lib-link.jar", "--classpath-jar", true),
        ("lib.jar", "platform-link.jar", "--platform-jar", true),
        ("java.base.jmod", "platform-link.jmod", "--platform-jmod", true),
    ] {
        symlink(f.0.join(target), f.0.join(link)).unwrap();
        let mut args = vec!["--compilation", "app", "main", flag, link];
        if binary {
            args.push("--binary-only");
        }
        if !flag.starts_with("--platform") {
            args.push("--no-platform");
        }
        assert_eq!(checked(f.configure(&args), false)["error"]["code"], "load_failed");
        assert!(fs::symlink_metadata(f.0.join(link)).unwrap().file_type().is_symlink());
    }
}

#[test]
fn opaque_flag_looking_ids_and_paths_are_not_supervision_switches() {
    let f = Fixture::new();
    f.write("--cancel-on-stdin-close/A.java", "class A {}");
    let result = checked(
        f.configure(&[
            "--timeout-ms",
            "5000",
            "--compilation",
            "--timeout-ms",
            "--cancel-on-stdin-close",
            "--source-root",
            "--cancel-on-stdin-close",
            "--no-platform",
            "--compilation",
            "--cancel-on-stdin-close",
            "--timeout-ms",
            "--binary-only",
            "--no-platform",
            "--depends-on",
            "--timeout-ms",
            "--cancel-on-stdin-close",
        ]),
        true,
    );
    assert_eq!(
        result["compilations"][0]["id"],
        json!({"project":"--timeout-ms", "source_set":"--cancel-on-stdin-close"})
    );
    assert_eq!(result["compilations"][1]["dependencies"][0], result["compilations"][0]["id"]);
    assert!(Path::new(result["compilations"][0]["source_roots"][0].as_str().unwrap()).is_absolute());
}

#[test]
fn output_expansion_and_existing_input_budgets_fail_without_partial_stdout() {
    let f = Fixture::new();
    // JSON escaping expands these valid opaque IDs sixfold. Total argv is below
    // 200 KiB, including on platforms with a smaller OS argument ceiling.
    let id = "\u{1}".repeat(60_000);
    let output = f.configure(&[
        "--compilation",
        &id,
        "one",
        "--binary-only",
        "--no-platform",
        "--compilation",
        &id,
        "two",
        "--binary-only",
        "--no-platform",
        "--compilation",
        &id,
        "three",
        "--binary-only",
        "--no-platform",
    ]);
    assert_eq!(checked(output, false)["error"]["code"], "output_limit");
    let large = f.write("src/Large.java", []);
    fs::File::options().write(true).open(&large).unwrap().set_len(8 * 1024 * 1024 + 1).unwrap();
    let output = f.configure(&["--compilation", "app", "main", "--source-root", "src", "--no-platform"]);
    let error = checked(output, false);
    assert_eq!(error["error"]["code"], "load_failed");
    assert!(error["error"]["message"].as_str().unwrap().contains("LimitExceeded"));
    assert_eq!(fs::metadata(large).unwrap().len(), 8 * 1024 * 1024 + 1);
    let mut args = vec!["configure".to_string()];
    for index in 0..129 {
        args.extend([
            "--compilation".into(),
            format!("p{index}"),
            "main".into(),
            "--binary-only".into(),
            "--no-platform".into(),
        ]);
    }
    let output = f.command().args(args).output().unwrap();
    assert_eq!(checked(output, false)["error"]["code"], "load_failed");
}

#[cfg(target_os = "linux")]
#[test]
fn argument_byte_limit_is_checked_before_worker_spawn() {
    // Linux permits this bounded argv; portable exact-boundary tests also exercise
    // the parser directly, independent of a host's smaller exec argument limit.
    let f = Fixture::new();
    let mut args = vec!["configure".to_string()];
    for index in 0..12 {
        args.extend([
            "--compilation".into(),
            "x".repeat(90_000),
            index.to_string(),
            "--binary-only".into(),
            "--no-platform".into(),
        ]);
    }
    let output = f.command().args(args).output().unwrap();
    assert_eq!(checked(output, false)["error"]["code"], "document_limit");
}

#[test]
fn source_roots_and_forward_edges_keep_caller_order() {
    let f = Fixture::new();
    f.write("z/Z.java", "class Z {}");
    f.write("a/A.java", "class A {}");
    let output = f.configure(&[
        "--compilation",
        "app",
        "tests",
        "--binary-only",
        "--no-platform",
        "--depends-on",
        "lib",
        "z",
        "--depends-on",
        "lib",
        "a",
        "--compilation",
        "lib",
        "z",
        "--source-root",
        "z",
        "--source-root",
        "a",
        "--no-platform",
        "--compilation",
        "lib",
        "a",
        "--binary-only",
        "--no-platform",
    ]);
    let manifest = checked(output, true);
    assert_eq!(
        manifest["compilations"][0]["dependencies"],
        json!([
        {"project":"lib", "source_set":"z"}, {"project":"lib", "source_set":"a"}])
    );
    assert_eq!(manifest["compilations"][1]["source_roots"], json!([f.0.join("z"), f.0.join("a")]));
}

#[test]
fn actual_configuration_timeout_and_cancellation_remain_supervised() {
    let f = Fixture::new();
    f.write("src/A.java", format!("/*{}*/ class A {{}}", "a".repeat(4 * 1024 * 1024)));
    let args = ["--compilation", "app", "main", "--source-root", "src", "--no-platform"];
    let started = std::time::Instant::now();
    for (extra, code) in
        [(&["--timeout-ms", "1"][..], "execution_timeout"), (&["--cancel-on-stdin-close"][..], "execution_cancelled")]
    {
        let error = checked(f.configure(&[&args[..], extra].concat()), false);
        assert_eq!(error["error"]["code"], code);
        assert_eq!(error["error"]["worker_reaped"], true);
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}

#[test]
fn help_advertises_only_explicit_configuration_generation() {
    let f = Fixture::new();
    let help = f.run(&["--help"], true);
    assert_eq!(help["capabilities"]["configuration_generation"]["mode"], "explicit_inputs");
    assert_eq!(help["capabilities"]["configuration_generation"]["output"], "stdout");
    assert_eq!(help["capabilities"]["configuration_generation"]["discovers_dependencies"], false);
    assert_eq!(help["capabilities"]["configuration_generation"]["generates_sources"], false);
    assert_eq!(help["capabilities"]["build_tool_import"], false);
    assert!(help["usage"].as_array().unwrap().iter().any(|s| s.as_str().unwrap().contains("configure --compilation")));
}

#[test]
fn generated_manifest_initializes_the_native_lsp_without_java() {
    use std::{
        io::{BufRead, BufReader, Read},
        process::{Child, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    struct Guard(Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let f = Fixture::new();
    f.write("src/A.java", "class A { int value; int read() { return value; } }");
    let output = f.configure(&["--compilation", "app", "main", "--source-root", "src", "--no-platform"]);
    let bytes = output.stdout.clone();
    checked(output, true);
    let relocated = Fixture::new();
    let manifest = relocated.write("config/project.json", bytes);
    let mut child = Guard(
        relocated
            .command()
            .args(["lsp", "--manifest"])
            .arg(manifest)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut input = child.0.stdin.take().unwrap();
    let output = child.0.stdout.take().unwrap();
    let (send, receive) = mpsc::sync_channel(4);
    thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut length = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.strip_prefix("Content-Length: ") {
                    length = value.trim().parse::<usize>().ok();
                }
            }
            let Some(length) = length.filter(|n| *n <= 1024 * 1024) else { return };
            let mut bytes = vec![0; length];
            if reader.read_exact(&mut bytes).is_err() {
                return;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { return };
            if send.send(value).is_err() {
                return;
            }
        }
    });
    let mut send = |message: Value| {
        let body = serde_json::to_vec(&message).unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        input.write_all(&body).unwrap();
        input.flush().unwrap();
    };
    send(json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{"capabilities":{}}}));
    let response = receive.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["capabilities"]["definitionProvider"], true, "{response}");
    send(json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}));
    send(json!({"jsonrpc":"2.0", "id":2, "method":"shutdown", "params":null}));
    assert_eq!(receive.recv_timeout(Duration::from_secs(5)).unwrap()["id"], 2);
    send(json!({"jsonrpc":"2.0", "method":"exit", "params":null}));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "LSP failed to exit");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn source_exclusions_apply_to_every_root_independently_of_option_order() {
    let f = Fixture::new();
    for root in ["one", "two"] {
        f.write(&format!("{root}/Keep.java"), "class Keep {}");
        f.write(&format!("{root}/module-info.java"), "module unsupported {}");
        f.write(&format!("{root}/--cancel-on-stdin-close/Bad.java"), [0xff]);
        f.write(&format!("{root}/中文 !/Bad.java"), [0xff]);
    }
    for before in [true, false] {
        let roots = ["--source-root", "one", "--source-root", "two"];
        let rules = [
            "--exclude-source",
            "module-info.java",
            "--exclude-source",
            "--cancel-on-stdin-close",
            "--exclude-source",
            "中文 !",
        ];
        let mut args = vec!["--compilation", "app", "main", "--no-platform"];
        args.extend(if before { &rules[..] } else { &roots[..] });
        args.extend(if before { &roots[..] } else { &rules[..] });
        let output = f.configure(&args);
        let bytes = output.stdout.clone();
        let manifest = checked(output, true);
        assert_eq!(
            manifest["compilations"][0]["source_exclusions"],
            json!(["module-info.java", "--cancel-on-stdin-close", "中文 !"])
        );
        let saved = f.write("selected.json", bytes);
        let inspection = f.run(&["inspect", "--manifest", saved.to_str().unwrap(), "--details"], true);
        assert_eq!(
            inspection["result"]["compilations"][0]["source_paths"],
            json!([f.0.join("one/Keep.java"), f.0.join("two/Keep.java")])
        );
    }
}

#[test]
fn source_exclusions_are_optional_and_scoped_to_the_current_compilation() {
    let f = Fixture::new();
    f.write("one/Keep.java", "class Keep {}");
    f.write("two/Keep.java", "class Keep {}");
    let manifest = checked(
        f.configure(&[
            "--compilation",
            "app",
            "one",
            "--exclude-source",
            "Keep.java",
            "--source-root",
            "one",
            "--no-platform",
            "--compilation",
            "app",
            "two",
            "--source-root",
            "two",
            "--no-platform",
        ]),
        true,
    );
    assert_eq!(manifest["compilations"][0]["source_exclusions"], json!(["Keep.java"]));
    assert!(manifest["compilations"][1].get("source_exclusions").is_none());
    let saved = f.write("selected.json", manifest.to_string());
    let inspection = f.run(&["inspect", "--manifest", saved.to_str().unwrap(), "--details"], true);
    assert_eq!(inspection["result"]["compilations"][0]["source_paths"], json!([]));
    assert_eq!(inspection["result"]["compilations"][1]["source_paths"], json!([f.0.join("two/Keep.java")]));
}
