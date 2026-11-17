//! Explicit, version-unverified display attachments. Never analysis inputs.
use super::{
    Error,
    config::{BinaryEntry, Manifest, PlatformInput},
};
use java_engine::{
    GreenElement, GreenNode, LineIndex, ParsedFile, SyntaxKind as K, Target, TargetKind, Utf16Position,
    WorkspaceSnapshot,
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

// Display-only matching: no source type resolution, erasure, or inference.
#[derive(Clone, Copy)]
struct MethodKey<'a> {
    name: &'a str,
    descriptor: &'a str,
    is_static: bool,
    is_varargs: bool,
}

fn binary_method_key<'a>(
    raw: &str,
    methods: &[java_engine::MemberSignature],
    target: &'a Target,
) -> Option<MethodKey<'a>> {
    let identity = target.binary.as_ref()?;
    if target.kind != TargetKind::Method || identity.binary_name != raw || target.name.starts_with('<') {
        return None;
    }
    let mut matching = methods.iter().filter(|m| m.name == target.name && m.descriptor == identity.descriptor);
    let member = matching.next()?;
    if matching.next().is_some()
        || member.kind != java_engine::MemberKind::Method
        || member.signature.is_some()
        || member.raw_access_flags & (0x40 | 0x1000) != 0
    {
        return None;
    }
    Some(MethodKey {
        name: &target.name,
        descriptor: &identity.descriptor,
        is_static: member.raw_access_flags & 0x8 != 0,
        is_varargs: member.raw_access_flags & 0x80 != 0,
    })
}

fn primitive_type(node: &GreenNode, allow_void: bool) -> Option<String> {
    let mut tokens = Vec::new();
    for item in node.children() {
        match item {
            GreenElement::Node(n) if n.kind() == K::Annotation => {}
            GreenElement::Token(t) if t.kind().is_trivia() => {}
            GreenElement::Token(t) => tokens.push(t.kind()),
            _ => return None,
        }
    }
    let (first, rest) = tokens.split_first()?;
    let letter = match first {
        K::BooleanKw => "Z",
        K::Byte => "B",
        K::CharKw => "C",
        K::Short => "S",
        K::IntKw => "I",
        K::LongKw => "J",
        K::FloatKw => "F",
        K::DoubleKw => "D",
        K::Void if allow_void && rest.is_empty() => "V",
        _ => return None,
    };
    let rank = array_rank(rest)?;
    Some(format!("{}{letter}", "[".repeat(rank)))
}

fn array_rank(tokens: &[K]) -> Option<usize> {
    if !tokens.len().is_multiple_of(2)
        || tokens.len() / 2 > 255
        || tokens.chunks_exact(2).any(|p| p != [K::LBracket, K::RBracket])
    {
        return None;
    }
    Some(tokens.len() / 2)
}

fn parameter_descriptor(node: &GreenNode) -> Option<(String, bool)> {
    let mut ty = None;
    let mut tokens = Vec::new();
    for item in node.children() {
        match item {
            GreenElement::Node(n) if n.kind() == K::ModifierList => {
                modifiers(n, true)?;
            }
            GreenElement::Node(n) if n.kind() == K::Type && ty.is_none() => ty = Some(primitive_type(n, false)?),
            GreenElement::Token(t) if t.kind().is_trivia() => {}
            GreenElement::Token(t) if ty.is_some() => tokens.push(t.kind()),
            _ => return None,
        }
    }
    let ty = ty?;
    let varargs = tokens.first() == Some(&K::DotDotDot);
    let tokens = if varargs { &tokens[1..] } else { &tokens[..] };
    let (name, rest) = tokens.split_first()?;
    if *name != K::Ident || (varargs && !rest.is_empty()) {
        return None;
    }
    let added = array_rank(rest)? + usize::from(varargs);
    if ty.bytes().filter(|c| *c == b'[').count() + added > 255 {
        return None;
    }
    Some((format!("{}{ty}", "[".repeat(added)), varargs))
}

fn modifiers(node: &GreenNode, parameter: bool) -> Option<(bool, bool)> {
    let mut seen = Vec::new();
    for item in node.children() {
        match item {
            GreenElement::Node(n) if n.kind() == K::Annotation => {}
            GreenElement::Token(t) if t.kind().is_trivia() => {}
            GreenElement::Token(t) => {
                let k = t.kind();
                if seen.contains(&k)
                    || if parameter {
                        k != K::Final
                    } else {
                        !matches!(
                            k,
                            K::Public | K::Protected | K::Private | K::Static | K::Final | K::Native | K::Abstract
                        )
                    }
                {
                    return None;
                }
                seen.push(k);
            }
            _ => return None,
        }
    }
    if seen.iter().filter(|k| matches!(k, K::Public | K::Protected | K::Private)).count() > 1
        || (seen.contains(&K::Abstract)
            && seen.iter().any(|k| matches!(k, K::Static | K::Final | K::Native | K::Private)))
    {
        return None;
    }
    Some((seen.contains(&K::Static), seen.contains(&K::Native) || seen.contains(&K::Abstract)))
}

