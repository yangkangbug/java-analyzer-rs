use java_engine::{JarErrorKind, JmodOptions, read_jmod};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const LIB: &[u8] = include_bytes!("fixtures/classes/Lib.class");
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Jmod(PathBuf);
impl Jmod {
    fn bytes(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "java-engine-jmod-{}-{}.jmod",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn new(entries: &[(&str, &[u8])]) -> Self {
        Self::bytes(&jmod(entries, CompressionMethod::Deflated))
    }
}
impl Drop for Jmod {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn archive(entries: &[(&str, &[u8])], compression: CompressionMethod) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default().compression_method(compression)).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn jmod(entries: &[(&str, &[u8])], compression: CompressionMethod) -> Vec<u8> {
    let mut bytes = b"JM\x01\x00".to_vec();
    bytes.extend(archive(entries, compression));
    bytes
}
#[test]
fn reads_classes_only_with_actual_origin_and_no_manifest_semantics() {
    let input = Jmod::new(&[
        ("classes/lib/Lib.class", LIB),
        ("classes/module-info.class", b"ignored module descriptor"),
        ("lib/Lib.class", b"ignored"),
        ("bin/tool", b"ignored"),
        ("META-INF/MANIFEST.MF", b"Class-Path: other.jar\nMulti-Release: true\n"),
        ("classes/META-INF/versions/17/lib/Lib.class", b"ignored"),
    ]);
    let classes = read_jmod(&input.0, &JmodOptions::default()).unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].binary_name, "lib/Lib");
    assert_eq!(classes[0].origin, format!("{}!/classes/lib/Lib.class", input.0.display()));
}
#[test]
fn requires_valid_supported_jmod_header() {
    for bytes in [b"".as_slice(), b"JM", b"PK\x01\x00", b"JM\x01"] {
        assert_eq!(read_jmod(&Jmod::bytes(bytes).0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::Archive);
    }
    for header in [b"JM\x02\x00", b"JM\x01\x01"] {
        let mut bytes = header.to_vec();
        bytes.extend(archive(&[], CompressionMethod::Stored));
        assert_eq!(
            read_jmod(&Jmod::bytes(&bytes).0, &JmodOptions::default()).unwrap_err().kind,
            JarErrorKind::Unsupported
        );
    }
    let zip = Jmod::bytes(&archive(&[], CompressionMethod::Stored));
    assert_eq!(read_jmod(&zip.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::Archive);
}
#[test]
fn counts_nonclass_sections_and_header_toward_limits() {
    let input = Jmod::new(&[("classes/lib/Lib.class", LIB), ("legal/license", &[0; 4096])]);
    let length = std::fs::metadata(&input.0).unwrap().len();
    for options in [
        JmodOptions { max_archive_bytes: length - 1, ..JmodOptions::default() },
        JmodOptions { max_entries: 1, ..JmodOptions::default() },
        JmodOptions { max_entry_bytes: 1024, ..JmodOptions::default() },
        JmodOptions { max_total_uncompressed_bytes: 4200, ..JmodOptions::default() },
    ] {
        assert_eq!(read_jmod(&input.0, &options).unwrap_err().kind, JarErrorKind::LimitExceeded);
    }
}
#[test]
fn rejects_unsafe_resources_and_name_mismatch() {
    for name in ["legal/../license", "/legal/license", "classes/./Lib.class", "legal\\license"] {
        let input = Jmod::new(&[(name, LIB)]);
        assert_eq!(read_jmod(&input.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::UnsafePath);
    }
    let input = Jmod::new(&[("classes/wrong/Lib.class", LIB)]);
    assert_eq!(read_jmod(&input.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::NameMismatch);
}
#[test]
fn rejects_duplicate_entries_corrupt_crc_and_bad_classes() {
    let mut bytes = jmod(&[("classes/lib/Lib.class", LIB), ("classes/lib/Lix.class", LIB)], CompressionMethod::Stored);
    let old = b"classes/lib/Lix.class";
    for i in 0..bytes.len() - old.len() {
        if &bytes[i..i + old.len()] == old {
            bytes[i + 14] = b'b';
        }
    }
    assert_eq!(
        read_jmod(&Jmod::bytes(&bytes).0, &JmodOptions::default()).unwrap_err().kind,
        JarErrorKind::DuplicateEntry
    );
    let mut bytes = jmod(&[("classes/lib/Lib.class", LIB)], CompressionMethod::Stored);
    let position = bytes.windows(4).position(|w| w == [0xca, 0xfe, 0xba, 0xbe]).unwrap();
    bytes[position + 30] ^= 1;
    assert_eq!(read_jmod(&Jmod::bytes(&bytes).0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::Archive);
    let input = Jmod::new(&[("classes/lib/Lib.class", b"bad")]);
    assert_eq!(read_jmod(&input.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::ClassFile);
}

#[test]
fn valid_stored_empty_archives_and_exact_limits_work() {
    let jar = Jmod::bytes(&jmod(&[("classes/lib/Lib.class", LIB)], CompressionMethod::Stored));
    let options = JmodOptions {
        max_archive_bytes: std::fs::metadata(&jar.0).unwrap().len(),
        max_entries: 1,
        max_entry_bytes: LIB.len() as u64,
        max_total_uncompressed_bytes: LIB.len() as u64,
    };
    assert_eq!(read_jmod(&jar.0, &options).unwrap().len(), 1);
    let jar = Jmod::new(&[]);
    assert!(read_jmod(&jar.0, &JmodOptions::default()).unwrap().is_empty());
}
#[test]
fn local_and_central_header_disagreement_fails() {
    let mut bytes = jmod(&[("classes/lib/Lib.class", LIB)], CompressionMethod::Stored);
    // Change just the local name while retaining the central-directory name.
    let name = b"classes/lib/Lib.class";
    let start = bytes.windows(name.len()).position(|w| w == name).unwrap();
    bytes[start] = b'x';
    let jar = Jmod::bytes(&bytes);
    assert_eq!(read_jmod(&jar.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::Archive);
}
#[test]
fn symlinks_encryption_and_unsupported_compression_fail() {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink("link", "classes/lib/Lib.class", SimpleFileOptions::default()).unwrap();
    let mut bytes = b"JM\x01\x00".to_vec();
    bytes.extend(zip.finish().unwrap().into_inner());
    let jar = Jmod::bytes(&bytes);
    assert_eq!(read_jmod(&jar.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::Unsupported);
    for (local_offset, central_offset, value) in [(6, 8, 1u16), (8, 10, 12u16)] {
        let mut bytes = jmod(&[("classes/lib/Lib.class", LIB)], CompressionMethod::Stored);
        let cd = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[4 + local_offset..4 + local_offset + 2].copy_from_slice(&value.to_le_bytes());
        bytes[cd + central_offset..cd + central_offset + 2].copy_from_slice(&value.to_le_bytes());
        let jar = Jmod::bytes(&bytes);
        assert_eq!(read_jmod(&jar.0, &JmodOptions::default()).unwrap_err().kind, JarErrorKind::Unsupported);
    }
}

#[test]
fn output_order_is_binary_name_order_not_archive_order() {
    let mut other = LIB.to_vec();
    let pos = other.windows(7).position(|w| w == b"lib/Lib").unwrap();
    other[pos + 4] = b'Z';
    let jar = Jmod::new(&[("classes/lib/Zib.class", &other), ("classes/lib/Lib.class", LIB)]);
    let classes = read_jmod(&jar.0, &JmodOptions::default()).unwrap();
    assert_eq!(classes.iter().map(|c| c.binary_name.as_str()).collect::<Vec<_>>(), ["lib/Lib", "lib/Zib"]);
}
#[test]
fn forged_uncompressed_size_never_returns_a_partial_class() {
    let mut bytes = jmod(&[("classes/lib/Lib.class", LIB)], CompressionMethod::Deflated);
    let cd = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[26..30].copy_from_slice(&1u32.to_le_bytes());
    bytes[cd + 24..cd + 28].copy_from_slice(&1u32.to_le_bytes());
    let jar = Jmod::bytes(&bytes);
    assert!(
        read_jmod(
            &jar.0,
            &JmodOptions { max_entry_bytes: 1, max_total_uncompressed_bytes: 1, ..JmodOptions::default() }
        )
        .is_err()
    );
}

#[test]
fn default_limits_accept_large_ignored_native_section() {
    // Match the size of Temurin 21's java.base.jmod lib/server/libjvm.so.
    // Synthetic zero bytes are data only, never executable or extracted.
    let native = vec![0; 27_056_488];
    let input = Jmod::new(&[("classes/lib/Lib.class", LIB), ("lib/server/libjvm.so", &native)]);
    assert_eq!(read_jmod(&input.0, &JmodOptions::default()).unwrap().len(), 1);
    let smaller = JmodOptions { max_entry_bytes: 16 * 1024 * 1024, ..JmodOptions::default() };
    assert_eq!(read_jmod(&input.0, &smaller).unwrap_err().kind, JarErrorKind::LimitExceeded);
    assert_eq!(java_engine::JarOptions::default().max_entry_bytes, 16 * 1024 * 1024);
}
