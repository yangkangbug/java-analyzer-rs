use java_engine::{JarErrorKind, JarOptions, read_jar};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const LIB: &[u8] = include_bytes!("fixtures/classes/Lib.class");
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Jar(PathBuf);
impl Jar {
    fn bytes(bytes: &[u8]) -> Self {
        let path = std::env::temp_dir().join(format!(
            "java-engine-jar-{}-{}.jar",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn new(entries: &[(&str, &[u8])]) -> Self {
        Self::bytes(&archive(entries, CompressionMethod::Deflated))
    }
}
impl Drop for Jar {
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
#[test]
fn reads_deflated_class_with_actual_origin() {
    let jar = Jar::new(&[("lib/Lib.class", LIB), ("resource.txt", b"hello")]);
    let classes = read_jar(&jar.0, &JarOptions::default()).unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].binary_name, "lib/Lib");
    assert_eq!(classes[0].origin, format!("{}!/lib/Lib.class", jar.0.display()));
}
#[test]
fn multi_release_selects_highest_eligible_and_ignores_without_manifest() {
    let jar = Jar::new(&[
        ("lib/Lib.class", LIB),
        ("META-INF/versions/9/lib/Lib.class", LIB),
        ("META-INF/versions/17/lib/Lib.class", LIB),
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\r\nMulti-Release: tr\r\n ue\r\n\r\n"),
    ]);
    for (release, suffix) in [
        (8, "!/lib/Lib.class"),
        (9, "!/META-INF/versions/9/lib/Lib.class"),
        (16, "!/META-INF/versions/9/lib/Lib.class"),
        (17, "!/META-INF/versions/17/lib/Lib.class"),
    ] {
        let classes = read_jar(&jar.0, &JarOptions { target_release: release, ..JarOptions::default() }).unwrap();
        assert_eq!(classes.len(), 1);
        assert!(classes[0].origin.ends_with(suffix));
    }
    let jar = Jar::new(&[("lib/Lib.class", LIB), ("META-INF/versions/17/lib/Lib.class", b"ignored")]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap().len(), 1);
}
#[test]
fn manifest_named_sections_do_not_enable_multi_release() {
    let jar = Jar::new(&[
        ("lib/Lib.class", LIB),
        ("META-INF/versions/9/lib/Lib.class", b"ignored"),
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n\nName: lib/Lib.class\nMulti-Release: true\n"),
    ]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap().len(), 1);
}
#[test]
fn rejects_implicit_manifest_dependencies() {
    let jar = Jar::new(&[("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\nClass-Path: oth\n er.jar\n\n")]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Unsupported);
}
#[test]
fn rejects_unsafe_paths_and_binary_name_mismatch() {
    for name in
        ["../Lib.class", "/lib/Lib.class", "lib\\Lib.class", "C:/Lib.class", "lib/./Lib.class", "lib//Lib.class"]
    {
        let jar = Jar::new(&[(name, LIB)]);
        assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::UnsafePath, "{name}");
    }
    let jar = Jar::new(&[("wrong/Lib.class", LIB)]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::NameMismatch);
}
#[test]
fn enforces_all_limits_including_ignored_resources() {
    let jar = Jar::new(&[("lib/Lib.class", LIB), ("ignored", &[0; 4096])]);
    for options in [
        JarOptions { max_archive_bytes: 1, ..JarOptions::default() },
        JarOptions { max_entries: 1, ..JarOptions::default() },
        JarOptions { max_entry_bytes: 1024, ..JarOptions::default() },
        JarOptions { max_total_uncompressed_bytes: 4200, ..JarOptions::default() },
    ] {
        assert_eq!(read_jar(&jar.0, &options).unwrap_err().kind, JarErrorKind::LimitExceeded);
    }
}
#[test]
fn corrupt_crc_and_bad_archives_fail() {
    let mut bytes = archive(&[("lib/Lib.class", LIB)], CompressionMethod::Stored);
    let position = bytes.windows(4).position(|w| w == [0xca, 0xfe, 0xba, 0xbe]).unwrap();
    bytes[position + 30] ^= 1;
    let jar = Jar::bytes(&bytes);
    assert!(read_jar(&jar.0, &JarOptions::default()).is_err());
    let jar = Jar::bytes(b"not a zip");
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Archive);
}
#[test]
fn duplicate_entry_names_fail_instead_of_last_wins() {
    let mut bytes = archive(&[("lib/Lib.class", LIB), ("lib/Lix.class", LIB)], CompressionMethod::Stored);
    for i in 0..bytes.len() - 13 {
        if &bytes[i..i + 13] == b"lib/Lix.class" {
            bytes[i + 6] = b'b';
        }
    }
    let jar = Jar::bytes(&bytes);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::DuplicateEntry);
}
#[test]
fn skips_module_descriptor_and_rejects_malformed_selected_class() {
    let jar = Jar::new(&[("module-info.class", b"not parsed"), ("lib/Lib.class", LIB)]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap().len(), 1);
    let jar = Jar::new(&[("lib/Lib.class", b"bad")]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::ClassFile);
}

