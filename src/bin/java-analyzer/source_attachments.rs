//! Explicit, version-unverified display attachments. Never analysis inputs.
use super::{
    Error,
    config::{BinaryEntry, Manifest, PlatformInput},
};
use java_engine::{
    GreenElement, LineIndex, ParsedFile, SyntaxKind as K, Target, TargetKind, Utf16Position, WorkspaceSnapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_ATTACHMENTS: usize = 64;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_PATH: usize = 4096;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub binary_input: String,
    pub source_root: String,
    pub association: String,
}

fn invalid() -> Error {
    Error::new("invalid_configuration", "invalid or ambiguous explicit source attachment")
}

// Reject symlinks along the supplied path, not just its canonical destination.
// This is local path hygiene, not an atomic hostile-filesystem sandbox.
fn checked_path(base: &Path, value: &str, directory: bool) -> Option<PathBuf> {
    if value.is_empty() || value.len() > MAX_PATH || value.contains('\0') {
        return None;
    }
    let path = base.join(value);
    if !path.is_absolute() || path.to_str()?.len() > MAX_PATH {
        return None;
    }
    let mut cursor = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::Prefix(_)) {
            return None;
        }
        cursor.push(component);
        if fs::symlink_metadata(&cursor).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    let canonical = path.canonicalize().ok()?;
    if canonical.to_str()?.len() > MAX_PATH {
        return None;
    }
    let metadata = fs::metadata(&canonical).ok()?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return None;
    }
    Some(canonical)
}

fn configured_archives(manifest: &Manifest, base: &Path) -> Option<BTreeMap<PathBuf, bool>> {
    let mut paths = BTreeMap::new();
    let mut add = |path: &str, jmod: bool| -> Option<()> {
        let path = super::config::local_path(base, path).ok()?.canonicalize().ok()?;
        if paths.insert(path, jmod).is_some_and(|old| old != jmod) {
            return None;
        }
        Some(())
    };
    for compilation in &manifest.compilations {
        for entry in &compilation.classpath {
            if let BinaryEntry::Jar { path } = entry {
                add(path, false)?;
            }
        }
        for entry in &compilation.platform {
            match entry {
                PlatformInput::Jar { path } => add(path, false)?,
                PlatformInput::Jmod { path } => add(path, true)?,
            }
        }
    }
    Some(paths)
}

fn entry_matches(entry: &str, raw: &str, jmod: bool) -> bool {
    let expected = format!("{raw}.class");
    if jmod {
        return entry.strip_prefix("classes/") == Some(expected.as_str());
    }
    if entry == expected {
        return true;
    }
    let Some((version, logical)) = entry.strip_prefix("META-INF/versions/").and_then(|s| s.split_once('/')) else {
        return false;
    };
    let Ok(release) = version.parse::<u16>() else {
        return false;
    };
    release >= 9 && version == release.to_string() && logical == expected
}

fn associations(attachments: &[Attachment], manifest: &Manifest, base: &Path) -> Option<Vec<(PathBuf, PathBuf, bool)>> {
    if attachments.len() > MAX_ATTACHMENTS {
        return None;
    }
    if attachments.is_empty() {
        return Some(Vec::new());
    }
    let configured = configured_archives(manifest, base)?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for attachment in attachments {
        if attachment.association != "explicit_unverified" {
            return None;
        }
        let binary = checked_path(base, &attachment.binary_input, false)?;
        // Archive origins use this delimiter. Reject ambiguous literal delimiters.
        if binary.to_str()?.contains("!/") || !configured.contains_key(&binary) || !seen.insert(binary.clone()) {
            return None;
        }
        let root = checked_path(base, &attachment.source_root, true)?;
        let jmod = *configured.get(&binary)?;
        result.push((binary, root, jmod));
    }
    Some(result)
}

pub fn validate(attachments: &[Attachment], manifest: &Manifest, base: &Path) -> Result<(), Error> {
    associations(attachments, manifest, base).map(|_| ()).ok_or_else(invalid)
}

