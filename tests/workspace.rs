use java_engine::{
    ClasspathEntry, CompilationConfig, CompilationId, PlatformEntry, ProjectConfig, ResolveStatus, WorkspaceConfig,
    WorkspaceLoadErrorKind, WorkspaceSnapshot,
};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use zip::{ZipWriter, write::SimpleFileOptions};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "java-engine-scopes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, name: &str, data: impl AsRef<[u8]>) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, data).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
// Minimal JVMS class signatures, assembled here without a Java compiler.
fn class(name: &str, method: &str) -> Vec<u8> {
    class_with_descriptor(name, method, "()Ljava/lang/String;")
}
fn class_with_descriptor(name: &str, method: &str, descriptor: &str) -> Vec<u8> {
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    fn u(b: &mut Vec<u8>, n: u16) {
        b.extend(n.to_be_bytes());
    }
    fn utf(b: &mut Vec<u8>, s: &str) {
        b.push(1);
        u(b, s.len() as u16);
        b.extend(s.as_bytes());
    }
    u(&mut b, 7);
    utf(&mut b, name);
    b.push(7);
    u(&mut b, 1);
    utf(&mut b, "java/lang/Object");
    b.push(7);
    u(&mut b, 3);
    utf(&mut b, method);
    utf(&mut b, descriptor);
    u(&mut b, 0x21);
    u(&mut b, 2);
    u(&mut b, if name == "java/lang/Object" { 0 } else { 4 });
    u(&mut b, 0);
    u(&mut b, 0);
    u(&mut b, 1);
    u(&mut b, 0x101);
    u(&mut b, 5);
    u(&mut b, 6);
    u(&mut b, 0);
    u(&mut b, 0);
    b
}
fn archive(jmod: bool, method: &str) -> Vec<u8> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("java/lang/Object.class", class("java/lang/Object", method)),
        ("java/lang/String.class", class("java/lang/String", "intern")),
        ("module-info.class", b"ignored module descriptor".to_vec()),
        ("resource", vec![0; 17]),
    ] {
        let name = if jmod { format!("classes/{name}") } else { name.into() };
        z.start_file(name, SimpleFileOptions::default()).unwrap();
        z.write_all(&data).unwrap();
    }
    let zip = z.finish().unwrap().into_inner();
    if jmod { [b"JM\x01\x00".as_slice(), &zip].concat() } else { zip }
}

