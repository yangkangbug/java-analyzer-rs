//! Owned view of a Java class file.
//!
//! Parsing is `cafebabe` 0.9.0 (0BSD, `forbid(unsafe_code)`). This module does
//! not call a JVM. IntelliJ's compiled PSI is not reused: at commit
//! `840d3be11dfdeada533d05052008b48816b6cc4e`, `StubBuildingVisitor` extends
//! ASM `ClassVisitor` and builds stubs that `ClsClassImpl` then exposes as
//! `PsiClass`. The binder in this crate consumes [`ClassSignature`] the same
//! way it consumes a parsed source type. That is a shared index, not a port
//! of ASM or of the stub builder.

use std::fmt;
use std::fs;
use std::path::Path;

use cafebabe::attributes::AttributeData;

/// Major versions this reader accepts: 45 (Java 1.1) through 71 (Java 27).
const MIN_MAJOR: u16 = 45;
const MAX_MAJOR: u16 = 71;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Public,
    Protected,
    Private,
    /// No access flag. Same package only.
    Package,
    /// More than one of public, protected, and private.
    Unspecified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Field,
    Method,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSignature {
    /// Original JVMS member access bits; interpretation depends on member kind.
    pub raw_access_flags: u16,
    pub name: String,
    pub kind: MemberKind,
    /// JVM field or method descriptor, such as `I` or `()V`.
    pub descriptor: String,
    /// Raw `Signature` attribute. Generic types are not instantiated.
    pub signature: Option<String>,
    pub access: Access,
}

/// One successfully parsed class file and the path it was registered under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassSignature {
    pub origin: String,
    /// Internal name with `/`, such as `pkg/Outer$Inner`.
    pub binary_name: String,
    /// `binary_name` with `/` and `$` turned into `.`.
    ///
    /// `$` is treated as javac's nesting separator. A binary name that uses
    /// `$` for another reason is misread.
    pub dotted_name: String,
    pub package: String,
    pub nesting: Vec<String>,
    pub super_class: Option<String>,
    pub interfaces: Vec<String>,
    pub signature: Option<String>,
    pub major: u16,
    pub minor: u16,
    /// True for ACC_INTERFACE. Its class-file Object superclass is not a
    /// Java class-inheritance edge for method overriding.
    pub is_interface: bool,
    /// True for ACC_FINAL. Source classes cannot extend this binary class.
    pub is_final: bool,
    pub access: Access,
    pub fields: Vec<MemberSignature>,
    pub methods: Vec<MemberSignature>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassErrorKind {
    Truncated,
    BadMagic,
    BadConstantPool,
    UnsupportedVersion,
    Io,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassError {
    pub origin: String,
    pub kind: ClassErrorKind,
    pub detail: String,
}

impl fmt::Display for ClassError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}: {}", self.origin, self.kind, self.detail)
    }
}

impl std::error::Error for ClassError {}

/// Parse `bytes` as a class file. Truncation, a bad magic, a bad constant
/// pool, and a version outside 45..=71 are errors. This does not panic on
/// those inputs.
pub fn parse_class_bytes(origin: &str, bytes: &[u8]) -> Result<ClassSignature, ClassError> {
    let mut options = cafebabe::ParseOptions::default();
    options.parse_bytecode(false);
    let class =
        cafebabe::parse_class_with_options(bytes, &options).map_err(|err| classify(origin, &err.to_string()))?;
    if class.major_version < MIN_MAJOR || class.major_version > MAX_MAJOR {
        return Err(ClassError {
            origin: origin.to_string(),
            kind: ClassErrorKind::UnsupportedVersion,
            detail: format!("major {} minor {}", class.major_version, class.minor_version),
        });
    }
    if class.major_version >= 56 && class.minor_version != 0 && class.minor_version != 65535 {
        return Err(ClassError {
            origin: origin.to_string(),
            kind: ClassErrorKind::UnsupportedVersion,
            detail: format!("major {} minor {}", class.major_version, class.minor_version),
        });
    }
    let (field_flags, method_flags) = raw_member_flags(bytes)
        .filter(|(fields, methods)| fields.len() == class.fields.len() && methods.len() == class.methods.len())
        .ok_or_else(|| ClassError {
            origin: origin.to_owned(),
            kind: ClassErrorKind::Other,
            detail: "raw member flag inventory does not match parsed class".into(),
        })?;
    Ok(signature_from(origin, &class, &field_flags, &method_flags))
}

/// Read every `*.class` file under `root`. A parse error or an I/O error
/// fails the whole call; nothing from this call is returned. Symlinks are
/// not followed. This does not open JARs.
pub(crate) fn read_class_directory(root: &Path) -> Result<Vec<ClassSignature>, ClassError> {
    let mut out = Vec::new();
    walk_class_dir(root, &mut out)?;
    Ok(out)
}