fn source_range(text: &str, binary_name: &str, is_interface: bool) -> Option<(Utf16Position, Utf16Position)> {
    if binary_name.contains(['$', '\\', '\0']) {
        return None;
    }
    let mut parts: Vec<_> = binary_name.split('/').collect();
    if parts.iter().any(|part| part.is_empty() || *part == "." || *part == "..") {
        return None;
    }
    let name = parts.pop()?;
    let package = parts.join(".");
    let parsed = ParsedFile::parse(text);
    if !parsed.errors().is_empty() {
        return None;
    }
    let mut found_package = None;
    let mut candidates = Vec::new();
    let mut offset = 0u32;
    for element in parsed.root().children() {
        if let GreenElement::Node(node) = element {
            if node.kind() == K::PackageDecl {
                if found_package.is_some() {
                    return None;
                }
                let qualified =
                    node.children().iter().filter_map(GreenElement::as_node).find(|n| n.kind() == K::Name)?;
                let mut spelling = String::new();
                for token in qualified.children() {
                    let token = token.as_token()?;
                    if !token.kind().is_trivia() {
                        spelling.push_str(token.text());
                    }
                }
                found_package = Some(spelling);
            }
            if matches!(node.kind(), K::Class | K::Interface | K::Enum | K::Record) {
                let mut at = offset;
                for item in node.children() {
                    if let Some(token) = item.as_token()
                        && token.kind() == K::Ident
                    {
                        if token.text() == name {
                            let expected = if is_interface { K::Interface } else { K::Class };
                            if node.kind() != expected {
                                return None;
                            }
                            candidates.push((at, at + token.utf8_len()));
                        }
                        break;
                    }
                    at += item.utf8_len();
                }
            }
        }
        offset += element.utf8_len();
    }
    if found_package.unwrap_or_default() != package || candidates.len() != 1 {
        return None;
    }
    let (start, end) = candidates[0];
    if text.get(start as usize..end as usize)? != name {
        return None;
    }
    let index = LineIndex::new(text);
    Some((index.byte_to_position(start)?, index.byte_to_position(end)?))
}