fn id(project: &str, set: &str) -> CompilationId {
    CompilationId::new(project, set)
}
fn compilation(project: &str, set: &str, roots: Vec<PathBuf>, deps: Vec<CompilationId>) -> CompilationConfig {
    CompilationConfig {
        id: id(project, set),
        inputs: ProjectConfig { source_roots: roots, ..Default::default() },
        dependencies: deps,
    }
}
fn key(path: &Path) -> String {
    path.canonicalize().unwrap().to_str().unwrap().to_owned()
}
fn query(s: &WorkspaceSnapshot, scope: &CompilationId, path: &Path, needle: &str) -> java_engine::ResolveResult {
    let text = fs::read_to_string(path).unwrap();
    let byte = text.rfind(needle).unwrap() as u32;
    let fresh = s.scope(scope).unwrap().resolve_at(&key(path), byte);
    let prepared = s.prepare();
    assert_eq!(prepared.scope(scope).unwrap().resolve_at(&key(path), byte), fresh);
    fresh
}
fn fixture() -> (WorkspaceConfig, PathBuf) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scopes");
    let configs = vec![
        compilation("app", "test", vec![root.join("app/test")], vec![id("app", "main")]),
        compilation("app", "main", vec![root.join("app/main")], vec![id("lib", "main")]),
        compilation("lib", "main", vec![root.join("lib/main")], vec![]),
        compilation("lib", "test", vec![root.join("lib/test")], vec![id("lib", "main")]),
        compilation("unrelated", "main", vec![root.join("unrelated/main")], vec![]),
    ];
    (WorkspaceConfig { compilations: configs, ..Default::default() }, root)
}
#[test]
fn main_test_and_sibling_visibility_are_directed_and_transitive() {
    let (config, root) = fixture();
    let s = WorkspaceSnapshot::load(&config).unwrap();
    let app = root.join("app/main/app/App.java");
    let test = root.join("app/test/app/AppTest.java");
    assert_eq!(query(&s, &id("app", "main"), &app, "value").status, ResolveStatus::Resolved);
    assert_eq!(query(&s, &id("app", "test"), &test, "value").status, ResolveStatus::Resolved);
    assert_eq!(query(&s, &id("app", "test"), &test, "LibTest").status, ResolveStatus::Unresolved);
    assert_eq!(query(&s, &id("app", "test"), &test, "Other").status, ResolveStatus::Unresolved);
    assert_eq!(query(&s, &id("app", "main"), &test, "value").status, ResolveStatus::Unresolved);
    assert_eq!(query(&s, &id("lib", "main"), &app, "value").status, ResolveStatus::Unresolved);
    let scope = s.scope(&id("app", "test")).unwrap();
    assert_eq!(scope.id(), &id("app", "test"));
    let paths = scope.source_paths();
    assert_eq!(paths.len(), 3);
    assert!(paths.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(scope.inputs().source_paths().len(), 1);
    assert!(s.scope(&id("missing", "main")).is_none());
}
#[test]
fn consumer_symbols_do_not_fix_dependency_superclasses_or_field_receivers() {
    let f = Fixture::new();
    let lib = f.write("lib/Lib.java", "package lib; import consumer.Imported; public class Lib extends Helper { public Helper field; public consumer.Qualified qualified; public Imported imported; void own() { leaked; } }");
    f.write("app/Helper.java", "package lib; public class Helper { public int leaked; }");
    f.write("app/Qualified.java", "package consumer; public class Qualified { public int leaked; }");
    f.write("app/Imported.java", "package consumer; public class Imported { public int leaked; }");
    f.write("lib/QualifiedLib.java", "package lib; public class QualifiedLib extends consumer.Qualified {}");
    let qualified_app = f.write(
        "app/QualifiedApp.java",
        "package app; class QualifiedApp extends lib.QualifiedLib { void call() { leaked; } }",
    );
    let app = f.write("app/App.java", "package app; class App extends lib.Lib { void call() { leaked; field.leaked; qualified.leaked; imported.leaked; } }");
    let config = WorkspaceConfig {
        compilations: vec![
            compilation("lib", "main", vec![f.0.join("lib")], vec![]),
            compilation("app", "test", vec![f.0.join("app")], vec![id("lib", "main")]),
        ],
        ..Default::default()
    };
    let s = WorkspaceSnapshot::load(&config).unwrap();
    for scope in [id("lib", "main"), id("app", "test")] {
        assert_eq!(query(&s, &scope, &lib, "leaked").status, ResolveStatus::Unresolved);
        assert_eq!(query(&s, &scope, &lib, "Helper").status, ResolveStatus::Unresolved);
    }
    assert_eq!(query(&s, &id("app", "test"), &qualified_app, "leaked").status, ResolveStatus::Unresolved);
    let text = fs::read_to_string(&app).unwrap();
    let scope = s.scope(&id("app", "test")).unwrap();
    let prepared = s.prepare();
    let warm = prepared.scope(&id("app", "test")).unwrap();
    for byte in 0..=text.len() as u32 {
        assert_eq!(warm.resolve_at(&key(&app), byte), scope.resolve_at(&key(&app), byte));
    }
    assert_eq!(scope.resolve_at(&key(&app), text.find("leaked;").unwrap() as u32).status, ResolveStatus::Unresolved);
    for receiver in ["field", "qualified", "imported"] {
        let offset = text.find(&format!("{receiver}.leaked")).unwrap() + receiver.len() + 1;
        assert_eq!(scope.resolve_at(&key(&app), offset as u32).status, ResolveStatus::ReceiverNotBound, "{receiver}");
    }
}
#[test]
fn edge_changes_reload_fresh_views_without_mutating_old_snapshots() {
    let (mut config, root) = fixture();
    let app = root.join("app/main/app/App.java");
    let old = WorkspaceSnapshot::load(&config).unwrap();
    let old_prepared = old.prepare();
    let old_byte = fs::read_to_string(&app).unwrap().rfind("value").unwrap() as u32;
    let old_result = old_prepared.scope(&id("app", "main")).unwrap().resolve_at(&key(&app), old_byte);
    config.compilations[1].dependencies.clear();
    let removed = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(query(&removed, &id("app", "main"), &app, "value").status, ResolveStatus::Unresolved);
    config.compilations[1].dependencies = vec![id("lib", "test")];
    let changed = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(
        query(&changed, &id("app", "test"), &root.join("app/test/app/AppTest.java"), "LibTest").status,
        ResolveStatus::Resolved
    );
    assert_eq!(query(&changed, &id("app", "main"), &app, "value").status, ResolveStatus::Resolved);
    config.compilations[1].dependencies = vec![id("absent", "main")];
    assert!(WorkspaceSnapshot::load(&config).is_err());
    assert_eq!(old_prepared.scope(&id("app", "main")).unwrap().resolve_at(&key(&app), old_byte), old_result);
    assert_eq!(old_result.status, ResolveStatus::Resolved);
    assert_eq!(query(&old, &id("app", "main"), &app, "value").status, ResolveStatus::Resolved);
    assert_eq!(query(&removed, &id("app", "main"), &app, "value").status, ResolveStatus::Unresolved);
}
#[test]
fn graph_errors_and_limits_are_explicit() {
    let base = compilation("a", "main", vec![], vec![]);
    let check = |compilations, kind| {
        let config = WorkspaceConfig { compilations, ..Default::default() };
        assert_eq!(WorkspaceSnapshot::load(&config).unwrap_err().kind, kind);
    };
    check(vec![compilation("", "main", vec![], vec![])], WorkspaceLoadErrorKind::InvalidId);
    check(vec![compilation("a", "", vec![], vec![])], WorkspaceLoadErrorKind::InvalidId);
    check(vec![compilation("  ", "main", vec![], vec![])], WorkspaceLoadErrorKind::InvalidId);
    check(vec![compilation("a", "\t", vec![], vec![])], WorkspaceLoadErrorKind::InvalidId);
    let missing = Fixture::new();
    check(
        vec![
            compilation("a", "main", vec![missing.0.join("a-missing")], vec![id("b", "main")]),
            compilation("b", "main", vec![missing.0.join("b-missing")], vec![id("c", "main")]),
            compilation("c", "main", vec![missing.0.join("c-missing")], vec![id("a", "main")]),
        ],
        WorkspaceLoadErrorKind::Cycle,
    );
    check(vec![base.clone(), base.clone()], WorkspaceLoadErrorKind::DuplicateCompilation);
    check(vec![compilation("a", "main", vec![], vec![id("b", "main")])], WorkspaceLoadErrorKind::UnknownDependency);
    check(
        vec![base.clone(), compilation("b", "main", vec![], vec![id("a", "main"), id("a", "main")])],
        WorkspaceLoadErrorKind::DuplicateDependency,
    );
    check(vec![compilation("a", "main", vec![], vec![id("a", "main")])], WorkspaceLoadErrorKind::Cycle);
    check(
        vec![
            compilation("a", "main", vec![], vec![id("b", "main")]),
            compilation("b", "main", vec![], vec![id("a", "main")]),
        ],
        WorkspaceLoadErrorKind::Cycle,
    );
    let config = WorkspaceConfig { compilations: vec![base.clone()], max_compilations: 0, ..Default::default() };
    assert_eq!(WorkspaceSnapshot::load(&config).unwrap_err().kind, WorkspaceLoadErrorKind::LimitExceeded);
    let config = WorkspaceConfig {
        compilations: vec![base, compilation("b", "main", vec![], vec![id("a", "main")])],
        max_dependency_edges: 0,
        ..Default::default()
    };
    assert_eq!(WorkspaceSnapshot::load(&config).unwrap_err().kind, WorkspaceLoadErrorKind::LimitExceeded);
}
#[test]
fn shared_source_ownership_is_rejected_and_input_errors_propagate() {
    let f = Fixture::new();
    f.write("src/A.java", "class A {}");
    let config = WorkspaceConfig {
        compilations: vec![
            compilation("a", "main", vec![f.0.join("src")], vec![]),
            compilation("b", "main", vec![f.0.join("src")], vec![]),
        ],
        ..Default::default()
    };
    assert_eq!(WorkspaceSnapshot::load(&config).unwrap_err().kind, WorkspaceLoadErrorKind::DuplicateSource);
    let config = WorkspaceConfig {
        compilations: vec![compilation("a", "main", vec![f.0.join("missing")], vec![])],
        ..Default::default()
    };
    assert_eq!(WorkspaceSnapshot::load(&config).unwrap_err().kind, WorkspaceLoadErrorKind::Input);
}
#[test]
fn binary_versions_and_platforms_remain_in_their_declaring_compilation() {
    let f = Fixture::new();
    let lib = f.write("lib/Lib.java", "package lib; public class Lib { void call() { oldObject(); } }");
    let app =
        f.write("app/App.java", "package app; class App extends lib.Lib { void call() { oldObject(); newObject(); } }");
    let plain = f.write("plain/Plain.java", "class Plain { String value; }");
    let old = f.write("old.jmod", archive(true, "oldObject"));
    let new = f.write("new.jar", archive(false, "newObject"));
    let mut library = compilation("lib", "main", vec![f.0.join("lib")], vec![]);
    library.inputs.platform = vec![PlatformEntry::Jmod(old)];
    let mut consumer = compilation("app", "main", vec![f.0.join("app")], vec![id("lib", "main")]);
    consumer.inputs.classpath = vec![ClasspathEntry::Jar(new)];
    let config = WorkspaceConfig {
        compilations: vec![
            library,
            consumer,
            compilation("plain", "main", vec![f.0.join("plain")], vec![id("lib", "main")]),
        ],
        ..Default::default()
    };
    let s = WorkspaceSnapshot::load(&config).unwrap();
    for scope in [id("lib", "main"), id("app", "main")] {
        let r = query(&s, &scope, &lib, "oldObject");
        assert_eq!(r.status, ResolveStatus::Resolved);
        assert!(r.targets[0].path.contains("old.jmod!"));
    }
    assert_eq!(query(&s, &id("app", "main"), &app, "oldObject").status, ResolveStatus::Resolved);
    assert_eq!(query(&s, &id("app", "main"), &app, "newObject").status, ResolveStatus::Unresolved);
    assert_eq!(query(&s, &id("plain", "main"), &plain, "String").status, ResolveStatus::Unresolved);
}
#[test]
fn shared_jar_superclasses_use_scope_specific_platform_versions() {
    let f = Fixture::new();
    let a = f.write(
        "a/A.java",
        "class A extends shared.Shared { public shared.Shared field; void call() { oldObject(); newObject(); sharedMethod(); changedMethod(); } }",
    );
    let b = f.write(
        "b/B.java",
        "class B extends shared.Shared { void call() { oldObject(); newObject(); sharedMethod(); } }",
    );
    let c = f.write("b/C.java", "class C extends A { void call() { field.oldObject(); field.newObject(); } }");
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("shared/Shared.class", SimpleFileOptions::default()).unwrap();
    zip.write_all(&class("shared/Shared", "sharedMethod")).unwrap();
    let jar = f.write("shared.jar", zip.finish().unwrap().into_inner());
    let old = f.write("old.jmod", archive(true, "oldObject"));
    let new = f.write("new.jmod", archive(true, "newObject"));
    let mut first = compilation("a", "main", vec![f.0.join("a")], vec![]);
    first.inputs.classpath = vec![ClasspathEntry::Jar(jar.clone())];
    first.inputs.platform = vec![PlatformEntry::Jmod(old)];
    let mut second = compilation("b", "main", vec![f.0.join("b")], vec![id("a", "main")]);
    second.inputs.classpath = vec![ClasspathEntry::Jar(jar.clone())];
    second.inputs.platform = vec![PlatformEntry::Jmod(new)];
    let config = WorkspaceConfig { compilations: vec![first, second], ..Default::default() };
    let s = WorkspaceSnapshot::load(&config).unwrap();
    for scope in [id("a", "main"), id("b", "main")] {
        assert_eq!(query(&s, &scope, &a, "oldObject").status, ResolveStatus::Resolved);
        assert_eq!(query(&s, &scope, &a, "newObject").status, ResolveStatus::Unresolved);
    }
    assert_eq!(query(&s, &id("b", "main"), &b, "newObject").status, ResolveStatus::Resolved);
    assert_eq!(query(&s, &id("b", "main"), &b, "oldObject").status, ResolveStatus::Unresolved);
    assert_eq!(query(&s, &id("b", "main"), &b, "sharedMethod").status, ResolveStatus::Resolved);
    let inherited = query(&s, &id("b", "main"), &c, "oldObject");
    assert_eq!(inherited.status, ResolveStatus::Resolved);
    assert!(inherited.targets[0].path.contains("old.jmod!"));
    assert_eq!(query(&s, &id("b", "main"), &c, "newObject").status, ResolveStatus::Unresolved);

    let origin = format!("{}!/shared/Shared.class", key(&jar));
    assert!(std::ptr::eq(
        s.scope(&id("a", "main")).unwrap().inputs().project().class_signature(&origin).unwrap(),
        s.scope(&id("b", "main")).unwrap().inputs().project().class_signature(&origin).unwrap(),
    ));
    let prepared = s.prepare();
    for scope in [id("a", "main"), id("b", "main")] {
        for path in [&a, &b, &c] {
            for byte in 0..=fs::read_to_string(path).unwrap().len() as u32 + 1 {
                assert_eq!(
                    prepared.scope(&scope).unwrap().resolve_at(&key(path), byte),
                    s.scope(&scope).unwrap().resolve_at(&key(path), byte),
                );
            }
        }
    }
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("shared/Shared.class", SimpleFileOptions::default()).unwrap();
    zip.write_all(&class("shared/Shared", "changedMethod")).unwrap();
    fs::write(&jar, zip.finish().unwrap().into_inner()).unwrap();
    let next = WorkspaceSnapshot::load(&config).unwrap();
    for (snapshot, present, absent) in [(&s, "sharedMethod", "changedMethod"), (&next, "changedMethod", "sharedMethod")]
    {
        assert_eq!(query(snapshot, &id("a", "main"), &a, present).status, ResolveStatus::Resolved);
        assert_eq!(query(snapshot, &id("a", "main"), &a, absent).status, ResolveStatus::Unresolved);
    }
    let byte = fs::read_to_string(&a).unwrap().rfind("sharedMethod").unwrap() as u32;
    assert_eq!(prepared.scope(&id("a", "main")).unwrap().resolve_at(&key(&a), byte).status, ResolveStatus::Resolved);
}

#[test]
fn shared_jar_descriptors_resolve_only_in_their_declaring_compilation() {
    let f = Fixture::new();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, method, descriptor) in
        [("shared/Shared", "choose", "(Ldep/Arg;)V"), ("java/lang/Object", "choose", "(Ldep/Arg;)V")]
    {
        zip.start_file(format!("{name}.class"), SimpleFileOptions::default()).unwrap();
        zip.write_all(&class_with_descriptor(name, method, descriptor)).unwrap();
    }
    let shared = f.write("shared.jar", zip.finish().unwrap().into_inner());
    let mut compilations = Vec::new();
    let mut paths = Vec::new();
    for scope in ["a", "b", "c"] {
        paths.push(
            f.write(&format!("{scope}/Use.java"), "class Use extends shared.Shared { void run() { choose(null); } }"),
        );
        let mut input = compilation(scope, "main", vec![f.0.join(scope)], vec![]);
        input.inputs.classpath.push(ClasspathEntry::Jar(shared.clone()));
        if scope != "b" {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            zip.start_file("dep/Arg.class", SimpleFileOptions::default()).unwrap();
            zip.write_all(&class("dep/Arg", "value")).unwrap();
            let arg = f.write(&format!("{scope}-arg.jar"), zip.finish().unwrap().into_inner());
            input.inputs.classpath.push(ClasspathEntry::Jar(arg));
        }
        compilations.push(input);
    }
    let snapshot = WorkspaceSnapshot::load(&WorkspaceConfig { compilations, ..Default::default() }).unwrap();
    let origin = format!("{}!/shared/Shared.class", key(&shared));
    let mut signatures = Vec::new();
    for (scope, path) in ["a", "b", "c"].into_iter().zip(paths) {
        let result = query(&snapshot, &id(scope, "main"), &path, "choose");
        assert_eq!(
            result.status,
            if scope == "b" { ResolveStatus::SignatureNotCompared } else { ResolveStatus::Resolved },
        );
        if scope != "b" {
            assert_eq!(result.targets.len(), 1);
            assert_eq!(result.targets[0].path, origin);
        }
        signatures
            .push(snapshot.scope(&id(scope, "main")).unwrap().inputs().project().class_signature(&origin).unwrap());
    }
    assert!(std::ptr::eq(signatures[0], signatures[1]));
    assert!(std::ptr::eq(signatures[0], signatures[2]));
}
#[test]
fn workspace_budgets_apply_across_compilations() {
    let f = Fixture::new();
    f.write("a/A.java", "class A {}");
    f.write("b/B.java", "class B {}");
    let base = WorkspaceConfig {
        compilations: vec![
            compilation("a", "main", vec![f.0.join("a")], vec![]),
            compilation("b", "main", vec![f.0.join("b")], vec![]),
        ],
        ..Default::default()
    };
    assert!(WorkspaceSnapshot::load(&WorkspaceConfig { max_input_bytes: 19, ..base.clone() }).is_err());
    assert!(WorkspaceSnapshot::load(&WorkspaceConfig { max_entries: 1, ..base }).is_err());
    let jar = f.write("objects.jar", archive(false, "toString"));
    let mut first = compilation("a", "main", vec![], vec![]);
    first.inputs.classpath = vec![ClasspathEntry::Jar(jar.clone())];
    let mut second = compilation("b", "main", vec![], vec![]);
    second.inputs.classpath = vec![ClasspathEntry::Jar(jar)];
    let single =
        WorkspaceConfig { compilations: vec![first.clone()], max_archive_expanded_bytes: 400, ..Default::default() };
    assert!(WorkspaceSnapshot::load(&single).is_ok());
    assert!(WorkspaceSnapshot::load(&WorkspaceConfig { compilations: vec![first, second], ..single }).is_err());
}
#[test]
fn diamond_edges_do_not_duplicate_sources_and_configuration_order_is_irrelevant() {
    let f = Fixture::new();
    f.write("base/Base.java", "package p; public class Base { public int value; }");
    let app = f.write("app/App.java", "class App extends p.Base { void call() { value; } }");
    let mut config = WorkspaceConfig {
        compilations: vec![
            compilation("app", "main", vec![f.0.join("app")], vec![id("left", "main"), id("right", "main")]),
            compilation("left", "main", vec![], vec![id("base", "main")]),
            compilation("right", "main", vec![], vec![id("base", "main")]),
            compilation("base", "main", vec![f.0.join("base")], vec![]),
        ],
        ..Default::default()
    };
    let first = WorkspaceSnapshot::load(&config).unwrap();
    config.compilations.reverse();
    config.compilations.last_mut().unwrap().dependencies.reverse();
    let second = WorkspaceSnapshot::load(&config).unwrap();
    let result = query(&first, &id("app", "main"), &app, "value");
    assert_eq!(result.status, ResolveStatus::Resolved);
    assert_eq!(result, query(&second, &id("app", "main"), &app, "value"));
    assert_eq!(
        first.scope(&id("app", "main")).unwrap().source_paths(),
        second.scope(&id("app", "main")).unwrap().source_paths()
    );
    assert_eq!(first.scope(&id("app", "main")).unwrap().source_paths().len(), 2);
}
#[test]
fn visible_source_collisions_remain_ambiguous() {
    let f = Fixture::new();
    f.write("one/Name.java", "package p; public class Name {}");
    f.write("two/Name.java", "package p; public class Name {}");
    let app = f.write("app/App.java", "class App { p.Name value; }");
    let config = WorkspaceConfig {
        compilations: vec![
            compilation("one", "main", vec![f.0.join("one")], vec![]),
            compilation("two", "main", vec![f.0.join("two")], vec![]),
            compilation("app", "main", vec![f.0.join("app")], vec![id("one", "main"), id("two", "main")]),
        ],
        ..Default::default()
    };
    let s = WorkspaceSnapshot::load(&config).unwrap();
    assert_eq!(query(&s, &id("app", "main"), &app, "Name").status, ResolveStatus::Ambiguous);
}

