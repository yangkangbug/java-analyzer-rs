//! Bounded, native flat-classpath JAR loading. Nothing is extracted or fetched.
//!
//! Only stored/DEFLATE ZIP entries and UTF-8 entry names are supported. All
//! entries count toward metadata limits, including ignored resources/versions.
//! Only the manifest and selected classes are decompressed and CRC-checked;
//! this is not a whole-archive integrity or JAR-signature verifier. Module
//! descriptors are skipped, and nonempty manifest Class-Path is an error.
use crate::classfile::{ClassSignature, parse_class_bytes};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::unstable::read::ZipIterable;
use zip::{CompressionMethod, ZipArchive};

/// Caller-controlled resource limits, checked before metadata indexing or
/// entry decompression. Limits cover declared sizes of *all* archive entries.
#[derive(Clone, Debug)]
pub struct JarOptions {
    /// Multi-release selection ceiling; releases below 9 use only root entries.
    pub target_release: u16,
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
}
impl Default for JarOptions {
    fn default() -> Self {
        Self {
            target_release: 17,
            max_archive_bytes: 256 * 1024 * 1024,
            max_entries: 100_000,
            max_entry_bytes: 16 * 1024 * 1024,
            max_total_uncompressed_bytes: 512 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JarErrorKind {
    Io,
    Archive,
    LimitExceeded,
    UnsafePath,
    DuplicateEntry,
    NameMismatch,
    Unsupported,
    Manifest,
    ClassFile,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JarError {
    pub origin: String,
    pub kind: JarErrorKind,
    pub detail: String,
}
impl std::fmt::Display for JarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {:?}: {}", self.origin, self.kind, self.detail)
    }
}
impl std::error::Error for JarError {}
fn error(origin: &str, kind: JarErrorKind, detail: impl ToString) -> JarError {
    JarError { origin: origin.into(), kind, detail: detail.to_string() }
}

/// Read classes in logical binary-name order, with origins `path!/actual-entry`.
/// Any error fails the entire call. No dependency discovery, JPMS, JMOD, nested
/// archives, signature verification, extraction, or network access is performed.
/// `Multi-Release: true` in the main manifest section selects the highest release
/// at most `target_release`, including version-only classes. Unselected classes
/// are not parsed. Entry names must match each selected class's internal name.
pub fn read_jar(path: &Path, options: &JarOptions) -> Result<Vec<ClassSignature>, JarError> {
    Ok(read_jar_with_usage(path, options)?.classes)
}
pub(crate) struct JarRead {
    pub classes: Vec<ClassSignature>,
    pub entries: usize,
    pub archive_bytes: u64,
    pub uncompressed_bytes: u64,
}
pub(crate) fn read_jar_with_usage(path: &Path, options: &JarOptions) -> Result<JarRead, JarError> {
    let origin = path.display().to_string();
    let fail = |kind, detail: &str| error(&origin, kind, detail);
    let file = File::open(path).map_err(|e| error(&origin, JarErrorKind::Io, e))?;
    let metadata = file.metadata().map_err(|e| error(&origin, JarErrorKind::Io, e))?;
    if !metadata.is_file() {
        return Err(fail(JarErrorKind::Unsupported, "archive must be a regular file"));
    }
    if metadata.len() > options.max_archive_bytes {
        return Err(fail(JarErrorKind::LimitExceeded, "archive byte limit exceeded"));
    }
    // Snapshot with a bounded reader also protects against growth after metadata().
    let mut bytes = Vec::new();
    file.take(options.max_archive_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| error(&origin, JarErrorKind::Io, e))?;
    if bytes.len() as u64 > options.max_archive_bytes {
        return Err(fail(JarErrorKind::LimitExceeded, "archive byte limit exceeded"));
    }
    // Exact-pinned zip's iterable API reads EOCD without allocating an entry index.
    let mut preflight = ZipIterable::try_new(Cursor::new(&bytes), zip::read::Config::default())
        .map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
    if preflight.len() > options.max_entries {
        return Err(fail(JarErrorKind::LimitExceeded, "entry count limit exceeded"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for entry in preflight.files().map_err(|e| error(&origin, JarErrorKind::Archive, e))? {
        let entry = entry.map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
        let name = std::str::from_utf8(entry.name_raw()).map_err(|e| error(&origin, JarErrorKind::UnsafePath, e))?;
        validate_path(name).map_err(|detail| fail(JarErrorKind::UnsafePath, detail))?;
        if !names.insert(name.to_owned()) {
            return Err(fail(JarErrorKind::DuplicateEntry, name));
        }
        if names.len() > options.max_entries {
            return Err(fail(JarErrorKind::LimitExceeded, "entry count limit exceeded"));
        }
        if entry.size() > options.max_entry_bytes {
            return Err(fail(JarErrorKind::LimitExceeded, "entry byte limit exceeded"));
        }
        total =
            total.checked_add(entry.size()).ok_or_else(|| fail(JarErrorKind::LimitExceeded, "total size overflow"))?;
        if total > options.max_total_uncompressed_bytes {
            return Err(fail(JarErrorKind::LimitExceeded, "total uncompressed byte limit exceeded"));
        }
        if entry.encrypted()
            || entry.is_symlink()
            || !matches!(entry.compression(), CompressionMethod::Stored | CompressionMethod::Deflated)
        {
            return Err(fail(JarErrorKind::Unsupported, "encrypted, symlink, or unsupported compression entry"));
        }
    }
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
    if archive.has_duplicate_names() {
        return Err(fail(JarErrorKind::DuplicateEntry, "duplicate ZIP entry"));
    }
    archive.validate_header_consistency().map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
    let mut manifest_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index_data(index).map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
        if entry.name_raw().eq_ignore_ascii_case(b"META-INF/MANIFEST.MF") && manifest_index.replace(index).is_some() {
            return Err(fail(JarErrorKind::DuplicateEntry, "multiple case-insensitive manifest entries"));
        }
    }
    let multi_release = if let Some(index) = manifest_index {
        let manifest = read_entry(&mut archive, index, &origin, options)?;
        parse_manifest(&manifest).map_err(|(kind, detail)| fail(kind, &detail))?
    } else {
        false
    };
    let mut selected = BTreeMap::<String, (u16, usize, String)>::new();
    for index in 0..archive.len() {
        let entry = archive.by_index_data(index).map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
        let name = std::str::from_utf8(entry.name_raw()).map_err(|e| error(&origin, JarErrorKind::UnsafePath, e))?;
        let (release, logical) = if let Some(rest) = name.strip_prefix("META-INF/versions/") {
            if !multi_release || options.target_release < 9 {
                continue;
            }
            let Some((version, logical)) = rest.split_once('/') else {
                continue;
            };
            let Ok(release) = version.parse::<u16>() else {
                continue;
            };
            if release < 9 || release > options.target_release || version != release.to_string() {
                continue;
            }
            (release, logical)
        } else {
            (0, name)
        };
        if logical.starts_with("META-INF/") || logical == "module-info.class" || !logical.ends_with(".class") {
            continue;
        }
        if selected.get(logical).is_none_or(|(current, _, _)| release > *current) {
            selected.insert(logical.to_owned(), (release, index, name.to_owned()));
        }
    }
    let mut result = Vec::new();
    for (logical, (_, index, actual)) in selected {
        let entry_origin = format!("{origin}!/{actual}");
        let bytes = read_entry(&mut archive, index, &entry_origin, options)?;
        let signature =
            parse_class_bytes(&entry_origin, &bytes).map_err(|e| error(&entry_origin, JarErrorKind::ClassFile, e))?;
        if logical.strip_suffix(".class") != Some(signature.binary_name.as_str()) {
            return Err(error(
                &entry_origin,
                JarErrorKind::NameMismatch,
                format!("entry does not match internal name {}", signature.binary_name),
            ));
        }
        result.push(signature);
    }
    Ok(JarRead { classes: result, entries: names.len(), archive_bytes: bytes.len() as u64, uncompressed_bytes: total })
}
fn validate_path(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.contains(['\\', ':', '\0']) || name.chars().any(char::is_control) {
        return Err("invalid entry path");
    }
    let path = name.strip_suffix('/').unwrap_or(name);
    if path.split('/').any(|part| matches!(part, "" | "." | "..")) {
        return Err("absolute or noncanonical entry path");
    }
    Ok(())
}
fn read_entry(
    archive: &mut ZipArchive<Cursor<&Vec<u8>>>,
    index: usize,
    origin: &str,
    options: &JarOptions,
) -> Result<Vec<u8>, JarError> {
    let mut entry = archive.by_index(index).map_err(|e| error(origin, JarErrorKind::Archive, e))?;
    let declared = entry.size();
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = entry.read(&mut chunk).map_err(|e| error(origin, JarErrorKind::Archive, e))?;
        if n == 0 {
            break;
        }
        let size = (out.len() as u64)
            .checked_add(n as u64)
            .ok_or_else(|| error(origin, JarErrorKind::LimitExceeded, "entry size overflow"))?;
        if size > options.max_entry_bytes || size > declared {
            return Err(error(origin, JarErrorKind::LimitExceeded, "actual entry size exceeds bound"));
        }
        out.extend_from_slice(&chunk[..n]);
    }
    if out.len() as u64 != declared {
        return Err(error(origin, JarErrorKind::Archive, "entry size mismatch"));
    }
    Ok(out)
}
fn parse_manifest(bytes: &[u8]) -> Result<bool, (JarErrorKind, String)> {
    let invalid = |detail: &str| (JarErrorKind::Manifest, detail.to_owned());
    let mut unfolded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let width = if bytes[i..].starts_with(b"\r\n") {
            2
        } else if matches!(bytes[i], b'\r' | b'\n') {
            1
        } else {
            0
        };
        if width > 0 && bytes.get(i + width) == Some(&b' ') {
            // Keep an orphan leading continuation visible to the parser.
            if !unfolded.is_empty() && unfolded.last() != Some(&b'\n') {
                i += width + 1;
                continue;
            }
        }
        if width > 0 {
            unfolded.push(b'\n');
            i += width;
        } else {
            unfolded.push(bytes[i]);
            i += 1;
        }
    }
    let text = std::str::from_utf8(&unfolded).map_err(|_| invalid("manifest is not UTF-8"))?;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut keys = BTreeSet::new();
    // Line endings were normalized while unfolding raw UTF-8 bytes.
    for line in text.split('\n') {
        if line.is_empty() {
            break;
        }
        if let Some(continuation) = line.strip_prefix(' ') {
            let Some((_, value)) = headers.last_mut() else {
                return Err(invalid("orphan manifest continuation"));
            };
            value.push_str(continuation);
        } else {
            let Some((key, value)) = line.split_once(": ") else {
                return Err(invalid("malformed manifest header"));
            };
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                return Err(invalid("invalid manifest attribute name"));
            }
            if !keys.insert(key.to_ascii_lowercase()) {
                return Err(invalid("duplicate main manifest attribute"));
            }
            headers.push((key.to_owned(), value.to_owned()));
        }
    }
    let mut multi_release = false;
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("Class-Path") && !value.trim().is_empty() {
            return Err((
                JarErrorKind::Unsupported,
                "manifest Class-Path requires explicit caller-supplied dependencies".into(),
            ));
        }
        if key.eq_ignore_ascii_case("Multi-Release") {
            multi_release = value.eq_ignore_ascii_case("true");
        }
    }
    Ok(multi_release)
}