fn walk_class_dir(dir: &Path, out: &mut Vec<ClassSignature>) -> Result<(), ClassError> {
    let entries = fs::read_dir(dir).map_err(|err| ClassError {
        origin: dir.display().to_string(),
        kind: ClassErrorKind::Io,
        detail: err.to_string(),
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| ClassError {
            origin: dir.display().to_string(),
            kind: ClassErrorKind::Io,
            detail: err.to_string(),
        })?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|err| ClassError {
            origin: path.display().to_string(),
            kind: ClassErrorKind::Io,
            detail: err.to_string(),
        })?;
        if kind.is_dir() {
            walk_class_dir(&path, out)?;
            continue;
        }
        if !kind.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".class") {
            continue;
        }
        let bytes = fs::read(&path).map_err(|err| ClassError {
            origin: path.display().to_string(),
            kind: ClassErrorKind::Io,
            detail: err.to_string(),
        })?;
        out.push(parse_class_bytes(&path.display().to_string(), &bytes)?);
    }
    Ok(())
}

// cafebabe intentionally truncates unknown access bits. Walk only the already
// validated binary framing to retain original member u16 flags; never interpret
// constant-pool names, descriptors, bytecode, or attribute payloads here.
fn raw_member_flags(bytes: &[u8]) -> Option<(Vec<u16>, Vec<u16>)> {
    struct Cursor<'a> {
        bytes: &'a [u8],
        at: usize,
    }
    impl Cursor<'_> {
        fn skip(&mut self, count: usize) -> Option<()> {
            let end = self.at.checked_add(count)?;
            self.bytes.get(self.at..end)?;
            self.at = end;
            Some(())
        }
        fn u1(&mut self) -> Option<u8> {
            let value = *self.bytes.get(self.at)?;
            self.skip(1)?;
            Some(value)
        }
        fn u2(&mut self) -> Option<u16> {
            Some(u16::from_be_bytes([self.u1()?, self.u1()?]))
        }
        fn u4(&mut self) -> Option<u32> {
            Some(u32::from_be_bytes([self.u1()?, self.u1()?, self.u1()?, self.u1()?]))
        }
        fn members(&mut self) -> Option<Vec<u16>> {
            let count = self.u2()?;
            let mut flags = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                flags.push(self.u2()?);
                self.skip(4)?; // name_index and descriptor_index
                let attributes = self.u2()?;
                for _ in 0..attributes {
                    self.skip(2)?;
                    let length = usize::try_from(self.u4()?).ok()?;
                    self.skip(length)?;
                }
            }
            Some(flags)
        }
    }
    let mut cursor = Cursor { bytes, at: 0 };
    cursor.skip(8)?;
    let count = cursor.u2()?;
    let mut index = 1u32;
    while index < u32::from(count) {
        match cursor.u1()? {
            1 => {
                let length = usize::from(cursor.u2()?);
                cursor.skip(length)?;
            }
            3 | 4 | 9 | 10 | 11 | 12 | 17 | 18 => cursor.skip(4)?,
            5 | 6 => {
                cursor.skip(8)?;
                index += 1;
            }
            7 | 8 | 16 | 19 | 20 => cursor.skip(2)?,
            15 => cursor.skip(3)?,
            _ => return None,
        }
        index += 1;
    }
    if index != u32::from(count) {
        return None;
    }
    cursor.skip(6)?;
    let interfaces = usize::from(cursor.u2()?);
    cursor.skip(interfaces.checked_mul(2)?)?;
    Some((cursor.members()?, cursor.members()?))
}

fn signature_from(
    origin: &str,
    class: &cafebabe::ClassFile<'_>,
    field_flags: &[u16],
    method_flags: &[u16],
) -> ClassSignature {
    let binary_name = class.this_class.to_string();
    let (package, nesting) = split_binary(&binary_name);
    let dotted_name = dotted_binary(&binary_name);
    ClassSignature {
        origin: origin.to_string(),
        binary_name,
        dotted_name,
        package,
        nesting,
        super_class: class.super_class.as_ref().map(|name| dotted_binary(name)),
        interfaces: class.interfaces.iter().map(|name| dotted_binary(name)).collect(),
        signature: signature_attr(&class.attributes),
        major: class.major_version,
        minor: class.minor_version,
        is_interface: class.access_flags.contains(cafebabe::ClassAccessFlags::INTERFACE),
        is_final: class.access_flags.contains(cafebabe::ClassAccessFlags::FINAL),
        access: class_access(class.access_flags),
        fields: class
            .fields
            .iter()
            .zip(field_flags)
            .map(|(field, raw_flags)| MemberSignature {
                raw_access_flags: *raw_flags,
                name: field.name.to_string(),
                kind: MemberKind::Field,
                descriptor: field.descriptor.to_string(),
                signature: signature_attr(&field.attributes),
                access: field_access(field.access_flags),
            })
            .collect(),
        methods: class
            .methods
            .iter()
            .zip(method_flags)
            .filter(|(method, _)| method.name != "<clinit>")
            .map(|(method, raw_flags)| MemberSignature {
                raw_access_flags: *raw_flags,
                name: method.name.to_string(),
                kind: MemberKind::Method,
                descriptor: method.descriptor.to_string(),
                signature: signature_attr(&method.attributes),
                access: method_access(method.access_flags),
            })
            .collect(),
    }
}