#[test]
fn prepared_workspace_matches_every_byte_in_every_scope() {
    let (config, _) = fixture();
    let snapshot = WorkspaceSnapshot::load(&config).unwrap();
    let prepared = snapshot.prepare();
    assert!(prepared.scope(&id("absent", "main")).is_none());
    let paths: Vec<_> = snapshot
        .compilation_ids()
        .iter()
        .flat_map(|id| snapshot.scope(id).unwrap().inputs().source_paths().to_vec())
        .collect();
    std::thread::scope(|threads| {
        for id in snapshot.compilation_ids() {
            let fresh = snapshot.scope(id).unwrap();
            let warm = prepared.scope(id).unwrap();
            assert_eq!(warm.id(), fresh.id());
            assert_eq!(warm.source_paths(), fresh.source_paths());
            assert_eq!(warm.inputs().source_paths(), fresh.inputs().source_paths());
            let paths = &paths;
            threads.spawn(move || {
                for path in paths {
                    let len = fs::read_to_string(path).unwrap().len();
                    for byte in 0..=len + 1 {
                        assert_eq!(
                            warm.resolve_at(path, byte as u32),
                            fresh.resolve_at(path, byte as u32),
                            "{id:?} {path}:{byte}"
                        );
                    }
                }
                assert_eq!(warm.resolve_at("missing.java", u32::MAX), fresh.resolve_at("missing.java", u32::MAX));
            });
        }
    });
}

