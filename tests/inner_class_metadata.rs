use java_engine::{InnerClassEntry, parse_class_data};

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

#[test]
fn attributes_and_entries_remain_grouped_in_original_order() {
    let entry = ("p/Outer$Inner", Some("p/Outer"), Some("Inner"), 0x0009);
    let other = ("q/Other$Nested", Some("q/Other"), Some("Nested"), 0x8003);
    let (bytes, _) = class("p/Outer$Inner", &[vec![entry, other, entry], vec![], vec![other]], false);
    let parsed = parse_class_data("fixture", &bytes).unwrap();
    let own = InnerClassEntry {
        binary_name: "p/Outer$Inner".into(),
        outer_binary_name: Some("p/Outer".into()),
        inner_name: Some("Inner".into()),
        raw_access_flags: 9,
    };
    let other = InnerClassEntry {
        binary_name: "q/Other$Nested".into(),
        outer_binary_name: Some("q/Other".into()),
        inner_name: Some("Nested".into()),
        raw_access_flags: 0x8003,
    };
    assert_eq!(parsed.inner_classes, vec![vec![own.clone(), other.clone(), own], vec![], vec![other]]);
    assert!(!parsed.has_enclosing_method);
}

#[test]
fn absent_empty_and_dollar_named_top_level_are_not_inferred_relationships() {
    let (bytes, _) = class("p/Cash$Money", &[], false);
    let absent = parse_class_data("fixture", &bytes).unwrap();
    assert_eq!(absent.binary_name, "p/Cash$Money");
    assert!(absent.inner_classes.is_empty());
    let (bytes, _) = class("p/Cash$Money", &[vec![]], false);
    assert_eq!(parse_class_data("fixture", &bytes).unwrap().inner_classes, vec![vec![]]);
}

#[test]
fn anonymous_local_and_enclosing_attribute_are_preserved_not_certified() {
    let (bytes, _) = class(
        "p/Outer$1Local",
        &[vec![
            ("p/Outer$1", None, None, 0),
            ("p/Outer$1Local", None, Some("Local"), 0x1010),
            ("p/Outer$Odd", Some("p/Outer"), None, 0x0002),
        ]],
        true,
    );
    let parsed = parse_class_data("fixture", &bytes).unwrap();
    assert!(parsed.has_enclosing_method);
    assert_eq!(parsed.inner_classes[0][0].outer_binary_name, None);
    assert_eq!(parsed.inner_classes[0][0].inner_name, None);
    assert_eq!(parsed.inner_classes[0][1].inner_name.as_deref(), Some("Local"));
    assert_eq!(parsed.inner_classes[0][1].raw_access_flags, 0x1010);
    assert_eq!(parsed.inner_classes[0][2].inner_name, None);
}

#[test]
fn truncated_and_mismatched_payloads_are_errors_without_partial_metadata() {
    let (bytes, offsets) =
        class("p/Outer$Inner", &[vec![("p/Outer$Inner", Some("p/Outer"), Some("Inner"), 0x8001)]], false);
    for end in 0..bytes.len() {
        assert!(parse_class_data("fixture", &bytes[..end]).is_err(), "prefix {end}");
    }
    let start = offsets[0];
    let mut underreported = bytes.clone();
    underreported[start..start + 2].copy_from_slice(&0u16.to_be_bytes());
    assert!(parse_class_data("fixture", &underreported).is_err());
    let mut overreported = bytes.clone();
    overreported[start..start + 2].copy_from_slice(&2u16.to_be_bytes());
    assert!(parse_class_data("fixture", &overreported).is_err());
    let mut invalid_pool = bytes.clone();
    invalid_pool[start + 2..start + 4].copy_from_slice(&0u16.to_be_bytes());
    assert!(parse_class_data("fixture", &invalid_pool).is_err());
    let parsed = parse_class_data("fixture", &bytes).unwrap();
    assert_eq!(parsed.inner_classes[0][0].raw_access_flags, 0x8001);
}

#[test]
fn legacy_signature_construction_and_public_return_types_remain_compatible() {
    use java_engine::{Access, ClassError, ClassSignature, JarError, JarOptions, JmodOptions, Project};
    use std::path::Path;
    let signature = ClassSignature {
        origin: "fixture".into(),
        binary_name: "p/Outer$Inner".into(),
        dotted_name: "p.Outer.Inner".into(),
        package: "p".into(),
        nesting: vec!["Outer".into(), "Inner".into()],
        super_class: Some("java.lang.Object".into()),
        interfaces: vec![],
        binary_super_class: Some("java/lang/Object".into()),
        binary_interfaces: vec![],
        signature: None,
        major: 61,
        minor: 0,
        is_interface: false,
        is_final: false,
        access: Access::Public,
        fields: vec![],
        methods: vec![],
    };
    let parse: fn(&str, &[u8]) -> Result<ClassSignature, ClassError> = java_engine::parse_class_bytes;
    let _jar: fn(&Path, &JarOptions) -> Result<Vec<ClassSignature>, JarError> = java_engine::read_jar;
    let _jmod: fn(&Path, &JmodOptions) -> Result<Vec<ClassSignature>, JarError> = java_engine::read_jmod;
    let _lookup: for<'a, 'b> fn(&'a Project, &'b str) -> Option<&'a ClassSignature> = Project::class_signature;
    let (bytes, _) = class("p/Outer$Inner", &[vec![("p/Outer$Inner", Some("p/Outer"), Some("Inner"), 9)]], false);
    let legacy: ClassSignature = parse("fixture", &bytes).unwrap();
    assert_eq!(legacy, signature);
    assert_eq!(parse_class_data("fixture", &bytes).unwrap().class_signature, legacy);
}
