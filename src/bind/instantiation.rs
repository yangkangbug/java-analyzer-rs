//! Bounded templates for an explicit, plain instantiation of a binary owner.
//! Includes bounded parent templates, not general generic assignability or inference.
use super::Primitive;

const MAX_BYTES: usize = 4096;
const MAX_FORMALS: usize = 32;
const MAX_PARENTS: usize = 32;
const MAX_PARAMETERS: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ClassTemplate {
    pub parameters: Vec<String>,
    pub parents: Vec<ParentTemplate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ParentTemplate {
    pub name: String,
    pub arguments: Vec<TypeTemplate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TypeTemplate {
    Primitive(Primitive),
    Nominal(String),
    Parameter(usize),
    Void,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MethodTemplate {
    pub parameters: Vec<TypeTemplate>,
    pub result: TypeTemplate,
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}
impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Option<Self> {
        (text.len() <= MAX_BYTES).then_some(Self { text: text.as_bytes(), at: 0 })
    }
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }
    fn eat(&mut self, byte: u8) -> Option<()> {
        if self.peek()? != byte {
            return None;
        }
        self.at += 1;
        Some(())
    }
    // Deliberately narrow identifiers. Exotic JVM names and nested owners remain unknown.
    fn identifier(&mut self) -> Option<String> {
        let start = self.at;
        let first = self.peek()?;
        if !first.is_ascii_alphabetic() && first != b'_' {
            return None;
        }
        self.at += 1;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_') {
            self.at += 1;
        }
        String::from_utf8(self.text[start..self.at].to_vec()).ok()
    }
    fn binary_name(&mut self) -> Option<String> {
        let mut name = self.identifier()?;
        while self.peek() == Some(b'/') {
            self.at += 1;
            name.push('/');
            name.push_str(&self.identifier()?);
        }
        Some(name)
    }
    fn ty(&mut self, formals: &[String], void: bool) -> Option<TypeTemplate> {
        let byte = self.peek()?;
        self.at += 1;
        let primitive = match byte {
            b'Z' => Some(Primitive::Boolean),
            b'B' => Some(Primitive::Byte),
            b'C' => Some(Primitive::Char),
            b'S' => Some(Primitive::Short),
            b'I' => Some(Primitive::Int),
            b'J' => Some(Primitive::Long),
            b'F' => Some(Primitive::Float),
            b'D' => Some(Primitive::Double),
            _ => None,
        };
        if let Some(p) = primitive {
            return Some(TypeTemplate::Primitive(p));
        }
        match byte {
            b'V' if void => Some(TypeTemplate::Void),
            b'L' => {
                let name = self.binary_name()?;
                self.eat(b';')?;
                Some(TypeTemplate::Nominal(name))
            }
            b'T' => {
                let name = self.identifier()?;
                self.eat(b';')?;
                Some(TypeTemplate::Parameter(formals.iter().position(|f| f == &name)?))
            }
            _ => None,
        }
    }
    fn parent(&mut self, formals: &[String]) -> Option<ParentTemplate> {
        self.eat(b'L')?;
        let name = self.binary_name()?;
        let mut arguments = Vec::new();
        if self.peek() == Some(b'<') {
            self.at += 1;
            let mut count = 0;
            while self.peek() != Some(b'>') {
                count += 1;
                if count > MAX_FORMALS {
                    return None;
                }
                let argument = self.ty(formals, false)?;
                match argument {
                    TypeTemplate::Nominal(_) | TypeTemplate::Parameter(_) => arguments.push(argument),
                    _ => return None,
                }
            }
            if count == 0 {
                return None;
            }
            self.eat(b'>')?;
        }
        self.eat(b';')?;
        Some(ParentTemplate { name, arguments })
    }
    fn method(&mut self, formals: &[String]) -> Option<MethodTemplate> {
        self.eat(b'(')?;
        let mut parameters = Vec::new();
        while self.peek() != Some(b')') {
            if parameters.len() == MAX_PARAMETERS {
                return None;
            }
            parameters.push(self.ty(formals, false)?);
        }
        self.eat(b')')?;
        let result = self.ty(formals, true)?;
        if self.at != self.text.len() {
            return None;
        }
        Some(MethodTemplate { parameters, result })
    }
}