#[test]
#[ignore = "manual fixed-fixture measurement, not a project-scale performance budget"]
fn repeated_workspace_queries_measurement() {
    use std::{hint::black_box, time::Instant};
    let (config, root) = fixture();
    let snapshot = WorkspaceSnapshot::load(&config).unwrap();
    let scope_id = id("app", "test");
    let path = key(&root.join("app/test/app/AppTest.java"));
    let byte = fs::read_to_string(&path).unwrap().rfind("value").unwrap() as u32;
    let fresh = snapshot.scope(&scope_id).unwrap();
    let expected = fresh.resolve_at(&path, byte);
    assert_eq!(expected.status, ResolveStatus::Resolved);
    let start = Instant::now();
    for _ in 0..500 {
        assert_eq!(black_box(fresh.resolve_at(&path, byte)), expected);
    }
    let rebuild = start.elapsed().as_micros();
    let start = Instant::now();
    let prepared = snapshot.prepare();
    let prepare = start.elapsed().as_micros();
    let start = Instant::now();
    for _ in 0..500 {
        assert_eq!(black_box(prepared.scope(&scope_id).unwrap().resolve_at(&path, byte)), expected);
    }
    println!(
        "fixture=scopes scopes=5 queries=500 rebuild_us={rebuild} prepare_us={prepare} warm_us={}",
        start.elapsed().as_micros()
    );
}