fn method_header(node: &GreenNode) -> Option<(String, bool, bool)> {
    let mut result = None;
    let mut params = None;
    let mut is_static = false;
    let mut declaration_only = false;
    let mut is_varargs = false;
    let mut phase = 0;
    for item in node.children() {
        match item {
            GreenElement::Token(t) if t.kind().is_trivia() => {}
            GreenElement::Node(n) if phase == 0 && n.kind() == K::ModifierList => {
                (is_static, declaration_only) = modifiers(n, false)?;
                phase = 1;
            }
            GreenElement::Node(n) if phase <= 1 && n.kind() == K::Type => {
                result = Some(primitive_type(n, true)?);
                phase = 2;
            }
            GreenElement::Token(t) if phase == 2 && t.kind() == K::Ident => phase = 3,
            GreenElement::Node(n) if phase == 3 && n.kind() == K::ParamList => {
                let mut value = String::from("(");
                let mut state = 0;
                for child in n.children() {
                    match child {
                        GreenElement::Token(t) if t.kind().is_trivia() => {}
                        GreenElement::Token(t) if state == 0 && t.kind() == K::LParen => state = 1,
                        GreenElement::Node(param)
                            if matches!(state, 1 | 3) && param.kind() == K::Parameter && !is_varargs =>
                        {
                            let (descriptor, varargs) = parameter_descriptor(param)?;
                            value.push_str(&descriptor);
                            is_varargs = varargs;
                            state = 2;
                        }
                        GreenElement::Token(t) if state == 2 && t.kind() == K::Comma && !is_varargs => state = 3,
                        GreenElement::Token(t) if matches!(state, 1 | 2) && t.kind() == K::RParen => state = 4,
                        _ => return None,
                    }
                }
                if state != 4 {
                    return None;
                }
                value.push(')');
                params = Some(value);
                phase = 4;
            }
            GreenElement::Node(n) if phase == 4 && n.kind() == K::Block && !declaration_only => phase = 5,
            GreenElement::Token(t) if phase == 4 && t.kind() == K::Semi => phase = 5,
            _ => return None,
        }
    }
    if phase != 5 {
        return None;
    }
    Some((format!("{}{}", params?, result?), is_static, is_varargs))
}

fn method_range(owner: &GreenNode, owner_start: u32, key: MethodKey<'_>) -> Option<(u32, u32)> {
    let mut found = None;
    let mut parameter_keys = BTreeSet::new();
    let mut offset = owner_start;
    for item in owner.children() {
        if let GreenElement::Node(body) = item
            && matches!(body.kind(), K::ClassBody | K::InterfaceBody)
        {
            let mut at = offset;
            for child in body.children() {
                if let GreenElement::Node(method) = child
                    && method.kind() == K::Method
                {
                    let mut name_at = at;
                    for part in method.children() {
                        if let Some(token) = part.as_token()
                            && token.kind() == K::Ident
                        {
                            if token.text() == key.name {
                                // Unknown same-name headers cannot silently disappear from the family.
                                let (descriptor, is_static, is_varargs) = method_header(method)?;
                                let parameter_key = descriptor.split_once(')')?.0.to_owned();
                                if !parameter_keys.insert(parameter_key) {
                                    return None;
                                }
                                if descriptor == key.descriptor {
                                    if is_static != key.is_static || is_varargs != key.is_varargs || found.is_some() {
                                        return None;
                                    }
                                    found = Some((name_at, name_at + token.utf8_len()));
                                }
                            }
                            break;
                        }
                        name_at += part.utf8_len();
                    }
                }
                at += child.utf8_len();
            }
        }
        offset += item.utf8_len();
    }
    found
}

fn source_range(text: &str, binary_name: &str, is_interface: bool) -> Option<(Utf16Position, Utf16Position)> {
    source_target_range(text, binary_name, is_interface, None)
}

