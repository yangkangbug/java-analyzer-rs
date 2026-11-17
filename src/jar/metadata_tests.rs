use super::*;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

type Entry<'a> = (&'a str, Option<&'a str>, Option<&'a str>, u16);
struct Pool {
    bytes: Vec<u8>,
    count: u16,
}
impl Pool {
    fn new() -> Self {
        Self { bytes: vec![], count: 1 }
    }
    fn utf(&mut self, s: &str) -> u16 {
        let n = self.count;
        self.count += 1;
        self.bytes.push(1);
        self.bytes.extend((s.len() as u16).to_be_bytes());
        self.bytes.extend(s.as_bytes());
        n
    }
    fn class(&mut self, s: &str) -> u16 {
        let text = self.utf(s);
        let n = self.count;
        self.count += 1;
        self.bytes.push(7);
        self.bytes.extend(text.to_be_bytes());
        n
    }
}
fn class(name: &str, groups: &[Vec<Entry<'_>>], enclosing: bool) -> (Vec<u8>, Vec<usize>) {
    let mut p = Pool::new();
    let this = p.class(name);
    let parent = p.class("java/lang/Object");
    let inner_attr = p.utf("InnerClasses");
    let unknown = p.utf("Unrelated");
    let mut attrs = vec![(unknown, vec![7, 8, 9])];
    for group in groups {
        let mut payload = (group.len() as u16).to_be_bytes().to_vec();
        for (binary, outer, simple, flags) in group {
            let inner = p.class(binary);
            let outer = outer.map(|n| p.class(n)).unwrap_or(0);
            let simple = simple.map(|n| p.utf(n)).unwrap_or(0);
            for value in [inner, outer, simple, *flags] {
                payload.extend(value.to_be_bytes());
            }
        }
        attrs.push((inner_attr, payload));
    }
    if enclosing {
        let name = p.utf("EnclosingMethod");
        let owner = p.class("p/Outer");
        let mut bytes = owner.to_be_bytes().to_vec();
        bytes.extend(0u16.to_be_bytes());
        attrs.push((name, bytes));
    }
    attrs.push((unknown, vec![]));
    let mut b = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61];
    b.extend(p.count.to_be_bytes());
    b.extend(p.bytes);
    for value in [0x21u16, this, parent, 0, 0, 0, attrs.len() as u16] {
        b.extend(value.to_be_bytes());
    }
    let mut offsets = vec![];
    for (name, payload) in attrs {
        b.extend(name.to_be_bytes());
        b.extend((payload.len() as u32).to_be_bytes());
        if name == inner_attr {
            offsets.push(b.len());
        }
        b.extend(payload);
    }
    (b, offsets)
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Archive(PathBuf);
impl Drop for Archive {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn write_archive(path: &Path, jmod: bool, flags: u16) {
    let (bytes, _) = class("p/Outer$Inner", &[vec![("p/Outer$Inner", Some("p/Outer"), Some("Inner"), flags)]], true);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let name = if jmod { "classes/p/Outer$Inner.class" } else { "p/Outer$Inner.class" };
    zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
    zip.write_all(&bytes).unwrap();
    let data = zip.finish().unwrap().into_inner();
    let mut archive = if jmod { vec![b'J', b'M', 1, 0] } else { vec![] };
    archive.extend(data);
    std::fs::write(path, archive).unwrap();
}
fn archive(jmod: bool) -> Archive {
    let path = std::env::temp_dir().join(format!(
        "java-engine-metadata-cache-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    write_archive(&path, jmod, 0x8009);
    Archive(path)
}
fn read(cache: &mut ArchiveReads, path: &Path, jmod: bool, max_entry: u64) -> Result<SharedArchiveRead, JarError> {
    if jmod {
        cache.read_jmod(path, &JmodOptions { max_entry_bytes: max_entry, ..Default::default() })
    } else {
        cache.read_jar(path, &JarOptions { max_entry_bytes: max_entry, ..Default::default() })
    }
}
#[test]
fn metadata_survives_jar_and_jmod_cache_hits_and_content_changes() {
    for jmod in [false, true] {
        let file = archive(jmod);
        let probe = test_support::ArchiveProbe::new();
        let mut cache = ArchiveReads::default();
        let first = read(&mut cache, &file.0, jmod, u64::MAX).unwrap();
        let second = read(&mut cache, &file.0, jmod, u64::MAX).unwrap();
        assert_eq!(first.classes.len(), 1);
        assert!(Arc::ptr_eq(&first.classes[0], &second.classes[0]));
        assert_eq!(first.classes[0].inner_classes[0][0].raw_access_flags, 0x8009);
        assert!(first.classes[0].has_enclosing_method);
        assert_eq!(probe.paths().len(), 1);
        assert_eq!(probe.captures().len(), 2);
        // Equal byte count and unchanged origin cannot authorize stale metadata.
        write_archive(&file.0, jmod, 0x0002);
        let changed = read(&mut cache, &file.0, jmod, u64::MAX).unwrap();
        assert!(!Arc::ptr_eq(&first.classes[0], &changed.classes[0]));
        assert_eq!(changed.classes[0].inner_classes[0][0].raw_access_flags, 2);
        assert_eq!(first.classes[0].inner_classes[0][0].raw_access_flags, 0x8009);
        assert_eq!(probe.paths().len(), 2);
        assert_eq!(probe.captures().len(), 3);
    }
}
#[test]
fn metadata_cache_does_not_bypass_tighter_jar_or_jmod_limits() {
    for jmod in [false, true] {
        let file = archive(jmod);
        let mut cache = ArchiveReads::default();
        let old = read(&mut cache, &file.0, jmod, u64::MAX).unwrap();
        assert_eq!(read(&mut cache, &file.0, jmod, 1).unwrap_err().kind, JarErrorKind::LimitExceeded);
        let restored = read(&mut cache, &file.0, jmod, u64::MAX).unwrap();
        assert_eq!(old.classes[0].inner_classes, restored.classes[0].inner_classes);
        assert_eq!(old.classes[0].has_enclosing_method, restored.classes[0].has_enclosing_method);
        let public: Vec<ClassSignature> = if jmod {
            read_jmod(&file.0, &JmodOptions::default()).unwrap()
        } else {
            read_jar(&file.0, &JarOptions::default()).unwrap()
        };
        assert_eq!(public[0], restored.classes[0].class_signature);
    }
}