#[test]
fn binary_method_identity_uses_actual_declaration_context_not_origin_guessing() {
    let f = Fixture::new();
    let shared = f.write("shared.jar", include_bytes!("fixtures/project/deps/library.jar"));
    let a = f.write("a/A.java", "class A extends lib.Lib { void use(){run();} }");
    let b = f.write("b/B.java", "class B extends lib.Lib { void use(){run();} }");
    let mut first = compilation("z", "main", vec![f.0.join("a")], vec![]);
    first.inputs.classpath = vec![ClasspathEntry::Jar(shared.clone())];
    let mut second = compilation("a", "test", vec![f.0.join("b")], vec![]);
    second.inputs.classpath = vec![ClasspathEntry::Jar(shared)];
    let s =
        WorkspaceSnapshot::load(&WorkspaceConfig { compilations: vec![first, second], ..Default::default() }).unwrap();
    let one = query(&s, &id("z", "main"), &a, "run");
    let two = query(&s, &id("a", "test"), &b, "run");
    assert_eq!(one.status, ResolveStatus::Resolved);
    assert_eq!(two.status, ResolveStatus::Resolved);
    assert_eq!(one.targets[0].path, two.targets[0].path);
    for (r, scope) in [(&one, id("z", "main")), (&two, id("a", "test"))] {
        let identity = r.targets[0].binary.as_ref().unwrap();
        assert_eq!(identity.binary_name, "lib/Lib");
        assert_eq!(identity.descriptor, "()V");
        assert_eq!(identity.linking_context, java_engine::BinaryLinkingContext::Compilation(scope));
    }
    assert_ne!(one.targets, two.targets);
}
