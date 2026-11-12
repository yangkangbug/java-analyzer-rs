//! Optional, checksum-gated upstream-source acceptance and repeated-query baseline.
//! Use scripts/check-real-source.py with the pinned external artifacts.
use java_engine::{GreenNode, PlatformEntry, ProjectConfig, ProjectSnapshot, ResolveResult, SyntaxKind, TargetKind};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

struct Query {
    id: String,
    path: String,
    offset: u32,
    fields: Vec<String>,
    targets: Vec<Vec<String>>,
}

// Audit only direct unqualified invocation names, not arguments, value receivers,
// or method references. Refused/ambiguous methods are not semantic successes.
fn unqualified_callees(node: &GreenNode, start: u32, out: &mut Vec<u32>) {
    if node.kind() == SyntaxKind::CallExpr {
        let mut offset = start;
        for child in node.children() {
            if let Some(callee) = child.as_node() {
                if callee.kind() == SyntaxKind::ReferenceExpr {
                    for part in callee.children() {
                        if let Some(token) = part.as_token()
                            && token.kind() == SyntaxKind::Ident
                        {
                            out.push(offset);
                            break;
                        }
                        offset += part.utf8_len();
                    }
                }
                break;
            }
            offset += child.utf8_len();
        }
    }
    let mut offset = start;
    for child in node.children() {
        if let Some(child) = child.as_node() {
            unqualified_callees(child, offset, out);
        }
        offset += child.utf8_len();
    }
}

fn check(query: &Query, result: &ResolveResult) {
    let f = &query.fields;
    // The immutable TSV's q06 target is now independently justified by the
    // effective-binary proof. Its intermediate Ambiguous stage remains frozen
    // in binary-method-probes-v3.json and the CLI baseline verification mode.
    let expected_targets = query.targets.clone();
    let (status, count) = (f[4].as_str(), f[5].parse::<usize>().unwrap());
    if query.id == "q06" {
        assert_eq!(&f[1..], &["org/apache/commons/lang3/StringUtils.java", "15520", "length", "Resolved", "1"]);
        assert_eq!(result.targets.len(), 1);
        let identity = result.targets[0].binary.as_ref().unwrap();
        assert_eq!(identity.binary_name, "java/lang/String");
        assert_eq!(identity.descriptor, "()I");
    }
    assert_eq!(format!("{:?}", result.status), status, "{} status", query.id);
    assert_eq!(result.targets.len(), count, "{} count", query.id);
    assert_eq!(expected_targets.len(), result.targets.len(), "{} fixture target count", query.id);
    for (target, expected) in result.targets.iter().zip(&expected_targets) {
        assert!(target.path.ends_with(&expected[1]), "{} target origin", query.id);
        assert_eq!(format!("{:?}", target.kind), expected[2], "{} kind", query.id);
        assert_eq!(target.owner, if expected[3] == "-" { "" } else { &expected[3] }, "{} owner", query.id);
        assert_eq!(target.name, expected[4], "{} name", query.id);
        assert_eq!(target.start_byte, expected[5].parse::<u32>().unwrap(), "{} start", query.id);
        assert_eq!(target.end_byte, expected[6].parse::<u32>().unwrap(), "{} end", query.id);
    }
}