fn source_target_range(
    text: &str,
    binary_name: &str,
    is_interface: bool,
    method: Option<MethodKey<'_>>,
) -> Option<(Utf16Position, Utf16Position)> {
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
                            candidates.push((at, at + token.utf8_len(), node, offset));
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
    let (start, end, owner, owner_start) = candidates[0];
    let (start, end, name) = if let Some(key) = method {
        let (start, end) = method_range(owner, owner_start, key)?;
        (start, end, key.name)
    } else {
        (start, end, name)
    };
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
    if !matches!(target.kind, TargetKind::Class | TargetKind::Method) || attachments.is_empty() {
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
    let method = if target.kind == TargetKind::Method {
        Some(binary_method_key(raw, &signature.methods, target)?)
    } else {
        None
    };
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
    let (start, end) = if let Some(key) = method {
        source_target_range(text, raw, signature.is_interface, Some(key))?
    } else {
        source_range(text, raw, signature.is_interface)?
    };
    Some((path, start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn method(
        text: &str,
        descriptor: &str,
        is_static: bool,
        is_varargs: bool,
    ) -> Option<(Utf16Position, Utf16Position)> {
        source_target_range(text, "p/A", false, Some(MethodKey { name: "m", descriptor, is_static, is_varargs }))
    }
    #[test]
    fn binary_method_identity_and_metadata_refusals() {
        use java_engine::{Access, BinaryLinkingContext, BinaryTargetIdentity, MemberKind, MemberSignature};
        let target = Target {
            binary: Some(BinaryTargetIdentity {
                binary_name: "p/A".into(),
                descriptor: "(C)Z".into(),
                linking_context: BinaryLinkingContext::Standalone,
            }),
            path: "fixture.jar!/p/A.class".into(),
            kind: TargetKind::Method,
            name: "m".into(),
            owner: "p.A".into(),
            start_byte: 0,
            end_byte: 0,
        };
        let member = MemberSignature {
            raw_access_flags: 0x9,
            name: "m".into(),
            kind: MemberKind::Method,
            descriptor: "(C)Z".into(),
            signature: None,
            access: Access::Public,
        };
        let one = std::slice::from_ref(&member);
        assert!(binary_method_key("p/A", one, &target).unwrap().is_static);
        assert!(binary_method_key("p/B", one, &target).is_none());
        assert!(binary_method_key("p/A", &[], &target).is_none());
        assert!(binary_method_key("p/A", &[member.clone(), member.clone()], &target).is_none());
        for flags in [0x49, 0x1009] {
            assert!(
                binary_method_key("p/A", &[MemberSignature { raw_access_flags: flags, ..member.clone() }], &target)
                    .is_none()
            );
        }
        assert!(
            binary_method_key(
                "p/A",
                &[MemberSignature { signature: Some("(TC;)Z".into()), ..member.clone() }],
                &target
            )
            .is_none()
        );
        assert!(
            binary_method_key("p/A", &[MemberSignature { descriptor: "(I)Z".into(), ..member.clone() }], &target)
                .is_none()
        );
        assert!(
            binary_method_key("p/A", &[MemberSignature { kind: MemberKind::Field, ..member.clone() }], &target)
                .is_none()
        );
        assert!(binary_method_key("p/A", one, &Target { binary: None, ..target.clone() }).is_none());
        assert!(binary_method_key("p/A", one, &Target { name: "<init>".into(), ..target.clone() }).is_none());
        assert!(binary_method_key("p/A", one, &Target { kind: TargetKind::Class, ..target }).is_none());
    }
    #[test]
    fn exact_primitive_methods_arrays_and_native() {
        let text = "// 😀\r\npackage p; class A { static boolean m(char c) { return true; } static boolean m(int c) { return false; } class N { void m() {} } }";
        for (descriptor, needle) in [("(C)Z", "m(char"), ("(I)Z", "m(int")] {
            let (start, end) = method(text, descriptor, true, false).unwrap();
            let index = LineIndex::new(text);
            assert_eq!(start, index.byte_to_position(text.find(needle).unwrap() as u32).unwrap());
            assert_eq!(end.character - start.character, 1);
        }
        assert!(method(text, "()V", false, false).is_none());
        for (head, descriptor, varargs) in [
            ("native int m();", "()I", false),
            ("int[] m(byte x[][]) { return null; }", "([[B)[I", false),
            ("void m(int... x) {}", "([I)V", true),
        ] {
            assert!(
                method(&format!("package p; class A {{ {head} }}"), descriptor, false, varargs).is_some(),
                "{head}"
            );
        }
    }
    #[test]
    fn method_source_family_refusals() {
        assert!(method("package p; class A { int m(int x,) { return 1; } }", "(I)I", false, false).is_none());
        assert!(method("package p; class A { int m(public int x) { return 1; } }", "(I)I", false, false).is_none());
        assert!(method("package p; class A { static static int m() { return 1; } }", "()I", true, false).is_none());
        for body in [
            "int m() { return 1; } int m() { return 2; }",
            "int m() { return 1; } long m() { return 2; }",
            "int m() { return 1; } int m(String s) { return 2; }",
            "int m() { return 1; } <T> int m(int x) { return 2; }",
            "static int m() { return 1; }",
            "class N { int m() { return 1; } }",
            "int m(int x,) { return 1; }",
            "int m() throws int { return 1; }",
            "int m() { return 1; } void m(public int x) {}",
            "public private int m() { return 1; }",
            "public public int m() { return 1; }",
            "abstract int m() { return 1; }",
        ] {
            assert!(method(&format!("package p; class A {{ {body} }}"), "()I", false, false).is_none(), "{body}");
        }
        assert!(method("package p; class A { void m(int[] x) {} }", "([I)V", false, true).is_none());
        assert!(method("package p; class A { void m(int... x) {} }", "([I)V", false, false).is_none());
        assert!(method("package p; class A { int m() { return 1; } }", "()J", false, false).is_none());
        let old = "package p; class A { int m() { return 1; } }";
        let fresh = format!("// 😀\n{old}");
        assert_eq!(method(old, "()I", false, false).unwrap().0.line, 0);
        assert_eq!(method(&fresh, "()I", false, false).unwrap().0.line, 1);
    }
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
