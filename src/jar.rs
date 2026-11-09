//! Bounded, native flat-classpath JAR and JMOD loading. Nothing is extracted or fetched.
//!
//! Only stored/DEFLATE ZIP entries and UTF-8 entry names are supported. All
//! entries count toward metadata limits, including ignored resources/versions.
//! Only the JAR manifest and selected classes are decompressed and CRC-checked;
//! this is not a whole-archive integrity or JAR-signature verifier. Module
//! descriptors are skipped, and nonempty JAR manifest Class-Path is an error.
use crate::classfile::{ClassSignature, parse_class_bytes};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
/// Resource bounds for a JMOD 1.0 data input. All sections count toward limits,
/// and `max_archive_bytes` includes the four-byte JMOD header.
#[derive(Clone, Debug)]
pub struct JmodOptions {
    pub max_archive_bytes: u64,
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_uncompressed_bytes: u64,
}
impl Default for JmodOptions {
    fn default() -> Self {
        let limits = JarOptions::default();
        Self {
            max_archive_bytes: limits.max_archive_bytes,
            max_entries: limits.max_entries,
            // Native-library sections in ordinary JDK JMODs can exceed 16 MiB.
            max_entry_bytes: 64 * 1024 * 1024,
            max_total_uncompressed_bytes: limits.max_total_uncompressed_bytes,
        }
    }
}
/// Read a JMOD 1.0 file as a flat platform class input, entirely in Rust.
/// Only `classes/` entries are selected; the module descriptor and `META-INF/`
/// resources are skipped. There is no JPMS export/readability enforcement,
/// multi-release selection, manifest dependency expansion, or JDK discovery.
/// Selected classes are CRC-checked and their internal names must match the
/// entry below `classes/`. Origins retain `path!/classes/actual-entry`.
/// Other sections are validated and bounded but not decompressed or executed.
/// Errors use the same categories as [`read_jar`]; the call is all-or-nothing.
pub fn read_jmod(path: &Path, options: &JmodOptions) -> Result<Vec<ClassSignature>, JarError> {
    Ok(read_jmod_with_usage(path, options)?.classes)
}
pub(crate) fn read_jmod_with_usage(path: &Path, options: &JmodOptions) -> Result<JarRead, JarError> {
    read_archive_with_usage(path, &jmod_limits(options), ArchiveFormat::Jmod)
}
fn jmod_limits(options: &JmodOptions) -> JarOptions {
    JarOptions {
        target_release: 0,
        max_archive_bytes: options.max_archive_bytes,
        max_entries: options.max_entries,
        max_entry_bytes: options.max_entry_bytes,
        max_total_uncompressed_bytes: options.max_total_uncompressed_bytes,
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    Jar,
    Jmod,
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
    pub max_entry_bytes: u64,
}
// Parsed facts are immutable; project ownership and binder contexts are separate.
#[derive(Clone, Debug)]
pub(crate) struct SharedArchiveRead {
    pub classes: Vec<Arc<ClassSignature>>,
    pub entries: usize,
    pub archive_bytes: u64,
    pub uncompressed_bytes: u64,
    max_entry_bytes: u64,
}
impl From<JarRead> for SharedArchiveRead {
    fn from(read: JarRead) -> Self {
        Self {
            classes: read.classes.into_iter().map(Arc::new).collect(),
            entries: read.entries,
            archive_bytes: read.archive_bytes,
            uncompressed_bytes: read.uncompressed_bytes,
            max_entry_bytes: read.max_entry_bytes,
        }
    }
}
impl SharedArchiveRead {
    fn fits(&self, options: &JarOptions) -> bool {
        self.archive_bytes <= options.max_archive_bytes
            && self.entries <= options.max_entries
            && self.max_entry_bytes <= options.max_entry_bytes
            && self.uncompressed_bytes <= options.max_total_uncompressed_bytes
    }
}

struct CachedArchive {
    path: PathBuf,
    format: ArchiveFormat,
    target_release: u16,
    bytes: Box<[u8]>,
    read: SharedArchiveRead,
}

/// Private to one workspace load. Caps cover retained raw bytes/records, not
/// parsed signatures, fresh captures, ZIP metadata, or independently built indexes.
/// FIFO eviction and oversized-input bypass affect work only, never acceptance.
pub(crate) struct ArchiveReads {
    entries: VecDeque<CachedArchive>,
    retained_bytes: usize,
    max_retained_bytes: usize,
    max_entries: usize,
}
impl Default for ArchiveReads {
    fn default() -> Self {
        Self { entries: VecDeque::new(), retained_bytes: 0, max_retained_bytes: 64 * 1024 * 1024, max_entries: 16 }
    }
}
impl ArchiveReads {
    #[cfg(test)]
    pub(crate) fn with_limits(max_retained_bytes: usize, max_entries: usize) -> Self {
        Self { max_retained_bytes, max_entries, ..Self::default() }
    }

    pub(crate) fn read_jar(&mut self, path: &Path, options: &JarOptions) -> Result<SharedArchiveRead, JarError> {
        self.read(path, options, ArchiveFormat::Jar)
    }
    pub(crate) fn read_jmod(&mut self, path: &Path, options: &JmodOptions) -> Result<SharedArchiveRead, JarError> {
        self.read(path, &jmod_limits(options), ArchiveFormat::Jmod)
    }
    // The project loader checks and canonicalizes the path before every call.
    fn read(
        &mut self,
        path: &Path,
        options: &JarOptions,
        format: ArchiveFormat,
    ) -> Result<SharedArchiveRead, JarError> {
        let bytes = capture_archive(path, options.max_archive_bytes)?;
        if let Some(index) = self.entries.iter().position(|entry| {
            entry.path == path && entry.format == format && entry.target_release == options.target_release
        }) {
            let cached = &self.entries[index];
            if cached.bytes.as_ref() == bytes {
                if cached.read.fits(options) {
                    return Ok(cached.read.clone());
                }
                // Let the ordinary parser reject tighter bounds in its original
                // order, including limits on ignored resources and versions.
            } else {
                self.retained_bytes -= self.entries.remove(index).expect("existing entry").bytes.len();
            }
        }
        let read: SharedArchiveRead = parse_archive(path, &bytes, options, format)?.into();
        if bytes.len() <= self.max_retained_bytes && self.max_entries > 0 {
            while self.entries.len() >= self.max_entries || self.retained_bytes > self.max_retained_bytes - bytes.len()
            {
                self.retained_bytes -= self.entries.pop_front().expect("retained entry").bytes.len();
            }
            // Box drops spare Vec capacity so the cap bounds retained raw storage.
            let bytes = bytes.into_boxed_slice();
            self.retained_bytes += bytes.len();
            self.entries.push_back(CachedArchive {
                path: path.to_owned(),
                format,
                target_release: options.target_release,
                bytes,
                read: read.clone(),
            });
        }
        Ok(read)
    }
}

pub(crate) fn read_jar_with_usage(path: &Path, options: &JarOptions) -> Result<JarRead, JarError> {
    read_archive_with_usage(path, options, ArchiveFormat::Jar)
}
fn read_archive_with_usage(path: &Path, options: &JarOptions, format: ArchiveFormat) -> Result<JarRead, JarError> {
    let bytes = capture_archive(path, options.max_archive_bytes)?;
    parse_archive(path, &bytes, options, format)
}

// Every occurrence captures fresh bytes with the original open/metadata/read
// checks, including cache hits. A miss parses these exact bytes, not a reread.
fn capture_archive(path: &Path, max_archive_bytes: u64) -> Result<Vec<u8>, JarError> {
    #[cfg(test)]
    test_support::record_capture(path);
    let origin = path.display().to_string();
    let fail = |kind, detail: &str| error(&origin, kind, detail);
    let file = File::open(path).map_err(|e| error(&origin, JarErrorKind::Io, e))?;
    let metadata = file.metadata().map_err(|e| error(&origin, JarErrorKind::Io, e))?;
    if !metadata.is_file() {
        return Err(fail(JarErrorKind::Unsupported, "archive must be a regular file"));
    }
    if metadata.len() > max_archive_bytes {
        return Err(fail(JarErrorKind::LimitExceeded, "archive byte limit exceeded"));
    }
    // Snapshot with a bounded reader also protects against growth after metadata().
    let mut bytes = Vec::new();
    file.take(max_archive_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| error(&origin, JarErrorKind::Io, e))?;
    if bytes.len() as u64 > max_archive_bytes {
        return Err(fail(JarErrorKind::LimitExceeded, "archive byte limit exceeded"));
    }
    Ok(bytes)
}

fn parse_archive(path: &Path, bytes: &[u8], options: &JarOptions, format: ArchiveFormat) -> Result<JarRead, JarError> {
    #[cfg(test)]
    test_support::record_read(path);
    let origin = path.display().to_string();
    let fail = |kind, detail: &str| error(&origin, kind, detail);
    let archive_bytes = bytes.len() as u64;
    let payload = if format == ArchiveFormat::Jmod {
        if bytes.len() < 4 || !bytes.starts_with(b"JM") {
            return Err(fail(JarErrorKind::Archive, "invalid or truncated JMOD header"));
        }
        if bytes[2..4] != [1, 0] {
            return Err(fail(JarErrorKind::Unsupported, "unsupported JMOD version (expected 1.0)"));
        }
        &bytes[4..]
    } else {
        bytes
    };
    // Exact-pinned zip's iterable API reads EOCD without allocating an entry index.
    let mut preflight = ZipIterable::try_new(Cursor::new(payload), zip::read::Config::default())
        .map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
    if preflight.len() > options.max_entries {
        return Err(fail(JarErrorKind::LimitExceeded, "entry count limit exceeded"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    let mut max_entry_bytes = 0;
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
        max_entry_bytes = max_entry_bytes.max(entry.size());
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
    let mut archive = ZipArchive::new(Cursor::new(payload)).map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
    if archive.has_duplicate_names() {
        return Err(fail(JarErrorKind::DuplicateEntry, "duplicate ZIP entry"));
    }
    archive.validate_header_consistency().map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
    let mut manifest_index = None;
    for index in 0..archive.len() {
        let entry = archive.by_index_data(index).map_err(|e| error(&origin, JarErrorKind::Archive, e))?;
        if format == ArchiveFormat::Jar
            && entry.name_raw().eq_ignore_ascii_case(b"META-INF/MANIFEST.MF")
            && manifest_index.replace(index).is_some()
        {
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
        let (release, logical) = if format == ArchiveFormat::Jmod {
            let Some(logical) = name.strip_prefix("classes/") else {
                continue;
            };
            (0, logical)
        } else if let Some(rest) = name.strip_prefix("META-INF/versions/") {
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
    #[cfg(test)]
    test_support::after_read();
    Ok(JarRead { classes: result, entries: names.len(), archive_bytes, uncompressed_bytes: total, max_entry_bytes })
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
    archive: &mut ZipArchive<Cursor<&[u8]>>,
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

// Thread-local, scoped test observation. Absent from production builds.
#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct Observation {
        paths: Vec<PathBuf>,
        captures: Vec<PathBuf>,
        after_read: Option<Box<dyn FnOnce()>>,
    }
    thread_local! {
        static OBSERVATION: RefCell<Option<Observation>> = const { RefCell::new(None) };
    }
    pub(crate) struct ArchiveProbe;
    impl ArchiveProbe {
        pub(crate) fn new() -> Self {
            OBSERVATION.with(|state| {
                assert!(state.borrow().is_none(), "nested archive probe");
                *state.borrow_mut() = Some(Observation::default());
            });
            Self
        }
        pub(crate) fn paths(&self) -> Vec<PathBuf> {
            OBSERVATION.with(|state| state.borrow().as_ref().unwrap().paths.clone())
        }
        pub(crate) fn captures(&self) -> Vec<PathBuf> {
            OBSERVATION.with(|state| state.borrow().as_ref().unwrap().captures.clone())
        }
        pub(crate) fn after_read(&self, callback: impl FnOnce() + 'static) {
            OBSERVATION.with(|state| state.borrow_mut().as_mut().unwrap().after_read = Some(Box::new(callback)));
        }
    }
    impl Drop for ArchiveProbe {
        fn drop(&mut self) {
            OBSERVATION.with(|state| *state.borrow_mut() = None);
        }
    }
    pub(super) fn record_capture(path: &Path) {
        OBSERVATION.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.captures.push(path.to_owned());
            }
        });
    }
    pub(super) fn record_read(path: &Path) {
        OBSERVATION.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.paths.push(path.to_owned());
            }
        });
    }
    pub(super) fn after_read() {
        let callback = OBSERVATION.with(|state| state.borrow_mut().as_mut().and_then(|state| state.after_read.take()));
        if let Some(callback) = callback {
            callback();
        }
    }
}