fn timings(label: &str, samples: &mut [u128]) {
    samples.sort_unstable();
    let percentile = |p: usize| samples[(samples.len() * p).div_ceil(100) - 1];
    println!(
        "{label}: samples={} total_ns={} p50_ns={} p95_ns={} min_ns={} max_ns={}",
        samples.len(),
        samples.iter().sum::<u128>(),
        percentile(50),
        percentile(95),
        samples[0],
        samples[samples.len() - 1]
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("use scripts/check-real-source.py SOURCES_JAR JAVA_BASE_JMOD [--rounds N]".into());
    }
    let root = PathBuf::from(&args[0]).canonicalize()?;
    let rounds: usize = args[2].to_str().ok_or("invalid rounds")?.parse()?;
    if !(1..=100).contains(&rounds) {
        return Err("rounds must be between 1 and 100".into());
    }
    let started = Instant::now();
    let snapshot = ProjectSnapshot::load(&ProjectConfig {
        source_roots: vec![root.clone()],
        platform: vec![PlatformEntry::Jmod(PathBuf::from(&args[1]))],
        ..Default::default()
    })?;
    let load_ns = started.elapsed().as_nanos();
    assert_eq!(snapshot.source_paths().len(), 249);
    assert_eq!(snapshot.class_origins().len(), 7615);
    assert!(snapshot.shadowed_classes().is_empty());
    let mut errors = BTreeMap::new();
    let mut source_bytes = 0;
    for path in snapshot.source_paths() {
        let file = snapshot.project().file(path).ok_or("missing parsed source")?;
        assert!(file.tokens_cover_text(), "lossless token coverage: {path}");
        source_bytes += file.text().len();
        let syntax_errors = file.errors();
        let count = syntax_errors.len();
        if count != 0 {
            let relative = PathBuf::from(path).strip_prefix(&root)?.to_string_lossy().replace('\\', "/");
            println!("parser_gap\t{relative}\t{count}");
            let first = &syntax_errors[0];
            let line_start = file.text()[..first.start_byte as usize].rfind('\n').map_or(0, |i| i + 1);
            let line_end = file.text()[first.start_byte as usize..]
                .find('\n')
                .map_or(file.text().len(), |i| first.start_byte as usize + i);
            println!(
                "first_parser_gap\t{relative}\tbyte={}\t{}\t{:?}",
                first.start_byte,
                first.message,
                &file.text()[line_start..line_end]
            );
            errors.insert(relative, count);
        }
    }
    assert_eq!(source_bytes, 3_676_819);
    let expected_errors: BTreeMap<String, usize> = include_str!("real_source/parser-errors.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let (path, count) = line.split_once('\t').unwrap();
            (path.trim_start_matches('/').to_owned(), count.parse().unwrap())
        })
        .collect();
    if errors != expected_errors {
        println!("parser_gap_baseline_changed=true (inventory only, not a fixed error-count gate)");
    } else {
        println!("parser_gap_baseline_changed=false");
    }
    let target_rows: Vec<Vec<String>> = include_str!("real_source/targets.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<String> = line.split('\t').map(str::to_owned).collect();
            assert_eq!(fields.len(), 7);
            fields
        })
        .collect();
    let queries: Vec<Query> = include_str!("real_source/queries.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<String> = line.split('\t').map(str::to_owned).collect();
            assert_eq!(fields.len(), 6);
            Query {
                id: fields[0].clone(),
                path: root.join(&fields[1]).to_str().unwrap().to_owned(),
                offset: fields[2].parse().unwrap(),
                targets: target_rows.iter().filter(|target| target[0] == fields[0]).cloned().collect(),
                fields,
            }
        })
        .collect();
    let started = Instant::now();
    let prepared = snapshot.project().prepare();
    let prepare_ns = started.elapsed().as_nanos();
    let mut calls = 0;
    let mut wrong_namespace = 0;
    for path in snapshot.source_paths() {
        let file = snapshot.project().file(path).ok_or("missing parsed source")?;
        let mut callees = Vec::new();
        unqualified_callees(file.root(), 0, &mut callees);
        for byte in callees {
            calls += 1;
            let result = prepared.resolve_at(path, byte);
            if result.targets.iter().any(|target| target.kind != TargetKind::Method) {
                wrong_namespace += 1;
                println!("wrong_call_namespace\t{path}\t{byte}\t{result:?}");
            }
        }
    }
    println!("unqualified_call_namespace_audit: calls={calls} wrong_namespace={wrong_namespace}");
    assert_eq!(calls, 3030, "review pinned call inventory when parser/walker coverage changes");
    assert_eq!(wrong_namespace, 0, "method invocations must never resolve to variable/type declarations");
    let mut expected = Vec::new();
    for query in &queries {
        let file = snapshot.project().file(&query.path).ok_or("missing query source")?;
        assert!(file.text()[query.offset as usize..].starts_with(&query.fields[3]));
        let result = prepared.resolve_at(&query.path, query.offset);
        check(query, &result);
        println!(
            "query\t{}\t{}\t{}\t{:?}\ttargets={}",
            query.id,
            query.fields[1],
            query.offset,
            result.status,
            result.targets.len()
        );
        expected.push(result);
    }
    let mut legacy_samples = Vec::new();
    let mut prepared_samples = Vec::new();
    for round in 0..rounds {
        for (query, expected) in queries.iter().zip(&expected) {
            // Alternate ordering across rounds; equality checks are outside timers.
            for legacy in [round % 2 == 0, round % 2 != 0] {
                let started = Instant::now();
                let result = if legacy {
                    snapshot.resolve_at(black_box(&query.path), black_box(query.offset))
                } else {
                    prepared.resolve_at(black_box(&query.path), black_box(query.offset))
                };
                let elapsed = started.elapsed().as_nanos();
                assert_eq!(&result, expected, "{} prepared/legacy equality", query.id);
                if legacy {
                    legacy_samples.push(elapsed);
                } else {
                    prepared_samples.push(elapsed);
                }
            }
        }
    }
    println!(
        "profile={} sources=249 source_bytes={source_bytes} platform_classes=7615 files_with_parser_errors={} parser_errors={} queries={} rounds={rounds}",
        if cfg!(debug_assertions) { "debug" } else { "release" },
        errors.len(),
        errors.values().sum::<usize>(),
        queries.len()
    );
    println!("load_parse_ns={load_ns} prepare_index_ns={prepare_ns}");
    timings("legacy_rebuild_query", &mut legacy_samples);
    timings("prepared_warm_query", &mut prepared_samples);
    println!(
        "Measured fixed name-binding queries, not complete project support, an incremental cache, or a performance guarantee."
    );
    Ok(())
}