#[test]
fn manifest_folding_accepts_split_utf8_and_case_insensitive_attributes() {
    let jar = Jar::new(&[
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\rDescription: \xc3\r\n \xa9\rMuLtI-ReLeAsE: TRUE\r\r"),
        ("META-INF/versions/9/lib/Lib.class", LIB),
    ]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap().len(), 1);
}
#[test]
fn malformed_and_duplicate_main_attributes_fail() {
    for manifest in [b" continuation\n".as_slice(), b"Missing colon\n", b"Multi-Release: true\nmulti-release: false\n"]
    {
        let jar = Jar::new(&[("META-INF/MANIFEST.MF", manifest)]);
        assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Manifest);
    }
}
#[test]
fn malformed_version_directories_and_future_versions_are_not_selected() {
    let jar = Jar::new(&[
        ("META-INF/MANIFEST.MF", b"Multi-Release: true\n\n"),
        ("META-INF/versions/09/lib/Lib.class", b"ignored"),
        ("META-INF/versions/8/lib/Lib.class", b"ignored"),
        ("META-INF/versions/18/lib/Lib.class", b"ignored"),
        ("META-INF/versions/no/lib/Lib.class", b"ignored"),
        ("lib/Lib.class", LIB),
    ]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap().len(), 1);
}
#[test]
fn valid_stored_empty_archives_and_exact_limits_work() {
    let jar = Jar::bytes(&archive(&[("lib/Lib.class", LIB)], CompressionMethod::Stored));
    let options = JarOptions {
        max_archive_bytes: std::fs::metadata(&jar.0).unwrap().len(),
        max_entries: 1,
        max_entry_bytes: LIB.len() as u64,
        max_total_uncompressed_bytes: LIB.len() as u64,
        ..JarOptions::default()
    };
    assert_eq!(read_jar(&jar.0, &options).unwrap().len(), 1);
    let jar = Jar::new(&[]);
    assert!(read_jar(&jar.0, &JarOptions::default()).unwrap().is_empty());
}
#[test]
fn local_and_central_header_disagreement_fails() {
    let mut bytes = archive(&[("lib/Lib.class", LIB)], CompressionMethod::Stored);
    // Change just the local name while retaining the central-directory name.
    let start = bytes.windows(13).position(|w| w == b"lib/Lib.class").unwrap();
    bytes[start] = b'x';
    let jar = Jar::bytes(&bytes);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Archive);
}
#[test]
fn symlinks_encryption_and_unsupported_compression_fail() {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.add_symlink("link", "lib/Lib.class", SimpleFileOptions::default()).unwrap();
    let jar = Jar::bytes(&zip.finish().unwrap().into_inner());
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Unsupported);
    for (local_offset, central_offset, value) in [(6, 8, 1u16), (8, 10, 12u16)] {
        let mut bytes = archive(&[("lib/Lib.class", LIB)], CompressionMethod::Stored);
        let cd = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[local_offset..local_offset + 2].copy_from_slice(&value.to_le_bytes());
        bytes[cd + central_offset..cd + central_offset + 2].copy_from_slice(&value.to_le_bytes());
        let jar = Jar::bytes(&bytes);
        assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Unsupported);
    }
}

#[test]
fn output_order_is_binary_name_order_not_archive_order() {
    let mut other = LIB.to_vec();
    let pos = other.windows(7).position(|w| w == b"lib/Lib").unwrap();
    other[pos + 4] = b'Z';
    let jar = Jar::new(&[("lib/Zib.class", &other), ("lib/Lib.class", LIB)]);
    let classes = read_jar(&jar.0, &JarOptions::default()).unwrap();
    assert_eq!(classes.iter().map(|c| c.binary_name.as_str()).collect::<Vec<_>>(), ["lib/Lib", "lib/Zib"]);
}
#[test]
fn forged_uncompressed_size_never_returns_a_partial_class() {
    let mut bytes = archive(&[("lib/Lib.class", LIB)], CompressionMethod::Deflated);
    let cd = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[22..26].copy_from_slice(&1u32.to_le_bytes());
    bytes[cd + 24..cd + 28].copy_from_slice(&1u32.to_le_bytes());
    let jar = Jar::bytes(&bytes);
    assert!(
        read_jar(&jar.0, &JarOptions { max_entry_bytes: 1, max_total_uncompressed_bytes: 1, ..JarOptions::default() })
            .is_err()
    );
}

#[test]
fn case_insensitive_manifest_lookup_cannot_bypass_policy() {
    let jar = Jar::new(&[("META-INF/manifest.mf", b"Class-Path: hidden.jar\n\n")]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::Unsupported);
    let jar =
        Jar::new(&[("meta-inf/Manifest.mf", b"Multi-Release: true\n\n"), ("META-INF/versions/9/lib/Lib.class", LIB)]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap().len(), 1);
    let jar = Jar::new(&[("META-INF/MANIFEST.MF", b""), ("META-INF/manifest.mf", b"Class-Path: hidden.jar\n\n")]);
    assert_eq!(read_jar(&jar.0, &JarOptions::default()).unwrap_err().kind, JarErrorKind::DuplicateEntry);
}
