use java_engine::{Project, ResolveStatus};
use std::{hint::black_box, time::Instant};

fn project() -> Project {
    let mut project = Project::new();
    for i in 0..200 {
        project.insert(
            format!("C{i}.java"),
            &format!("package p; class C{i} {{ int field; int m() {{ return field; }} }}"),
        );
    }
    project
}

#[test]
#[ignore = "manual synthetic measurement, not a project-scale performance budget"]
fn repeated_query_measurement() {
    let project = project();
    let byte = project.file("C199.java").unwrap().text().rfind("field").unwrap() as u32;
    let start = Instant::now();
    for _ in 0..500 {
        assert_eq!(black_box(project.resolve_at("C199.java", byte)).status, ResolveStatus::Resolved);
    }
    let rebuild = start.elapsed().as_micros();
    let start = Instant::now();
    let prepared = project.prepare();
    let prepare = start.elapsed().as_micros();
    for _ in 0..500 {
        assert_eq!(black_box(prepared.resolve_at("C199.java", byte)), project.resolve_at("C199.java", byte));
    }
    // Time warm queries separately from the fresh-rebuild differential check.
    let start = Instant::now();
    for _ in 0..500 {
        assert_eq!(black_box(prepared.resolve_at("C199.java", byte)).status, ResolveStatus::Resolved);
    }
    println!(
        "synthetic files=200 queries=500 rebuild_us={rebuild} prepare_us={prepare} warm_us={}",
        start.elapsed().as_micros()
    );
}

#[test]
fn prepared_matches_every_offset_and_survives_independent_edits() {
    let old = project();
    let prepared = old.prepare();
    for path in ["C0.java", "C199.java", "missing.java"] {
        let len = old.file(path).map_or(0, |file| file.text().len());
        for byte in 0..=len + 1 {
            assert_eq!(prepared.resolve_at(path, byte as u32), old.resolve_at(path, byte as u32));
        }
    }
    let mut next = old.clone();
    next.insert("C199.java", "package p; class C199 { int m() { return field; } }");
    let byte = next.file("C199.java").unwrap().text().rfind("field").unwrap() as u32;
    assert_eq!(next.prepare().resolve_at("C199.java", byte).status, ResolveStatus::Unresolved);
    let byte = old.file("C199.java").unwrap().text().rfind("field").unwrap() as u32;
    assert_eq!(prepared.resolve_at("C199.java", byte).status, ResolveStatus::Resolved);
}

#[test]
fn prepared_handles_are_send_sync_and_support_shared_queries() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<java_engine::PreparedProject<'_>>();
    send_sync::<java_engine::PreparedWorkspace<'_>>();
    send_sync::<java_engine::PreparedCompilationScope<'_>>();
    let project = project();
    let prepared = project.prepare();
    std::thread::scope(|threads| {
        for path in ["C0.java", "C199.java"] {
            let prepared = &prepared;
            let project = &project;
            threads.spawn(move || {
                for byte in 0..project.file(path).unwrap().text().len() as u32 {
                    assert_eq!(prepared.resolve_at(path, byte), project.resolve_at(path, byte));
                }
            });
        }
    });
}

#[test]
fn prepared_binary_removal_preserves_old_input_context() {
    let mut old = Project::new();
    let text = "class Use extends lib.Lib { void m() { run(); } }";
    old.insert("Use.java", text);
    old.add_class_bytes("Lib.class", include_bytes!("fixtures/classes/Lib.class")).unwrap();
    let byte = text.find("run").unwrap() as u32;
    let prepared = old.prepare();
    let expected = prepared.resolve_at("Use.java", byte);
    assert_eq!(expected.status, ResolveStatus::Resolved);
    let mut next = old.clone();
    assert!(next.remove_class("Lib.class"));
    let next_prepared = next.prepare();
    assert_eq!(next_prepared.resolve_at("Use.java", byte), next.resolve_at("Use.java", byte));
    assert_eq!(next_prepared.resolve_at("Use.java", byte).status, ResolveStatus::Unresolved);
    assert_eq!(prepared.resolve_at("Use.java", byte), expected);
}

#[test]
fn prepared_matches_unicode_incomplete_and_extreme_offsets() {
    let mut project = Project::new();
    for (path, source) in [
        ("Unicode.java", "class Unicode { int café; int m() { return café; } }"),
        ("Broken.java", "class Broken { void m() { missing( ; int x = ; }"),
        ("Empty.java", ""),
    ] {
        project.insert(path, source);
    }
    let prepared = project.prepare();
    for path in ["Unicode.java", "Broken.java", "Empty.java", "missing.java"] {
        let len = project.file(path).map_or(0, |file| file.text().len()) as u32;
        for byte in (0..=len + 1).chain([u32::MAX]) {
            assert_eq!(prepared.resolve_at(path, byte), project.resolve_at(path, byte), "{path}:{byte}");
        }
    }
}