pub fn locate(
    attachments: &[Attachment],
    manifest: &Manifest,
    base: &Path,
    workspace: &WorkspaceSnapshot,
    target: &Target,
) -> Option<(PathBuf, Utf16Position, Utf16Position)> {
    if target.kind != TargetKind::Class || attachments.is_empty() {
        return None;
    }
    let mut signature = None;
    for id in workspace.compilation_ids() {
        let project = workspace.scope(id)?.inputs().project();
        if let Some(current) = project.class_signature(&target.path) {
            if signature.is_some_and(|previous| previous != current) {
                return None;
            }
            signature = Some(current);
        }
    }
    let signature = signature?;
    let raw = &signature.binary_name;
    if raw.contains(['$', '\\', '\0']) || raw.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return None;
    }
    let mut matching = Vec::new();
    for (binary, root, jmod) in associations(attachments, manifest, base)? {
        let prefix = format!("{}!/", binary.to_str()?);
        if let Some(entry) = target.path.strip_prefix(&prefix) {
            if !entry_matches(entry, raw, jmod)
                || entry.is_empty()
                || entry.split('/').any(|s| s.is_empty() || s == "." || s == "..")
            {
                return None;
            }
            matching.push(root);
        }
    }
    if matching.len() != 1 {
        return None;
    }
    let root = &matching[0];
    let path = checked_path(root, &format!("{raw}.java"), false)?;
    if !path.starts_with(root) {
        return None;
    }
    let mut file = File::open(&path).ok()?;
    if !file.metadata().ok()?.is_file() || file.metadata().ok()?.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    (&mut file).take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return None;
    }
    let text = std::str::from_utf8(&bytes).ok()?;
    let (start, end) = source_range(text, raw, signature.is_interface)?;
    Some((path, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_top_level_name_and_utf16() {
        let text = "// 😀\r\npackage p; public interface List<T> { T get(); }";
        let (start, end) = source_range(text, "p/List", true).unwrap();
        assert_eq!(start.line, 1);
        assert_eq!(end.character - start.character, 4);
        assert!(source_range(text, "q/List", true).is_none());
        assert!(source_range(text, "p/List", false).is_none());
    }
    #[test]
    fn refuses_ambiguous_unsupported_or_invalid_source() {
        for text in [
            "class A {} class A {}",
            "class C { class A {} }",
            "enum A { X }",
            "record A() {}",
            "@interface A {}",
            "class A {",
        ] {
            assert!(source_range(text, "A", false).is_none(), "{text}");
        }
        assert!(source_range("@interface A {}", "A", true).is_none());
        assert!(source_range("class A$B {}", "A$B", false).is_none());
        assert!(source_range("class A {}", "../A", false).is_none());
        assert!(source_range("class A {}", "/A", false).is_none());
    }
    #[test]
    fn validates_explicit_configured_unique_associations() {
        let root = std::env::temp_dir().join(format!("java-attachment-config-{}", std::process::id()));
        fs::create_dir_all(root.join("sources")).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("lib.jar"), b"fixture identity only").unwrap();
        fs::write(root.join("other.jar"), b"not configured").unwrap();
        let mut manifest: Manifest = serde_json::from_value(serde_json::json!({
            "schema_version": 1,
            "compilations": [{"id":{"project":"p","source_set":"main"},"source_roots":[],
                "classpath":[{"kind":"jar","path":"lib.jar"}]}]
        }))
        .unwrap();
        let attachment = Attachment {
            binary_input: "lib.jar".into(),
            source_root: "sources".into(),
            association: "explicit_unverified".into(),
        };
        assert!(validate(&[], &manifest, &root).is_ok());
        assert!(validate(std::slice::from_ref(&attachment), &manifest, &root).is_ok());
        assert!(validate(&[attachment.clone(), attachment.clone()], &manifest, &root).is_err());
        assert!(validate(&vec![attachment.clone(); MAX_ATTACHMENTS + 1], &manifest, &root).is_err());
        for changed in [
            Attachment { association: "verified".into(), ..attachment.clone() },
            Attachment { binary_input: "other.jar".into(), ..attachment.clone() },
            Attachment { source_root: "lib.jar".into(), ..attachment.clone() },
        ] {
            assert!(validate(&[changed], &manifest, &root).is_err());
        }
        manifest.compilations[0].platform.push(PlatformInput::Jmod { path: "lib.jar".into() });
        assert!(validate(&[attachment], &manifest, &root).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn archive_entry_requires_exact_raw_name_and_format() {
        for entry in ["p/A.class", "META-INF/versions/21/p/A.class"] {
            assert!(entry_matches(entry, "p/A", false));
        }
        assert!(entry_matches("classes/p/A.class", "p/A", true));
        for entry in [
            "p/B.class",
            "p/A.java",
            "classes/p/A.class",
            "META-INF/versions/08/p/A.class",
            "META-INF/versions/8/p/A.class",
            "META-INF/versions/21/p/A.class/extra",
        ] {
            assert!(!entry_matches(entry, "p/A", false));
        }
        assert!(!entry_matches("p/A.class", "p/A", true));
    }
    #[test]
    fn filesystem_checks_reject_symlinks_and_parent_segments() {
        let root = std::env::temp_dir().join(format!("java-attachment-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("A.java"), "class A {}").unwrap();
        assert!(checked_path(&root, "A.java", false).is_some());
        assert!(checked_path(&root, "A.java", true).is_none());
        assert!(checked_path(&root, "../A.java", false).is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("A.java"), root.join("link.java")).unwrap();
            assert!(checked_path(&root, "link.java", false).is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