fn signature_attr(attributes: &[cafebabe::attributes::AttributeInfo<'_>]) -> Option<String> {
    attributes.iter().find_map(|attr| match &attr.data {
        AttributeData::Signature(text) => Some(text.to_string()),
        _ => None,
    })
}

fn split_binary(binary: &str) -> (String, Vec<String>) {
    let (package, simple) = match binary.rfind('/') {
        Some(index) => (binary[..index].replace('/', "."), &binary[index + 1..]),
        None => (String::new(), binary),
    };
    let nesting = simple.split('$').filter(|part| !part.is_empty()).map(str::to_string).collect();
    (package, nesting)
}

pub(crate) fn dotted_binary(binary: &str) -> String {
    binary.replace(['/', '$'], ".")
}

fn class_access(flags: cafebabe::ClassAccessFlags) -> Access {
    // `ClassAccessFlags` does not name PRIVATE or PROTECTED. Nested classes
    // still store those bits in the same u16.
    let bits = flags.bits();
    one_access(bits & 0x0001 != 0, bits & 0x0004 != 0, bits & 0x0002 != 0)
}

fn field_access(flags: cafebabe::FieldAccessFlags) -> Access {
    one_access(
        flags.contains(cafebabe::FieldAccessFlags::PUBLIC),
        flags.contains(cafebabe::FieldAccessFlags::PROTECTED),
        flags.contains(cafebabe::FieldAccessFlags::PRIVATE),
    )
}

fn method_access(flags: cafebabe::MethodAccessFlags) -> Access {
    one_access(
        flags.contains(cafebabe::MethodAccessFlags::PUBLIC),
        flags.contains(cafebabe::MethodAccessFlags::PROTECTED),
        flags.contains(cafebabe::MethodAccessFlags::PRIVATE),
    )
}

fn one_access(public: bool, protected: bool, private: bool) -> Access {
    match (public, protected, private) {
        (true, false, false) => Access::Public,
        (false, true, false) => Access::Protected,
        (false, false, true) => Access::Private,
        (false, false, false) => Access::Package,
        _ => Access::Unspecified,
    }
}

fn classify(origin: &str, detail: &str) -> ClassError {
    let kind = if detail.contains("end of stream") {
        ClassErrorKind::Truncated
    } else if detail.contains("magic") {
        ClassErrorKind::BadMagic
    } else if detail.contains("constant pool")
        || detail.contains("Constant pool")
        || detail.contains("cp ")
        || detail.contains("tag")
    {
        ClassErrorKind::BadConstantPool
    } else if detail.contains("version") {
        ClassErrorKind::UnsupportedVersion
    } else {
        ClassErrorKind::Other
    };
    ClassError { origin: origin.to_string(), kind, detail: detail.to_string() }
}

#[cfg(test)]
mod raw_flags_tests {
    use super::raw_member_flags;
    fn framing() -> Vec<u8> {
        // Framing-only unit fixture: the semantic parser remains cafebabe.
        let mut bytes = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 61, 0, 5];
        bytes.push(5);
        bytes.extend([0; 8]); // long occupies slots 1 and 2
        bytes.push(6);
        bytes.extend([0; 8]); // double occupies slots 3 and 4
        bytes.extend([0, 0, 0, 0, 0, 0, 0, 0]); // access/this/super/interfaces
        for flags in [0x2001u16, 0x4001u16] {
            bytes.extend(1u16.to_be_bytes()); // one member
            bytes.extend(flags.to_be_bytes());
            bytes.extend([0; 4]);
            bytes.extend(2u16.to_be_bytes());
            for payload in [&b"abc"[..], &b""[..]] {
                bytes.extend([0, 0]);
                bytes.extend((payload.len() as u32).to_be_bytes());
                bytes.extend(payload);
            }
        }
        bytes
    }
    #[test]
    fn raw_flags_checked_framing_handles_double_slots_and_attributes() {
        let bytes = framing();
        assert_eq!(raw_member_flags(&bytes), Some((vec![0x2001], vec![0x4001])));
        for end in 0..bytes.len() {
            assert!(raw_member_flags(&bytes[..end]).is_none(), "prefix {end}");
        }
        let mut unknown = bytes.clone();
        unknown[10] = 2;
        assert!(raw_member_flags(&unknown).is_none());
        let mut overflow = bytes;
        let len = overflow.len();
        overflow[len - 4..].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(raw_member_flags(&overflow).is_none());
    }
}