impl ClassTemplate {
    pub fn parse(signature: &str, expected_parents: &[&str]) -> Option<Self> {
        let mut parser = Parser::new(signature)?;
        let mut parameters = Vec::new();
        if parser.peek() == Some(b'<') {
            parser.eat(b'<')?;
            while parser.peek() != Some(b'>') {
                if parameters.len() == MAX_FORMALS {
                    return None;
                }
                let name = parser.identifier()?;
                if parameters.contains(&name) {
                    return None;
                }
                parser.eat(b':')?;
                // Exactly one concrete Object bound. Interface/intersection/F-bounds are unknown.
                if parser.ty(&[], false)? != TypeTemplate::Nominal("java/lang/Object".into()) {
                    return None;
                }
                parameters.push(name);
            }
            parser.eat(b'>')?;
            if parameters.is_empty() {
                return None;
            }
        }
        let mut parents = Vec::new();
        while parser.at < parser.text.len() {
            if parents.len() == MAX_PARENTS {
                return None;
            }
            parents.push(parser.parent(&parameters)?);
        }
        if parents.is_empty() || parents.iter().map(|parent| parent.name.as_str()).ne(expected_parents.iter().copied())
        {
            return None;
        }
        Some(Self { parameters, parents })
    }
}
impl TypeTemplate {
    fn erases_to(&self, descriptor: &Self) -> bool {
        match self {
            Self::Parameter(_) => *descriptor == Self::Nominal("java/lang/Object".into()),
            _ => self == descriptor,
        }
    }
}
impl MethodTemplate {
    pub fn parse(signature: Option<&str>, descriptor: &str, owner: &ClassTemplate) -> Option<Self> {
        let erased = Parser::new(descriptor)?.method(&[])?;
        let Some(signature) = signature else {
            return Some(erased);
        };
        let template = Parser::new(signature)?.method(&owner.parameters)?;
        if template.parameters.len() != erased.parameters.len()
            || !template.parameters.iter().zip(&erased.parameters).all(|(a, b)| a.erases_to(b))
            || !template.result.erases_to(&erased.result)
        {
            return None;
        }
        Some(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner() -> ClassTemplate {
        ClassTemplate::parse("<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;", &["java/lang/Object"])
            .unwrap()
    }
    #[test]
    fn complete_class_and_parent_inventory() {
        let text = "<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;Lpkg/Parent<TK;Ljava/lang/String;>;";
        let c = ClassTemplate::parse(text, &["java/lang/Object", "pkg/Parent"]).unwrap();
        assert_eq!(c.parameters, ["K", "V"]);
        assert!(ClassTemplate::parse(text, &["pkg/Parent", "java/lang/Object"]).is_none());
        assert!(ClassTemplate::parse(text, &["java/lang/Object"]).is_none());
        assert!(ClassTemplate::parse(text, &["java/lang/Object", "pkg/Other"]).is_none());
    }
    #[test]
    fn rejects_malformed_bounds_variables_and_complex_parents() {
        for text in [
            "<>Ljava/lang/Object;",
            "<T:Ljava/lang/Object;T:Ljava/lang/Object;>Ljava/lang/Object;",
            "<T:Ljava/lang/Number;>Ljava/lang/Object;",
            "<T::Ljava/lang/Object;>Ljava/lang/Object;",
            "<T:Ljava/lang/Object;:Ljava/io/Serializable;>Ljava/lang/Object;",
            "<T:TT;>Ljava/lang/Object;",
            "<T:Ljava/lang/Object;>",
            "<T:Ljava/lang/Object;>Ljava/lang/Object;junk",
            "<T:Ljava/lang/Object;>Lpkg/P<TX;>;",
            "<T:Ljava/lang/Object;>Lpkg/P<+TT;>;",
            "<T:Ljava/lang/Object;>Lpkg/P<*>;",
            "<T:Ljava/lang/Object;>Lpkg/P<[TT;>;",
            "<T:Ljava/lang/Object;>Lpkg/P<>;",
            "<T:Ljava/lang/Object;>Lpkg/P<Lpkg/Q<TT;>;>;",
            "<T:Ljava/lang/Object;>Lpkg/P.Inner;",
            "<T:Ljava/lang/Object;>Lpkg//P;",
        ] {
            assert!(ClassTemplate::parse(text, &["java/lang/Object"]).is_none(), "{text}");
        }
    }
    #[test]
    fn unsupported_parent_syntax_is_not_hidden_by_inventory_mismatch() {
        for tail in [
            "Lpkg/P<TX;>;",
            "Lpkg/P<+TK;>;",
            "Lpkg/P<*>;",
            "Lpkg/P<[TK;>;",
            "Lpkg/P<>;",
            "Lpkg/P<Lpkg/Q<TK;>;>;",
            "Lpkg/P.Inner;",
            "Lpkg/P<I>;",
            "Lpkg/P<TK;>",
        ] {
            let text = format!("<K:Ljava/lang/Object;>{tail}");
            assert!(ClassTemplate::parse(&text, &["pkg/P"]).is_none(), "{text}");
        }
    }
    #[test]
    fn method_templates_and_no_guess_from_descriptor() {
        let c = owner();
        let m =
            MethodTemplate::parse(Some("(TK;TV;)TV;"), "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &c)
                .unwrap();
        assert_eq!(m.parameters, [TypeTemplate::Parameter(0), TypeTemplate::Parameter(1)]);
        assert_eq!(m.result, TypeTemplate::Parameter(1));
        let raw = MethodTemplate::parse(None, "(Ljava/lang/Object;)Ljava/lang/Object;", &c).unwrap();
        assert_eq!(raw.result, TypeTemplate::Nominal("java/lang/Object".into()));
        assert!(MethodTemplate::parse(Some("(ZBCSIJFD)V"), "(ZBCSIJFD)V", &c).is_some());
    }
    #[test]
    fn methods_reject_scope_erasure_arrays_and_trailing_data() {
        let c = owner();
        for (sig, desc) in [
            ("(TX;)V", "(Ljava/lang/Object;)V"),
            ("(TK;)TV;", "(Ljava/lang/String;)Ljava/lang/Object;"),
            ("(TK;)TV;", "(Ljava/lang/Object;)Ljava/lang/String;"),
            ("(I)V", "(J)V"),
            ("(I)V", "()V"),
            ("(V)V", "(V)V"),
            ("()[I", "()[I"),
            ("([TK;)V", "([Ljava/lang/Object;)V"),
            ("<T:Ljava/lang/Object;>(TT;)V", "(Ljava/lang/Object;)V"),
            ("()V^Ljava/lang/Exception;", "()V"),
            ("()Vjunk", "()V"),
            ("()Ljava/util/List<TK;>;", "()Ljava/util/List;"),
            ("()V", "()Vjunk"),
            ("()V", "()"),
            ("(TK)V", "(Ljava/lang/Object;)V"),
        ] {
            assert!(MethodTemplate::parse(Some(sig), desc, &c).is_none(), "{sig} vs {desc}");
        }
        assert!(MethodTemplate::parse(None, "()TK;", &c).is_none());
    }
    #[test]
    fn bounded_inputs() {
        let c = owner();
        assert!(MethodTemplate::parse(None, &format!("({})V", "I".repeat(255)), &c).is_some());
        assert!(MethodTemplate::parse(None, &format!("({})V", "I".repeat(256)), &c).is_none());
        assert!(MethodTemplate::parse(None, &format!("()L{};", "A".repeat(4096)), &c).is_none());
        for n in [32, 33] {
            let formals: String = (0..n).map(|i| format!("T{i}:Ljava/lang/Object;")).collect();
            let signature = format!("<{formals}>Ljava/lang/Object;");
            assert_eq!(ClassTemplate::parse(&signature, &["java/lang/Object"]).is_some(), n == 32);
        }
    }
    #[test]
    fn every_truncation_refuses_without_partial_certificate() {
        let text = "<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;Lpkg/P<TK;TV;>;";
        for end in 0..text.len() {
            assert!(ClassTemplate::parse(&text[..end], &["java/lang/Object", "pkg/P"]).is_none(), "class prefix {end}");
        }
        let owner = owner();
        let signature = "(TK;Ljava/lang/String;I)TV;";
        let descriptor = "(Ljava/lang/Object;Ljava/lang/String;I)Ljava/lang/Object;";
        for end in 0..signature.len() {
            assert!(
                MethodTemplate::parse(Some(&signature[..end]), descriptor, &owner).is_none(),
                "signature prefix {end}"
            );
        }
        for end in 0..descriptor.len() {
            assert!(
                MethodTemplate::parse(Some(signature), &descriptor[..end], &owner).is_none(),
                "descriptor prefix {end}"
            );
        }
    }

    #[test]
    fn formal_order_is_owner_local_and_nominal_names_are_not_variables() {
        let reversed =
            ClassTemplate::parse("<V:Ljava/lang/Object;K:Ljava/lang/Object;>Ljava/lang/Object;", &["java/lang/Object"])
                .unwrap();
        let signature = "(TK;LK;)TV;";
        let descriptor = "(Ljava/lang/Object;LK;)Ljava/lang/Object;";
        let first = MethodTemplate::parse(Some(signature), descriptor, &owner()).unwrap();
        let second = MethodTemplate::parse(Some(signature), descriptor, &reversed).unwrap();
        assert_eq!(first.parameters, [TypeTemplate::Parameter(0), TypeTemplate::Nominal("K".into())]);
        assert_eq!(second.parameters, [TypeTemplate::Parameter(1), TypeTemplate::Nominal("K".into())]);
        assert_eq!(first.result, TypeTemplate::Parameter(1));
        assert_eq!(second.result, TypeTemplate::Parameter(0));
        // Owner formals never turn descriptor type-variable syntax into a valid descriptor.
        assert!(MethodTemplate::parse(Some("(TK;)V"), "(TK;)V", &owner()).is_none());
    }

    #[test]
    fn parent_count_and_exact_byte_limit() {
        for n in [32, 33] {
            let parents: Vec<_> = (0..n).map(|i| format!("pkg/P{i}")).collect();
            let signature =
                format!("<T:Ljava/lang/Object;>{}", parents.iter().map(|p| format!("L{p};")).collect::<String>());
            let expected: Vec<_> = parents.iter().map(String::as_str).collect();
            assert_eq!(ClassTemplate::parse(&signature, &expected).is_some(), n == 32);
        }
        let exact = format!("()L{};", "A".repeat(MAX_BYTES - 4));
        assert_eq!(exact.len(), MAX_BYTES);
        assert!(MethodTemplate::parse(None, &exact, &owner()).is_some());
        assert!(MethodTemplate::parse(None, &exact.replace("()L", "()LA"), &owner()).is_none());
        for tail in
            ["Lpkg/P<TK;>;", "Lpkg/P<TK;>;junk", "Lpkg/P<TK;>;\0", "Lpkg/P$Inner;", "Lpkg/P.Inner;", "Lpkg/类型;"]
        {
            let signature = format!("<K:Ljava/lang/Object;>{tail}");
            assert_eq!(ClassTemplate::parse(&signature, &["pkg/P"]).is_some(), tail == "Lpkg/P<TK;>;");
        }
    }
    #[test]
    fn parent_templates_preserve_order_constant_and_raw_arguments() {
        let c = ClassTemplate::parse(
            "<T:Ljava/lang/Object;U:Ljava/lang/Object;>Ljava/lang/Object;Lpkg/P<TU;TT;LA;>;Lpkg/Raw;",
            &["java/lang/Object", "pkg/P", "pkg/Raw"],
        )
        .unwrap();
        assert_eq!(c.parents[0], ParentTemplate { name: "java/lang/Object".into(), arguments: vec![] });
        assert_eq!(
            c.parents[1],
            ParentTemplate {
                name: "pkg/P".into(),
                arguments: vec![
                    TypeTemplate::Parameter(1),
                    TypeTemplate::Parameter(0),
                    TypeTemplate::Nominal("A".into())
                ]
            }
        );
        assert_eq!(c.parents[2], ParentTemplate { name: "pkg/Raw".into(), arguments: vec![] });
        let c = ClassTemplate::parse("Lpkg/P<LA;>;", &["pkg/P"]).unwrap();
        assert!(c.parameters.is_empty());
        assert_eq!(c.parents[0].arguments, [TypeTemplate::Nominal("A".into())]);
        let raw = ClassTemplate::parse("Lpkg/P;", &["pkg/P"]).unwrap();
        assert!(raw.parents[0].arguments.is_empty());
        assert_ne!(raw.parents, c.parents);
    }

    #[test]
    fn nongeneric_signatures_have_no_implicit_formal_scope() {
        assert!(ClassTemplate::parse("Ljava/lang/Object;", &["java/lang/Object"]).unwrap().parameters.is_empty());
        for text in [
            "",
            "<>Ljava/lang/Object;",
            "Lpkg/P<TT;>;",
            "Lpkg/P<LA;>;junk",
            "Lpkg/P<LA;>",
            "Lpkg/P<+LA;>;",
            "Lpkg/P<[LA;>;",
        ] {
            assert!(ClassTemplate::parse(text, &["pkg/P"]).is_none(), "{text}");
        }
        let owner = ClassTemplate::parse("Lpkg/P<LA;>;", &["pkg/P"]).unwrap();
        assert!(MethodTemplate::parse(Some("()TT;"), "()Ljava/lang/Object;", &owner).is_none());
        assert!(MethodTemplate::parse(None, "()Ljava/lang/Object;", &owner).is_some());
    }
}
