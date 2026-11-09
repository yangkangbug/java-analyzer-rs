use java_engine::{
    CompilationConfig, CompilationId, Project, ProjectConfig, ResolveResult, ResolveStatus, TargetKind,
    WorkspaceConfig, WorkspaceSnapshot,
};

const BASE: &str = "package lib; public class Base { public int inherited; public int onlyBase; public Box box; public void run() {} public Box make() { return box; } }";
const USE: &str = r#"package app;
public class Use extends lib.Base {
    int inherited;
    public void run() {}
    void test(int parameter) {
        int local = 0;
        int a = super./*shadow*/inherited;
        int b = this./* super */ /*this_shadow*/inherited;
        String text = "super.inherited";
        int c = super./*base*/onlyBase;
        int d = this./*this_base*/onlyBase;
        int absent = super./*missing*/missing;
        int e = /*simple_shadow*/inherited;
        int f = /*simple_base*/onlyBase;
        int g = /*param*/parameter + /*local*/local;
        super./*call*/run();
        this./*this_call*/run();
        Object ref = super::/*reference*/run;
        int h = super.box./*chain*/leaf;
        int i = super.make()./*call_chain*/leaf;
        int j = (super.box)./*paren_chain*/leaf;
        accept(super.box, /*argument*/local);
    }
    void parameterShadow(int inherited) {
        int n = super./*parameter_shadow*/inherited;
        int p = /*ordinary_parameter_shadow*/inherited;
    }
    void localShadow() {
        int inherited = 0;
        int n = super./*local_shadow*/inherited;
        int p = /*ordinary_local_shadow*/inherited;
    }
    void accept(lib.Box box, int value) {}
    class Inner {
        int inherited;
        void test() {
            int x = /*outer_prefix*/Use.super./*outer*/inherited;
            Use.super./*outer_call*/run();
            Object ref = Use.super::/*outer_ref*/run;
        }
    }
}"#;

fn offset(marker: &str) -> u32 {
    let marker = format!("/*{marker}*/");
    (USE.find(&marker).unwrap() + marker.len()) as u32
}

fn cases(mut resolve: impl FnMut(u32) -> ResolveResult) {
    for marker in [
        "shadow",
        "missing",
        "parameter_shadow",
        "local_shadow",
        "base",
        "call",
        "reference",
        "chain",
        "call_chain",
        "paren_chain",
        "outer",
        "outer_call",
        "outer_ref",
    ] {
        let result = resolve(offset(marker));
        assert_eq!(result.status, ResolveStatus::ReceiverNotBound, "{marker}: {result:?}");
        assert!(result.targets.is_empty(), "{marker}: {result:?}");
    }
    for (marker, owner, kind) in [
        ("this_shadow", "app.Use", TargetKind::Field),
        ("this_base", "lib.Base", TargetKind::Field),
        ("simple_shadow", "app.Use", TargetKind::Field),
        ("simple_base", "lib.Base", TargetKind::Field),
        ("this_call", "app.Use", TargetKind::Method),
        ("param", "", TargetKind::Parameter),
        ("local", "", TargetKind::Local),
        ("argument", "", TargetKind::Local),
        ("ordinary_parameter_shadow", "", TargetKind::Parameter),
        ("ordinary_local_shadow", "", TargetKind::Local),
        ("outer_prefix", "app.Use", TargetKind::Class),
    ] {
        let result = resolve(offset(marker));
        assert_eq!(result.status, ResolveStatus::Resolved, "{marker}: {result:?}");
        assert_eq!(result.targets.len(), 1, "{marker}");
        assert_eq!(result.targets[0].kind, kind, "{marker}");
        if !owner.is_empty() {
            assert_eq!(result.targets[0].owner, owner, "{marker}");
        }
    }
}

#[test]
fn super_is_not_a_simple_or_this_receiver_in_legacy_and_prepared_queries() {
    let mut project = Project::new();
    for (path, text) in
        [("Base.java", BASE), ("Use.java", USE), ("Box.java", "package lib; public class Box { public int leaf; }")]
    {
        project.insert(path, text);
        assert!(project.file(path).unwrap().errors().is_empty(), "{path}: {:?}", project.file(path).unwrap().errors());
    }
    cases(|byte| project.resolve_at("Use.java", byte));
    let prepared = project.prepare();
    cases(|byte| prepared.resolve_at("Use.java", byte));
}

#[test]
fn super_guard_applies_across_compilation_scopes() {
    let root = std::env::temp_dir().join(format!("java-engine-super-{}", std::process::id()));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    for (path, text) in [
        ("lib/Base.java", BASE),
        ("app/Use.java", USE),
        ("lib/Box.java", "package lib; public class Box { public int leaf; }"),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let lib = CompilationId::new("lib", "main");
    let app = CompilationId::new("app", "main");
    let workspace = WorkspaceSnapshot::load(&WorkspaceConfig {
        compilations: vec![
            CompilationConfig {
                id: lib.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("lib")], ..Default::default() },
                dependencies: vec![],
            },
            CompilationConfig {
                id: app.clone(),
                inputs: ProjectConfig { source_roots: vec![root.join("app")], ..Default::default() },
                dependencies: vec![lib],
            },
        ],
        ..Default::default()
    })
    .unwrap();
    let path = root.join("app/Use.java").canonicalize().unwrap().to_str().unwrap().to_owned();
    cases(|byte| workspace.scope(&app).unwrap().resolve_at(&path, byte));
    let prepared = workspace.prepare();
    cases(|byte| prepared.scope(&app).unwrap().resolve_at(&path, byte));
}
