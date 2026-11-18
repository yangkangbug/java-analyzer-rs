//! Recursive-descent parser for a documented Java syntax subset.
//! Expression precedence follows the Java Language Specification.
//! This is not a translation of IntelliJ's `JavaParser`.

use std::sync::Arc;

use crate::green::{GreenElement, GreenNode, GreenToken};
use crate::kind::SyntaxKind;
use crate::lex::{Token, lex};

#[derive(Clone, Copy)]
enum NameRole {
    Package,
    Type,
    Import,
    StaticImport,
}

fn is_identifier(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::Ident | SyntaxKind::Var)
}
fn type_segment(kind: SyntaxKind, next: SyntaxKind, type_boundary: bool) -> bool {
    kind == SyntaxKind::Ident || kind == SyntaxKind::Var && next == SyntaxKind::Dot && !type_boundary
}

pub fn parse_file(text: &str) -> Arc<GreenNode> {
    let mut p = Parser::new(text, lex(text));
    p.start(SyntaxKind::File);
    p.parse_package_opt();
    while p.at(SyntaxKind::Import) {
        p.parse_import();
    }
    while !p.eof() {
        let before = p.pos;
        if p.eat(SyntaxKind::Semi) {
            continue;
        }
        if p.looks_like_type_decl() {
            p.parse_type_decl();
        } else {
            p.error_one("expected type declaration");
        }
        if p.pos == before {
            p.bump();
        }
    }
    p.drain();
    p.finish()
}

pub fn parse_fragment(kind: SyntaxKind, text: &str) -> Arc<GreenNode> {
    let mut p = Parser::new(text, lex(text));
    match kind {
        SyntaxKind::EnumBody => p.parse_enum_body(),
        SyntaxKind::ClassBody | SyntaxKind::InterfaceBody | SyntaxKind::RecordBody => p.parse_body(kind),
        _ => p.parse_block(),
    }
}

// Validate only type-grammar edges, not expressions inside annotation values.
// Shared parsing builds the tree; this role-specific check excludes invocation
// diamond/wildcards and invalid nested reference-type arguments.
fn valid_invocation_type_args(node: &GreenNode, allow_wildcard: bool) -> bool {
    let before_close =
        node.children().iter().rev().filter(|c| c.as_token().is_none_or(|t| !t.kind().is_trivia())).nth(1);
    if before_close.is_some_and(|c| c.as_token().is_some_and(|t| t.kind() == SyntaxKind::Comma)) {
        return false;
    }
    let types: Vec<_> = node.child_nodes().collect();
    if types.is_empty() || types.iter().any(|ty| ty.kind() != SyntaxKind::Type) {
        return false;
    }
    types.iter().all(|ty| {
        let kinds: Vec<_> = ty
            .children()
            .iter()
            .filter_map(|c| c.as_token())
            .filter(|t| !t.kind().is_trivia())
            .map(|t| t.kind())
            .collect();
        let array = kinds.contains(&SyntaxKind::LBracket);
        let wildcard = kinds.contains(&SyntaxKind::Question);
        if kinds.contains(&SyntaxKind::Void)
            || wildcard
                && (!allow_wildcard
                    || array && !kinds.contains(&SyntaxKind::Extends) && !kinds.contains(&SyntaxKind::Super))
        {
            return false;
        }
        if !array
            && kinds.iter().any(|k| {
                matches!(
                    k,
                    SyntaxKind::BooleanKw
                        | SyntaxKind::Byte
                        | SyntaxKind::CharKw
                        | SyntaxKind::Short
                        | SyntaxKind::IntKw
                        | SyntaxKind::LongKw
                        | SyntaxKind::FloatKw
                        | SyntaxKind::DoubleKw
                )
            })
        {
            return false;
        }
        ty.child_nodes().filter(|n| n.kind() == SyntaxKind::Name).all(|name| {
            name.child_nodes()
                .filter(|n| n.kind() == SyntaxKind::TypeArgList)
                .all(|args| valid_invocation_type_args(args, true))
        })
    })
}

struct Frame {
    kind: SyntaxKind,
    children: Vec<GreenElement>,
    message: Option<&'static str>,
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    stack: Vec<Frame>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, tokens: Vec<Token>) -> Self {
        Self { src, tokens, pos: 0, stack: Vec::new() }
    }

    fn start(&mut self, kind: SyntaxKind) {
        self.stack.push(Frame { kind, children: Vec::new(), message: None });
    }

    /// Open a node that begins at the next token. Leading trivia stays on the parent,
    /// so the node's byte range is the construct itself, not the whitespace before it.
    fn open(&mut self, kind: SyntaxKind) {
        if !self.stack.is_empty() {
            self.drain();
        }
        self.start(kind);
    }

    fn finish(&mut self) -> Arc<GreenNode> {
        let frame = self.stack.pop().expect("finish");
        let node = GreenNode::from_parts(frame.kind, frame.children, frame.message);
        if let Some(parent) = self.stack.last_mut() {
            parent.children.push(GreenElement::Node(Arc::clone(&node)));
        }
        node
    }

    fn set_message(&mut self, message: &'static str) {
        self.stack.last_mut().expect("frame").message = Some(message);
    }

    fn next_sig(&self) -> usize {
        let mut i = self.pos;
        while i < self.tokens.len() && self.tokens[i].kind.is_trivia() {
            i += 1;
        }
        i
    }

    fn kind(&self) -> SyntaxKind {
        let i = self.next_sig();
        if i >= self.tokens.len() { SyntaxKind::Eof } else { self.tokens[i].kind }
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.kind() == kind
    }

    fn eof(&self) -> bool {
        self.kind() == SyntaxKind::Eof
    }

    fn drain(&mut self) {
        while self.pos < self.tokens.len() && self.tokens[self.pos].kind.is_trivia() {
            if self.stack.is_empty() {
                self.pos += 1;
            } else {
                self.push_token(self.pos);
                self.pos += 1;
            }
        }
    }

    fn push_token(&mut self, index: usize) {
        let token = &self.tokens[index];
        let text = self.src[token.start as usize..token.end as usize].to_string();
        let green = GreenToken::new(token.kind, text);
        self.stack.last_mut().expect("frame").children.push(GreenElement::Token(green));
    }

    fn bump(&mut self) {
        self.drain();
        if self.pos >= self.tokens.len() {
            return;
        }
        self.push_token(self.pos);
        self.pos += 1;
    }

    fn bump_as(&mut self, kind: SyntaxKind) {
        self.drain();
        if let Some(token) = self.tokens.get(self.pos) {
            let text = self.src[token.start as usize..token.end as usize].to_owned();
            self.stack.last_mut().expect("frame").children.push(GreenElement::Token(GreenToken::new(kind, text)));
            self.pos += 1;
        }
    }
    fn eat_identifier(&mut self) -> bool {
        if is_identifier(self.kind()) {
            self.bump_as(SyntaxKind::Ident);
            true
        } else {
            false
        }
    }
    fn expect_identifier(&mut self, message: &'static str) {
        if !self.eat_identifier() {
            self.open(SyntaxKind::Error);
            self.set_message(message);
            self.finish();
        }
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: SyntaxKind, message: &'static str) {
        if !self.eat(kind) {
            self.open(SyntaxKind::Error);
            self.set_message(message);
            self.finish();
        }
    }

    fn eat_gt(&mut self) -> bool {
        match self.kind() {
            SyntaxKind::Gt => {
                self.bump();
                true
            }
            SyntaxKind::GtGt | SyntaxKind::GtGtGt => {
                self.split_one_gt();
                self.bump();
                true
            }
            _ => false,
        }
    }

    fn split_one_gt(&mut self) {
        let index = self.next_sig();
        let token = self.tokens[index].clone();
        let rest = match token.kind {
            SyntaxKind::GtGt => SyntaxKind::Gt,
            SyntaxKind::GtGtGt => SyntaxKind::GtGt,
            _ => return,
        };
        let mid = token.start + 1;
        self.tokens[index] = Token { kind: SyntaxKind::Gt, start: token.start, end: mid };
        self.tokens.insert(index + 1, Token { kind: rest, start: mid, end: token.end });
    }

    fn pop(&mut self) -> GreenElement {
        self.stack.last_mut().expect("frame").children.pop().unwrap_or_else(|| {
            GreenElement::Node(GreenNode::from_parts(SyntaxKind::Error, Vec::new(), Some("expected expression")))
        })
    }

    fn push(&mut self, element: GreenElement) {
        self.stack.last_mut().expect("frame").children.push(element);
    }

    fn error_one(&mut self, message: &'static str) {
        self.open(SyntaxKind::Error);
        self.set_message(message);
        if !self.eof() {
            self.bump();
        }
        self.finish();
    }

    fn parse_package_opt(&mut self) {
        if !self.at(SyntaxKind::Package) {
            return;
        }
        self.open(SyntaxKind::PackageDecl);
        self.bump();
        self.parse_qualified_name(NameRole::Package);
        self.expect(SyntaxKind::Semi, "expected ';'");
        self.finish();
    }

    fn parse_import(&mut self) {
        self.open(SyntaxKind::ImportDecl);
        self.bump();
        let static_member = self.eat(SyntaxKind::Static);
        self.parse_qualified_name(if static_member { NameRole::StaticImport } else { NameRole::Import });
        self.expect(SyntaxKind::Semi, "expected ';'");
        self.finish();
    }

    fn parse_qualified_name(&mut self, role: NameRole) {
        let allow_star = matches!(role, NameRole::Import | NameRole::StaticImport);
        let terminal_type = matches!(role, NameRole::Type | NameRole::Import);
        let static_owner = matches!(role, NameRole::StaticImport);
        self.open(SyntaxKind::Name);
        loop {
            let invalid_owner = static_owner
                && self.nth(1) == SyntaxKind::Dot
                && (self.nth(2) == SyntaxKind::Star || self.nth(3) != SyntaxKind::Dot);
            if self.at(SyntaxKind::Var) && (terminal_type && self.nth(1) != SyntaxKind::Dot || invalid_owner) {
                self.expect(SyntaxKind::Ident, "expected type name");
                self.bump();
            } else {
                self.expect_identifier("expected name");
            }
            if !self.eat(SyntaxKind::Dot) {
                break;
            }
            if allow_star && self.eat(SyntaxKind::Star) {
                break;
            }
        }
        self.finish();
    }

    fn looks_like_type_decl(&self) -> bool {
        let mut look = Look::from(self);
        look.skip_mods();
        matches!(look.kind(), SyntaxKind::ClassKw | SyntaxKind::InterfaceKw | SyntaxKind::EnumKw | SyntaxKind::RecordKw)
            || look.kind() == SyntaxKind::At && look.nth(1) == SyntaxKind::InterfaceKw
    }

    fn parse_type_decl(&mut self) {
        let mut look = Look::from(self);
        look.skip_mods();
        match look.kind() {
            SyntaxKind::At if look.nth(1) == SyntaxKind::InterfaceKw => self.parse_annotation_type(),
            SyntaxKind::InterfaceKw => self.parse_class_like(SyntaxKind::Interface, SyntaxKind::InterfaceBody),
            SyntaxKind::EnumKw => self.parse_enum(),
            SyntaxKind::RecordKw => self.parse_record(),
            SyntaxKind::ClassKw => self.parse_class_like(SyntaxKind::Class, SyntaxKind::ClassBody),
            _ => self.error_one("expected type declaration"),
        }
    }

    fn parse_annotation_type(&mut self) {
        self.start(SyntaxKind::AnnotationType);
        self.parse_modifier_list();
        self.expect(SyntaxKind::At, "expected '@'");
        self.expect(SyntaxKind::InterfaceKw, "expected 'interface'");
        self.expect(SyntaxKind::Ident, "expected annotation type name");
        if self.at(SyntaxKind::Lt) {
            self.open(SyntaxKind::Error);
            self.set_message("annotation types cannot have type parameters");
            self.parse_type_params();
            self.finish();
        }
        while matches!(self.kind(), SyntaxKind::Extends | SyntaxKind::Implements) {
            self.open(SyntaxKind::Error);
            self.set_message("annotation types cannot declare parent types");
            self.bump();
            self.parse_type_list();
            self.finish();
        }
        self.open(SyntaxKind::AnnotationBody);
        self.expect(SyntaxKind::LBrace, "expected '{'");
        while !self.eof() && !self.at(SyntaxKind::RBrace) {
            let before = self.pos;
            if self.at(SyntaxKind::Semi) {
                self.bump();
            } else {
                match self.member_shape() {
                    MemberShape::Type => self.parse_type_decl(),
                    MemberShape::Field => self.parse_field(),
                    _ => self.parse_annotation_element(),
                }
            }
            if self.pos == before {
                self.error_one("expected annotation member");
            }
        }
        self.expect(SyntaxKind::RBrace, "expected '}'");
        self.finish();
        self.finish();
    }

    fn parse_annotation_element(&mut self) {
        self.open(SyntaxKind::AnnotationElement);
        self.parse_modifier_list();
        if self.at(SyntaxKind::Lt) {
            self.open(SyntaxKind::Error);
            self.set_message("annotation elements cannot have type parameters");
            self.parse_type_params();
            self.finish();
        }
        let is_void = self.at(SyntaxKind::Void);
        self.parse_type();
        if is_void {
            self.open(SyntaxKind::Error);
            self.set_message("annotation elements cannot return void");
            self.finish();
        }
        self.expect_identifier("expected annotation element name");
        let empty = self.at(SyntaxKind::LParen) && self.nth(1) == SyntaxKind::RParen;
        self.parse_params();
        if !empty {
            self.open(SyntaxKind::Error);
            self.set_message("annotation elements require empty parameters");
            self.finish();
        }
        self.parse_array_dimensions();
        if self.at(SyntaxKind::Throws) {
            self.open(SyntaxKind::Error);
            self.set_message("annotation elements cannot declare throws");
            self.bump();
            self.parse_type_list();
            self.finish();
        }
        if self.at(SyntaxKind::Default) {
            self.open(SyntaxKind::AnnotationDefault);
            self.bump();
            self.parse_element_value();
            self.finish();
        }
        if self.at(SyntaxKind::LBrace) {
            self.open(SyntaxKind::Error);
            self.set_message("annotation elements cannot have a body");
            self.parse_block();
            self.finish();
        } else {
            self.expect(SyntaxKind::Semi, "expected ';' after annotation element");
        }
        self.finish();
    }

    fn parse_class_like(&mut self, node: SyntaxKind, body: SyntaxKind) {
        self.start(node);
        self.parse_modifier_list();
        self.bump();
        self.expect(SyntaxKind::Ident, "expected name");
        if self.at(SyntaxKind::Lt) {
            self.parse_type_params();
        }
        if self.eat(SyntaxKind::Extends) {
            self.parse_type();
            if node == SyntaxKind::Interface {
                while self.eat(SyntaxKind::Comma) {
                    self.parse_type();
                }
            }
        }
        if self.eat(SyntaxKind::Implements) {
            self.parse_type_list();
        }
        if self.eat(SyntaxKind::Permits) {
            self.parse_type_list();
        }
        self.parse_body(body);
        self.finish();
    }

    fn parse_enum(&mut self) {
        self.open(SyntaxKind::Enum);
        self.parse_modifier_list();
        self.bump();
        self.expect(SyntaxKind::Ident, "expected name");
        if self.at(SyntaxKind::Lt) {
            self.parse_type_params();
        }
        if self.eat(SyntaxKind::Implements) {
            self.parse_type_list();
        }
        self.parse_enum_body();
        self.finish();
    }

    fn parse_record(&mut self) {
        self.open(SyntaxKind::Record);
        self.parse_modifier_list();
        self.bump();
        self.expect(SyntaxKind::Ident, "expected name");
        if self.at(SyntaxKind::Lt) {
            self.parse_type_params();
        }
        self.parse_params();
        if self.eat(SyntaxKind::Implements) {
            self.parse_type_list();
        }
        self.parse_body(SyntaxKind::RecordBody);
        self.finish();
    }

    fn parse_type_list(&mut self) {
        self.parse_type();
        while self.eat(SyntaxKind::Comma) {
            self.parse_type();
        }
    }

    fn parse_type_params(&mut self) {
        self.open(SyntaxKind::TypeParamList);
        self.expect(SyntaxKind::Lt, "expected '<'");
        loop {
            self.open(SyntaxKind::TypeParam);
            self.expect(SyntaxKind::Ident, "expected type parameter");
            if self.eat(SyntaxKind::Extends) {
                self.parse_type();
                while self.eat(SyntaxKind::Amp) {
                    self.parse_type();
                }
            }
            self.finish();
            if !self.eat(SyntaxKind::Comma) {
                break;
            }
        }
        if !self.eat_gt() {
            self.expect(SyntaxKind::Gt, "expected '>'");
        }
        self.finish();
    }

    fn parse_body(&mut self, kind: SyntaxKind) -> Arc<GreenNode> {
        // Leading trivia stays in the parent so this node starts at `{`.
        self.drain();
        self.start(kind);
        self.expect(SyntaxKind::LBrace, "expected '{'");
        while !self.eof() && !self.at(SyntaxKind::RBrace) {
            let before = self.pos;
            self.parse_member();
            if self.pos == before {
                self.error_one("expected member");
            }
        }
        self.expect(SyntaxKind::RBrace, "expected '}'");
        self.finish()
    }

    fn parse_enum_body(&mut self) -> Arc<GreenNode> {
        self.drain();
        self.open(SyntaxKind::EnumBody);
        self.expect(SyntaxKind::LBrace, "expected '{'");
        while !self.eof() && !self.at(SyntaxKind::RBrace) {
            if self.at(SyntaxKind::Semi) {
                self.bump();
                self.parse_members_until_brace();
                break;
            }
            self.parse_enum_constant();
            if self.eat(SyntaxKind::Comma) {
                if self.at(SyntaxKind::Semi) || self.at(SyntaxKind::RBrace) {
                    continue;
                }
                continue;
            }
            if self.eat(SyntaxKind::Semi) {
                self.parse_members_until_brace();
            }
            break;
        }
        self.expect(SyntaxKind::RBrace, "expected '}'");
        self.finish()
    }

    fn parse_members_until_brace(&mut self) {
        while !self.eof() && !self.at(SyntaxKind::RBrace) {
            let before = self.pos;
            self.parse_member();
            if self.pos == before {
                self.error_one("expected member");
            }
        }
    }

    fn parse_enum_constant(&mut self) {
        self.open(SyntaxKind::EnumConstant);
        while self.at(SyntaxKind::At) {
            self.parse_annotation();
        }
        self.expect_identifier("expected enum constant");
        if self.at(SyntaxKind::LParen) {
            self.parse_args();
        }
        if self.at(SyntaxKind::LBrace) {
            self.parse_body(SyntaxKind::ClassBody);
        }
        self.finish();
    }

    fn member_shape(&self) -> MemberShape {
        let mut look = Look::from(self);
        look.skip_mods();
        if look.kind() == SyntaxKind::LBrace {
            return MemberShape::Initializer;
        }
        if matches!(
            look.kind(),
            SyntaxKind::ClassKw | SyntaxKind::InterfaceKw | SyntaxKind::EnumKw | SyntaxKind::RecordKw
        ) || look.kind() == SyntaxKind::At && look.nth(1) == SyntaxKind::InterfaceKw
        {
            return MemberShape::Type;
        }
        if look.kind() == SyntaxKind::Lt {
            let mut after = look;
            if after.scan_type_params() {
                if is_identifier(after.kind()) && after.nth(1) == SyntaxKind::LParen {
                    return MemberShape::Constructor;
                }
                return MemberShape::Method;
            }
        }
        if is_identifier(look.kind()) && look.nth(1) == SyntaxKind::LParen {
            return MemberShape::Constructor;
        }
        if look.scan_type() && is_identifier(look.kind()) && look.nth(1) == SyntaxKind::LParen {
            return MemberShape::Method;
        }
        MemberShape::Field
    }

    fn parse_member(&mut self) {
        if self.at(SyntaxKind::Semi) {
            self.bump();
            return;
        }
        match self.member_shape() {
            MemberShape::Initializer => self.parse_initializer(),
            MemberShape::Type => self.parse_type_decl(),
            MemberShape::Constructor => self.parse_constructor(),
            MemberShape::Method => self.parse_method(),
            MemberShape::Field => self.parse_field(),
        }
    }

    fn parse_initializer(&mut self) {
        self.open(SyntaxKind::Initializer);
        self.parse_modifier_list();
        self.parse_block();
        self.finish();
    }

    fn parse_constructor(&mut self) {
        self.open(SyntaxKind::Constructor);
        self.parse_modifier_list();
        if self.at(SyntaxKind::Lt) {
            self.parse_type_params();
        }
        self.expect(SyntaxKind::Ident, "expected constructor name");
        self.parse_params();
        if self.eat(SyntaxKind::Throws) {
            self.parse_type_list();
        }
        if self.at(SyntaxKind::LBrace) {
            self.parse_block();
        } else {
            self.expect(SyntaxKind::Semi, "expected constructor body");
        }
        self.finish();
    }

    fn parse_method(&mut self) {
        self.open(SyntaxKind::Method);
        self.parse_modifier_list();
        if self.at(SyntaxKind::Lt) {
            self.parse_type_params();
        }
        self.parse_type();
        self.expect_identifier("expected method name");
        self.parse_params();
        if self.eat(SyntaxKind::Throws) {
            self.parse_type_list();
        }
        if self.at(SyntaxKind::LBrace) {
            self.parse_block();
        } else {
            self.expect(SyntaxKind::Semi, "expected body or ';'");
        }
        self.finish();
    }

    fn parse_field(&mut self) {
        self.open(SyntaxKind::Field);
        self.parse_modifier_list();
        self.parse_type();
        self.parse_declarators();
        self.expect(SyntaxKind::Semi, "expected ';'");
        self.finish();
    }

    fn parse_declarators(&mut self) {
        loop {
            self.expect_identifier("expected name");
            self.parse_array_dimensions();
            if self.eat(SyntaxKind::Eq) {
                self.parse_expr();
            }
            if !self.eat(SyntaxKind::Comma) {
                break;
            }
        }
    }

    fn parse_modifier_list(&mut self) {
        if !self.has_modifier() {
            return;
        }
        self.open(SyntaxKind::ModifierList);
        while self.at(SyntaxKind::At) && self.nth(1) != SyntaxKind::InterfaceKw || self.kind().is_modifier() {
            if self.at(SyntaxKind::At) {
                self.parse_annotation();
            } else {
                self.bump();
            }
        }
        self.finish();
    }

    fn has_modifier(&self) -> bool {
        self.at(SyntaxKind::At) && self.nth(1) != SyntaxKind::InterfaceKw || self.kind().is_modifier()
    }

    fn parse_annotation(&mut self) {
        self.open(SyntaxKind::Annotation);
        self.expect(SyntaxKind::At, "expected '@'");
        self.parse_qualified_name(NameRole::Type);
        if self.eat(SyntaxKind::LParen) {
            if !self.at(SyntaxKind::RParen) {
                let named = is_identifier(self.kind()) && self.nth(1) == SyntaxKind::Eq;
                let mut first = true;
                loop {
                    let entry_named = is_identifier(self.kind()) && self.nth(1) == SyntaxKind::Eq;
                    if entry_named != named || !named && !first {
                        self.open(SyntaxKind::Error);
                        self.set_message("expected a uniform annotation element list");
                        self.finish();
                    }
                    if entry_named {
                        self.bump_as(SyntaxKind::Ident);
                        self.bump();
                    }
                    self.parse_element_value();
                    first = false;
                    if !self.eat(SyntaxKind::Comma) {
                        break;
                    }
                    if self.at(SyntaxKind::RParen) {
                        self.open(SyntaxKind::Error);
                        self.set_message("expected annotation element after ','");
                        self.finish();
                        break;
                    }
                }
            }
            self.expect(SyntaxKind::RParen, "expected ')'");
        }
        self.finish();
    }

    fn parse_element_value(&mut self) {
        if self.at(SyntaxKind::At) {
            self.parse_annotation();
        } else if self.at(SyntaxKind::LBrace) {
            self.parse_array_values(true);
        } else {
            self.parse_expr();
        }
    }

    /// `n == 0` is the current non-trivia token.
    fn nth(&self, n: usize) -> SyntaxKind {
        let mut i = self.pos;
        let mut seen = 0;
        while i < self.tokens.len() {
            if self.tokens[i].kind.is_trivia() {
                i += 1;
                continue;
            }
            if seen == n {
                return self.tokens[i].kind;
            }
            seen += 1;
            i += 1;
        }
        SyntaxKind::Eof
    }

    fn parse_params(&mut self) {
        self.open(SyntaxKind::ParamList);
        self.expect(SyntaxKind::LParen, "expected '('");
        if !self.at(SyntaxKind::RParen) && !self.eof() {
            loop {
                self.parse_param();
                if !self.eat(SyntaxKind::Comma) || self.at(SyntaxKind::RParen) {
                    break;
                }
            }
        }
        self.expect(SyntaxKind::RParen, "expected ')'");
        self.finish();
    }

    fn parse_param(&mut self) {
        self.open(SyntaxKind::Parameter);
        self.parse_modifier_list();
        self.parse_type();
        let varargs = self.eat(SyntaxKind::DotDotDot);
        self.expect_identifier("expected parameter name");
        if !varargs {
            self.parse_array_dimensions();
        }
        self.finish();
    }

    fn parse_type(&mut self) {
        self.open(SyntaxKind::Type);
        while self.at(SyntaxKind::At) {
            self.parse_annotation();
        }
        if self.eat(SyntaxKind::Question) {
            if self.eat(SyntaxKind::Extends) || self.eat(SyntaxKind::Super) {
                self.parse_type_rest(false);
            }
        } else {
            self.parse_type_rest(true);
        }
        self.parse_array_dimensions();
        self.finish();
    }

    fn parse_array_dimensions(&mut self) {
        while self.eat(SyntaxKind::LBracket) {
            self.expect(SyntaxKind::RBracket, "expected ']'");
        }
    }

    fn parse_type_rest(&mut self, allow_primitive: bool) {
        if allow_primitive && self.eat_primitive() {
            return;
        }
        self.open(SyntaxKind::Name);
        let mut type_boundary = false;
        loop {
            if type_segment(self.kind(), self.nth(1), type_boundary) {
                self.bump_as(SyntaxKind::Ident);
            } else {
                self.expect(SyntaxKind::Ident, "expected type name");
                if self.at(SyntaxKind::Var) {
                    self.bump();
                }
            }
            if self.at(SyntaxKind::Lt) {
                self.parse_type_args();
                type_boundary = true;
            }
            if !self.eat(SyntaxKind::Dot) {
                break;
            }
        }
        self.finish();
    }

    fn eat_primitive(&mut self) -> bool {
        match self.kind() {
            SyntaxKind::BooleanKw
            | SyntaxKind::Byte
            | SyntaxKind::CharKw
            | SyntaxKind::Short
            | SyntaxKind::IntKw
            | SyntaxKind::LongKw
            | SyntaxKind::FloatKw
            | SyntaxKind::DoubleKw
            | SyntaxKind::Void => {
                self.bump();
                true
            }
            _ => false,
        }
    }

    fn parse_type_args(&mut self) -> Arc<GreenNode> {
        self.open(SyntaxKind::TypeArgList);
        self.expect(SyntaxKind::Lt, "expected '<'");
        if !self.closes_args() {
            loop {
                self.parse_type();
                if self.eat(SyntaxKind::Comma) && !self.closes_args() {
                    continue;
                }
                break;
            }
        }
        if !self.eat_gt() {
            self.expect(SyntaxKind::Gt, "expected '>'");
        }
        self.finish()
    }

    fn closes_args(&self) -> bool {
        matches!(self.kind(), SyntaxKind::Gt | SyntaxKind::GtGt | SyntaxKind::GtGtGt)
    }

    fn parse_block(&mut self) -> Arc<GreenNode> {
        self.drain();
        self.open(SyntaxKind::Block);
        self.expect(SyntaxKind::LBrace, "expected '{'");
        self.parse_stmts();
        self.expect(SyntaxKind::RBrace, "expected '}'");
        self.finish()
    }

    fn parse_stmts(&mut self) {
        while !self.eof() && !self.at(SyntaxKind::RBrace) {
            let before = self.pos;
            self.parse_stmt();
            if self.pos == before {
                self.error_one("expected statement");
            }
        }
    }

    fn parse_stmt(&mut self) {
        match self.kind() {
            SyntaxKind::Semi => {
                self.open(SyntaxKind::EmptyStmt);
                self.bump();
                self.finish();
            }
            SyntaxKind::LBrace => {
                self.parse_block();
            }
            SyntaxKind::If => self.parse_if(),
            SyntaxKind::While => self.parse_while(),
            SyntaxKind::Synchronized => self.parse_synchronized(),
            SyntaxKind::Do => self.parse_do(),
            SyntaxKind::For => self.parse_for(),
            SyntaxKind::Return => self.parse_jump(SyntaxKind::ReturnStmt, true),
            SyntaxKind::Throw => self.parse_jump(SyntaxKind::ThrowStmt, true),
            SyntaxKind::Break => self.parse_jump(SyntaxKind::BreakStmt, false),
            SyntaxKind::Continue => self.parse_jump(SyntaxKind::ContinueStmt, false),
            SyntaxKind::Assert => self.parse_assert(),
            SyntaxKind::Try => self.parse_try(),
            SyntaxKind::Switch => self.parse_switch(),
            SyntaxKind::Ident | SyntaxKind::Var if self.nth(1) == SyntaxKind::Colon => self.parse_labeled(),
            _ if self.looks_like_local_type_decl() => self.parse_local_type(),
            _ if self.looks_like_decl() => self.parse_local(true),
            _ if self.can_start_expr() => {
                self.open(SyntaxKind::ExprStmt);
                self.parse_expr();
                self.expect(SyntaxKind::Semi, "expected ';'");
                self.finish();
            }
            _ => self.recover_stmt(),
        }
    }

    fn parse_labeled(&mut self) {
        // Consecutive prefixes are iterative, with a local tree-depth bound.
        // This is not a general statement/parser resource budget.
        let mut labels = 0;
        while is_identifier(self.kind()) && self.nth(1) == SyntaxKind::Colon {
            if labels == 32 {
                self.set_message("consecutive label limit exceeded");
                while is_identifier(self.kind()) && self.nth(1) == SyntaxKind::Colon {
                    self.bump_as(SyntaxKind::Ident);
                    self.bump();
                }
                break;
            }
            self.open(SyntaxKind::LabeledStmt);
            self.bump_as(SyntaxKind::Ident); // label is a token, never a value/type reference
            self.bump(); // colon (distinct from a method-reference :: token)
            labels += 1;
        }
        if self.looks_like_local_type_decl() || self.looks_like_decl() {
            // Block statements include declarations; a labeled Statement does
            // not. Keep recovery contained without exporting its declarations.
            self.set_message("declaration cannot be a labeled statement");
            self.parse_stmt();
        } else if matches!(
            self.kind(),
            SyntaxKind::RBrace
                | SyntaxKind::Eof
                | SyntaxKind::Else
                | SyntaxKind::Catch
                | SyntaxKind::Finally
                | SyntaxKind::Case
                | SyntaxKind::Default
        ) {
            self.set_message("expected labeled statement");
            // Leave the enclosing block/if/switch delimiter for its owner.
        } else {
            self.parse_stmt();
        }
        for _ in 0..labels {
            self.finish();
        }
    }

    fn looks_like_local_type_decl(&self) -> bool {
        // Recovery may reach the keyword in a malformed primitive `.class`
        // expression. It is not a declaration, even at a statement boundary.
        let follows_dot = self.tokens[..self.next_sig()]
            .iter()
            .rev()
            .find(|token| !token.kind.is_trivia())
            .is_some_and(|token| token.kind == SyntaxKind::Dot);
        !follows_dot && self.looks_like_type_decl()
    }

    // Keep unsupported local types contained, including nested bodies. Do not
    // parse their members as declarations of the surrounding class or block.
    fn parse_local_type(&mut self) {
        self.open(SyntaxKind::LocalType);
        self.set_message("local type bindings are unsupported");
        self.parse_modifier_list();
        if self.eat(SyntaxKind::At) {
            self.expect(SyntaxKind::InterfaceKw, "expected 'interface'");
        } else {
            self.bump(); // class, interface, enum or record
        }
        self.expect(SyntaxKind::Ident, "expected local type name");
        while !self.eof() && !matches!(self.kind(), SyntaxKind::RBrace | SyntaxKind::Semi) {
            match self.kind() {
                SyntaxKind::At => {
                    self.parse_annotation();
                    continue;
                }
                SyntaxKind::LBrace => {
                    // Header annotations were consumed above, so their array
                    // values cannot be confused with the local type body.
                    let mut braces = 1u32;
                    self.bump();
                    while !self.eof() && braces != 0 {
                        match self.kind() {
                            SyntaxKind::LBrace => braces += 1,
                            SyntaxKind::RBrace => braces -= 1,
                            _ => {}
                        }
                        self.bump();
                    }
                    self.finish();
                    return;
                }
                _ => {}
            }
            self.bump();
        }
        self.eat(SyntaxKind::Semi);
        self.finish();
    }

    fn parse_if(&mut self) {
        self.open(SyntaxKind::IfStmt);
        self.bump();
        self.expect(SyntaxKind::LParen, "expected '('");
        self.parse_expr();
        self.expect(SyntaxKind::RParen, "expected ')'");
        self.parse_stmt();
        if self.eat(SyntaxKind::Else) {
            self.parse_stmt();
        }
        self.finish();
    }

    fn parse_synchronized(&mut self) {
        self.open(SyntaxKind::SynchronizedStmt);
        self.bump();
        self.expect(SyntaxKind::LParen, "expected '('");
        if matches!(
            self.kind(),
            SyntaxKind::LBrace
                | SyntaxKind::RBrace
                | SyntaxKind::Else
                | SyntaxKind::Catch
                | SyntaxKind::Finally
                | SyntaxKind::Case
                | SyntaxKind::Default
                | SyntaxKind::Eof
        ) {
            // Preserve the required body or enclosing statement boundary.
            self.open(SyntaxKind::Error);
            self.set_message("expected monitor expression");
            self.finish();
        } else {
            self.parse_expr();
        }
        self.expect(SyntaxKind::RParen, "expected ')'");
        // Only the already-built header is checked, never the body.
        let invalid_header = self
            .stack
            .last()
            .expect("synchronized frame")
            .children
            .iter()
            .filter_map(GreenElement::as_node)
            .any(|node| !crate::green::collect_errors(node).is_empty());
        if invalid_header {
            self.set_message("invalid synchronized header");
        }
        if self.at(SyntaxKind::LBrace) {
            self.parse_block();
        } else {
            self.expect(SyntaxKind::LBrace, "expected synchronized block");
            self.set_message("expected synchronized block");
            // One recovered statement stays under the invalid node, so its
            // declarations cannot enter the surrounding lexical scope.
            if !matches!(
                self.kind(),
                SyntaxKind::RBrace
                    | SyntaxKind::Else
                    | SyntaxKind::Catch
                    | SyntaxKind::Finally
                    | SyntaxKind::Case
                    | SyntaxKind::Default
                    | SyntaxKind::Eof
            ) {
                self.parse_stmt();
            }
        }
        self.finish();
    }

    fn parse_while(&mut self) {
        self.open(SyntaxKind::WhileStmt);
        self.bump();
        self.expect(SyntaxKind::LParen, "expected '('");
        self.parse_expr();
        self.expect(SyntaxKind::RParen, "expected ')'");
        self.parse_stmt();
        self.finish();
    }

    fn parse_do(&mut self) {
        self.open(SyntaxKind::DoStmt);
        self.bump();
        self.parse_stmt();
        self.expect(SyntaxKind::While, "expected 'while'");
        self.expect(SyntaxKind::LParen, "expected '('");
        self.parse_expr();
        self.expect(SyntaxKind::RParen, "expected ')'");
        self.expect(SyntaxKind::Semi, "expected ';'");
        self.finish();
    }

    fn parse_for(&mut self) {
        self.open(SyntaxKind::ForStmt);
        self.bump();
        self.expect(SyntaxKind::LParen, "expected '('");
        if !self.at(SyntaxKind::Semi) && !self.at(SyntaxKind::RParen) {
            if self.looks_like_decl() {
                self.parse_local(false);
            } else {
                self.parse_for_expression_list();
            }
        }
        if self.eat(SyntaxKind::Colon) {
            self.parse_expr();
        } else {
            self.expect(SyntaxKind::Semi, "expected ';'");
            if !self.at(SyntaxKind::Semi) && !self.at(SyntaxKind::LBrace) && self.can_start_expr() {
                self.parse_expr();
            }
            self.expect(SyntaxKind::Semi, "expected ';'");
            if !self.at(SyntaxKind::RParen) {
                self.parse_for_expression_list();
            }
        }
        self.expect(SyntaxKind::RParen, "expected ')'");
        self.parse_stmt();
        self.finish();
    }

    /// JLS StatementExpressionList, confined to basic-for initialization/update.
    /// Nested argument/initializer commas belong to parse_expr; comma is never
    /// an expression operator. Empty items leave header delimiters untouched.
    fn parse_for_expression_list(&mut self) {
        loop {
            if self.can_start_expr() && !self.at(SyntaxKind::LBrace) {
                self.parse_expr();
                let expression = self.pop();
                let valid = expression.as_node().is_some_and(|node| match node.kind() {
                    SyntaxKind::CallExpr => true,
                    SyntaxKind::NewExpr => node.child_nodes().any(|n| n.kind() == SyntaxKind::ArgList),
                    SyntaxKind::UnaryExpr => node
                        .children()
                        .iter()
                        .filter_map(GreenElement::as_token)
                        .any(|t| matches!(t.kind(), SyntaxKind::PlusPlus | SyntaxKind::MinusMinus)),
                    SyntaxKind::BinaryExpr => node.children().iter().filter_map(GreenElement::as_token).any(|t| {
                        matches!(
                            t.kind(),
                            SyntaxKind::Eq
                                | SyntaxKind::PlusEq
                                | SyntaxKind::MinusEq
                                | SyntaxKind::StarEq
                                | SyntaxKind::SlashEq
                                | SyntaxKind::PercentEq
                                | SyntaxKind::AmpEq
                                | SyntaxKind::PipeEq
                                | SyntaxKind::CaretEq
                                | SyntaxKind::LtLtEq
                                | SyntaxKind::GtGtEq
                                | SyntaxKind::GtGtGtEq
                        )
                    }),
                    _ => false,
                });
                if valid {
                    self.push(expression);
                } else {
                    self.start(SyntaxKind::Error);
                    self.set_message("expected statement expression");
                    self.push(expression);
                    self.finish();
                }
            } else {
                self.open(SyntaxKind::Error);
                self.set_message("expected statement expression");
                self.finish();
            }
            if !self.eat(SyntaxKind::Comma) {
                break;
            }
        }
    }

    fn parse_jump(&mut self, kind: SyntaxKind, expr: bool) {
        self.start(kind);
        self.bump();
        if expr {
            if !self.at(SyntaxKind::Semi) && self.can_start_expr() {
                self.parse_expr();
            }
        } else if is_identifier(self.kind()) {
            self.bump_as(SyntaxKind::Ident);
        }
        self.expect(SyntaxKind::Semi, "expected ';'");
        self.finish();
    }

    fn parse_assert(&mut self) {
        self.open(SyntaxKind::AssertStmt);
        self.bump();
        self.parse_expr();
        if self.eat(SyntaxKind::Colon) {
            self.parse_expr();
        }
        self.expect(SyntaxKind::Semi, "expected ';'");
        self.finish();
    }

    fn parse_try(&mut self) {
        self.open(SyntaxKind::TryStmt);
        self.bump();
        if self.at(SyntaxKind::LParen) {
            self.bump();
            if !self.at(SyntaxKind::RParen) {
                loop {
                    self.open(SyntaxKind::Resource);
                    self.parse_modifier_list();
                    if self.at(SyntaxKind::Var) && is_identifier(self.nth(1)) {
                        self.bump();
                    } else {
                        self.parse_type();
                    }
                    self.expect_identifier("expected name");
                    self.expect(SyntaxKind::Eq, "expected '='");
                    self.parse_expr();
                    self.finish();
                    if self.at(SyntaxKind::RParen) || !self.eat(SyntaxKind::Semi) {
                        break;
                    }
                }
            }
            self.expect(SyntaxKind::RParen, "expected ')'");
        }
        if self.at(SyntaxKind::LBrace) {
            self.parse_block();
        }
        while self.at(SyntaxKind::Catch) {
            self.open(SyntaxKind::CatchClause);
            self.bump();
            self.expect(SyntaxKind::LParen, "expected '('");
            self.parse_catch_modifiers();
            self.parse_catch_type();
            while self.eat(SyntaxKind::Pipe) {
                self.parse_catch_type();
            }
            let named = is_identifier(self.kind());
            self.expect_identifier("expected name");
            self.parse_catch_dimensions();
            // Without a ')' an immediately following local declaration belongs
            // to the enclosing block, even if its initializer contains ')'.
            if !named || !self.looks_like_decl() {
                self.recover_catch_header();
            }
            self.expect(SyntaxKind::RParen, "expected ')'");
            if self.at(SyntaxKind::LBrace) {
                self.parse_block();
            } else {
                self.expect(SyntaxKind::LBrace, "expected catch body");
            }
            self.finish();
        }
        if self.eat(SyntaxKind::Finally) {
            self.parse_block();
        }
        self.finish();
    }

    // Catch parameters deliberately do not use ordinary formal-parameter grammar:
    // only annotations/final, reference types separated by '|', and one name.
    fn parse_catch_modifiers(&mut self) {
        if !self.has_modifier() {
            return;
        }
        self.open(SyntaxKind::ModifierList);
        let mut saw_final = false;
        while self.has_modifier() {
            if self.at(SyntaxKind::At) {
                self.parse_catch_annotation();
            } else if self.at(SyntaxKind::Final) && !saw_final {
                self.bump();
                saw_final = true;
            } else {
                self.error_one("invalid catch parameter modifier");
            }
        }
        self.finish();
    }

    fn parse_catch_type(&mut self) {
        self.parse_catch_type_part(false);
    }

    fn parse_catch_type_part(&mut self, argument: bool) {
        self.open(SyntaxKind::Type);
        while self.at(SyntaxKind::At) {
            self.parse_catch_annotation();
        }
        if argument && self.eat(SyntaxKind::Question) {
            if self.eat(SyntaxKind::Extends) || self.eat(SyntaxKind::Super) {
                self.parse_catch_type_part(true);
            }
        } else if argument && !self.at(SyntaxKind::Void) && self.eat_primitive() {
            // Primitive arrays are reference types; bare primitives are not.
            if !self.at(SyntaxKind::LBracket) && !self.at(SyntaxKind::At) {
                self.expect(SyntaxKind::LBracket, "expected array type argument");
            }
        } else {
            // Retain the direct Type/Name shape used by index_catch, while
            // keeping nested generic annotations on the bounded catch path.
            self.open(SyntaxKind::Name);
            let mut type_boundary = false;
            if type_segment(self.kind(), self.nth(1), false) {
                self.bump_as(SyntaxKind::Ident);
            } else {
                self.expect(SyntaxKind::Ident, "expected catch type name");
            }
            type_boundary |= self.at(SyntaxKind::Lt);
            self.parse_catch_type_args();
            while self.eat(SyntaxKind::Dot) {
                while self.at(SyntaxKind::At) {
                    type_boundary = true;
                    self.parse_catch_annotation();
                }
                if type_segment(self.kind(), self.nth(1), type_boundary) {
                    self.bump_as(SyntaxKind::Ident);
                } else {
                    self.expect(SyntaxKind::Ident, "expected catch type name");
                }
                type_boundary |= self.at(SyntaxKind::Lt);
                self.parse_catch_type_args();
            }
            self.finish();
        }
        if argument {
            self.parse_catch_dimensions();
        }
        self.finish();
    }

    fn parse_catch_type_args(&mut self) {
        if !self.at(SyntaxKind::Lt) {
            return;
        }
        self.open(SyntaxKind::TypeArgList);
        self.bump();
        loop {
            self.parse_catch_type_part(true);
            if !self.eat(SyntaxKind::Comma) {
                break;
            }
        }
        if !self.eat_gt() {
            self.expect(SyntaxKind::Gt, "expected '>'");
        }
        self.finish();
    }

    fn parse_catch_dimensions(&mut self) {
        while self.at(SyntaxKind::At) || self.at(SyntaxKind::LBracket) {
            while self.at(SyntaxKind::At) {
                self.parse_catch_annotation();
            }
            self.expect(SyntaxKind::LBracket, "expected '['");
            self.expect(SyntaxKind::RBracket, "expected ']'");
        }
    }

    /// Parse an annotation in isolation only when its delimiters and syntax are
    /// complete. A broken annotation must not interpret the catch body as an
    /// annotation array initializer or eat a subsequent declaration.
    fn parse_catch_annotation(&mut self) {
        self.drain();
        let start = self.pos;
        let mut look = Look::from(self);
        look.eat(SyntaxKind::At);
        if is_identifier(look.kind()) {
            look.bump();
        }
        while look.eat(SyntaxKind::Dot) && is_identifier(look.kind()) {
            look.bump();
        }
        let mut complete = true;
        if look.eat(SyntaxKind::LParen) {
            let mut delimiters = vec![SyntaxKind::RParen];
            while !delimiters.is_empty() {
                let kind = look.kind();
                match kind {
                    SyntaxKind::Eof | SyntaxKind::Semi | SyntaxKind::Catch | SyntaxKind::Finally => {
                        complete = false;
                        break;
                    }
                    SyntaxKind::LParen => delimiters.push(SyntaxKind::RParen),
                    SyntaxKind::LBrace => delimiters.push(SyntaxKind::RBrace),
                    SyntaxKind::LBracket => delimiters.push(SyntaxKind::RBracket),
                    SyntaxKind::RParen | SyntaxKind::RBrace | SyntaxKind::RBracket
                        if delimiters.pop() != Some(kind) =>
                    {
                        complete = false;
                        break;
                    }
                    _ => {}
                }
                look.bump();
            }
        }
        let end = look.i;
        if complete {
            let mut annotation = Parser::new(self.src, self.tokens[start..end].to_vec());
            annotation.start(SyntaxKind::ModifierList);
            annotation.parse_annotation();
            annotation.drain();
            let node = annotation.finish();
            if annotation.eof() && crate::green::collect_errors(&node).is_empty() {
                self.stack.last_mut().expect("frame").children.extend(node.children().iter().cloned());
                self.pos = end;
                return;
            }
        }
        self.error_one("incomplete or invalid catch annotation");
        // Keep malformed annotation material inside Error, not as a fake name.
        self.recover_catch_header();
    }

    // The first brace is a synchronization boundary. An unmatched annotation
    // array brace is not distinguishable from the intended body opener here;
    // this does not repair arbitrary brace imbalance in incomplete source.
    fn recover_catch_header(&mut self) {
        let mut look = Look::from(self);
        while !matches!(
            look.kind(),
            SyntaxKind::RParen
                | SyntaxKind::LBrace
                | SyntaxKind::RBrace
                | SyntaxKind::Semi
                | SyntaxKind::Catch
                | SyntaxKind::Finally
                | SyntaxKind::Eof
        ) {
            look.bump();
        }
        let has_close = look.kind() == SyntaxKind::RParen;
        while !matches!(
            self.kind(),
            SyntaxKind::RParen
                | SyntaxKind::LBrace
                | SyntaxKind::RBrace
                | SyntaxKind::Semi
                | SyntaxKind::Catch
                | SyntaxKind::Finally
                | SyntaxKind::Eof
        ) {
            if !has_close && self.looks_like_decl() {
                break;
            }
            self.error_one("unexpected catch parameter token");
        }
    }

    fn parse_switch(&mut self) {
        // Commit only a traditional colon statement. Unsupported labels remain
        // one opaque recovery node, never ordinary declarations in the caller.
        self.drain();
        let pos = self.pos;
        let depth = self.stack.len();
        let children = self.stack.last().map_or(0, |frame| frame.children.len());
        if !self.parse_switch_colon() {
            self.stack.truncate(depth);
            if let Some(parent) = self.stack.last_mut() {
                parent.children.truncate(children);
            }
            self.pos = pos;
            self.recover_switch();
        }
    }

    fn parse_switch_colon(&mut self) -> bool {
        self.open(SyntaxKind::SwitchStmt);
        self.bump();
        if !self.eat(SyntaxKind::LParen) {
            return false;
        }
        self.parse_expr();
        if !self.eat(SyntaxKind::RParen) || !self.at(SyntaxKind::LBrace) {
            return false;
        }
        if self
            .stack
            .last()
            .unwrap()
            .children
            .iter()
            .filter_map(|c| c.as_node())
            .any(|n| !crate::green::collect_errors(n).is_empty())
        {
            return false;
        }
        self.open(SyntaxKind::SwitchBody);
        self.bump();
        let mut labeled = false;
        while !self.eof() && !self.at(SyntaxKind::RBrace) {
            let before = self.pos;
            match self.kind() {
                SyntaxKind::Case | SyntaxKind::Default => {
                    self.open(SyntaxKind::SwitchLabel);
                    let default = self.at(SyntaxKind::Default);
                    self.bump();
                    if !default {
                        if self.at(SyntaxKind::Null) || !self.can_start_expr() {
                            return false;
                        }
                        self.parse_expr();
                    }
                    if !self.eat(SyntaxKind::Colon) {
                        return false;
                    }
                    let label = self.finish();
                    if !crate::green::collect_errors(&label).is_empty() || !Self::traditional_switch_label(&label) {
                        return false;
                    }
                    labeled = true;
                }
                _ if labeled => self.parse_stmt(),
                _ => return false,
            }
            if self.pos == before {
                return false;
            }
        }
        if !self.eat(SyntaxKind::RBrace) {
            return false;
        }
        self.finish();
        self.finish();
        true
    }

    fn traditional_switch_label(node: &GreenNode) -> bool {
        // No modern rule/pattern/null labels are silently reinterpreted as an
        // old constant label, even when nested inside an expression node.
        node.children().iter().all(|child| {
            if let Some(node) = child.as_node() {
                Self::traditional_switch_label(node)
            } else {
                child.as_token().is_none_or(|token| {
                    !matches!(token.kind(), SyntaxKind::Arrow | SyntaxKind::Null | SyntaxKind::When | SyntaxKind::Comma)
                })
            }
        })
    }

    fn recover_switch(&mut self) {
        self.open(SyntaxKind::Error);
        self.set_message("unsupported or malformed switch");
        self.bump();
        let mut depth = 0i32;
        let mut saw_brace = false;
        let mut parens = 0usize;
        while !self.eof() {
            match self.kind() {
                SyntaxKind::LParen if !saw_brace => {
                    parens += 1;
                    self.bump();
                }
                SyntaxKind::RParen if !saw_brace => {
                    parens = parens.saturating_sub(1);
                    self.bump();
                }
                SyntaxKind::LBrace => {
                    // A selector may itself contain an anonymous-class/array
                    // initializer body. Only a brace outside its parentheses
                    // begins the switch body.
                    if depth == 0 && parens == 0 {
                        saw_brace = true;
                    }
                    depth += 1;
                    self.bump();
                }
                SyntaxKind::RBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    self.bump();
                    if depth == 0 && saw_brace {
                        break;
                    }
                }
                SyntaxKind::Semi if depth == 0 => {
                    self.bump();
                    break;
                }
                _ => self.bump(),
            }
        }
        self.finish();
    }

    /// One unexpected token, then the next statement can parse.
    /// Same shape as `StatementParser.parseStatements`: a non-statement consumes
    /// a single token instead of skipping through the following declaration.
    fn recover_stmt(&mut self) {
        let kind = self.kind();
        self.open(SyntaxKind::Error);
        self.set_message(match kind {
            SyntaxKind::Else => "else without if",
            SyntaxKind::Catch => "catch without try",
            SyntaxKind::Finally => "finally without try",
            _ => "unexpected token",
        });
        if !self.eof() && kind != SyntaxKind::RBrace {
            self.bump();
        }
        self.finish();
    }

    fn parse_local(&mut self, semi: bool) {
        self.open(SyntaxKind::LocalVar);
        self.parse_modifier_list();
        if self.at(SyntaxKind::Var) && is_identifier(self.nth(1)) {
            self.bump();
            self.expect_identifier("expected name");
            if self.eat(SyntaxKind::Eq) {
                self.parse_expr();
            }
            while self.eat(SyntaxKind::Comma) {
                self.expect_identifier("expected name");
                if self.eat(SyntaxKind::Eq) {
                    self.parse_expr();
                }
            }
        } else {
            self.parse_type();
            self.parse_declarators();
        }
        if semi {
            self.expect(SyntaxKind::Semi, "expected ';'");
        }
        self.finish();
    }

    fn looks_like_decl(&self) -> bool {
        let mut look = Look::from(self);
        look.skip_mods();
        if look.kind() == SyntaxKind::Var && is_identifier(look.nth(1)) {
            return true;
        }
        look.scan_type() && is_identifier(look.kind())
    }

    fn can_start_expr(&self) -> bool {
        can_start_unary(self.kind())
            || self.at(SyntaxKind::Switch)
            || self.at(SyntaxKind::LBrace)
            || self.type_then_colon_colon()
            || Look::from(self).scan_primitive_class_literal()
    }

    fn type_args_then_colon_colon(&self) -> bool {
        let mut look = Look::from(self);
        look.scan_args() && look.kind() == SyntaxKind::ColonColon
    }

    fn looks_like_cast(&self) -> bool {
        let mut look = Look::from(self);
        if !look.eat(SyntaxKind::LParen) {
            return false;
        }
        let mut probe = look;
        probe.skip_annotations();
        let primitive = probe.is_primitive();
        if !look.scan_type() || !look.eat(SyntaxKind::RParen) {
            return false;
        }
        let next = look.kind();
        if matches!(next, SyntaxKind::PlusPlus | SyntaxKind::MinusMinus) {
            return primitive;
        }
        can_start_unary(next) || look.scan_primitive_class_literal()
    }

    fn parse_expr(&mut self) {
        self.parse_expr_bp(0);
    }

    fn parse_expr_bp(&mut self, min: u8) {
        self.parse_unary();
        loop {
            let op = self.kind();
            let Some((left_bp, right_bp)) = infix_power(op) else { break };
            if left_bp < min {
                break;
            }
            let left = self.pop();
            self.start(SyntaxKind::BinaryExpr);
            self.push(left);
            self.bump();
            if op == SyntaxKind::Instanceof {
                let mut look = Look::from(&*self);
                look.skip_mods();
                if look.scan_type() && is_identifier(look.kind()) {
                    self.open(SyntaxKind::TypePattern);
                    self.parse_modifier_list();
                    self.parse_type();
                    self.expect_identifier("expected pattern variable");
                    self.set_message("type pattern flow scope is unsupported");
                    self.finish();
                } else {
                    self.parse_type();
                }
            } else if op == SyntaxKind::Question {
                self.parse_expr_bp(0);
                self.expect(SyntaxKind::Colon, "expected ':'");
                self.parse_expr_bp(right_bp);
            } else {
                self.parse_expr_bp(right_bp);
            }
            self.finish();
        }
    }

    fn parse_unary(&mut self) {
        if matches!(
            self.kind(),
            SyntaxKind::PlusPlus
                | SyntaxKind::MinusMinus
                | SyntaxKind::Plus
                | SyntaxKind::Minus
                | SyntaxKind::Bang
                | SyntaxKind::Tilde
        ) {
            self.open(SyntaxKind::UnaryExpr);
            self.bump();
            self.parse_unary();
            self.finish();
        } else if self.lambda_decision().is_some() {
            // Same commit point as IntelliJ `parseLambdaAfterParenth`: a parameter
            // list is a lambda only when `->` follows it. Otherwise `(int) x` stays a cast.
            self.parse_lambda();
        } else if self.type_then_colon_colon() {
            self.parse_type_method_ref();
        } else if self.at(SyntaxKind::LParen) && self.looks_like_cast() {
            // The operand is a unary expression, so postfix binds inside the cast:
            // `(int) x.f` is a cast of `x.f`, not a field access on the cast.
            self.open(SyntaxKind::CastExpr);
            self.bump();
            self.parse_type();
            self.expect(SyntaxKind::RParen, "expected ')'");
            self.parse_unary();
            self.finish();
        } else {
            self.parse_primary();
            self.parse_postfix();
        }
    }

    /// `Some(true)` is a typed parameter list, `Some(false)` an untyped one.
    /// Mirrors the lookahead in `PrattExpressionParser.parseLambdaAfterParenth`
    /// (intellij-community 840d3be11dfdeada533d05052008b48816b6cc4e): `()`,
    /// `(a, b)`, `(a)`, modifiers or primitives, and a type followed by a name
    /// are lambdas only when `->` is next. A failed attempt is not parsed.
    fn lambda_decision(&self) -> Option<bool> {
        if is_identifier(self.kind()) && self.nth(1) == SyntaxKind::Arrow {
            return Some(false);
        }
        if !self.at(SyntaxKind::LParen) {
            return None;
        }
        let mut look = Look::from(self);
        look.bump();
        let first = look.kind();
        let mut second_look = look;
        second_look.bump();
        let second = second_look.kind();
        let mut third_look = second_look;
        third_look.bump();
        let third = third_look.kind();
        if first == SyntaxKind::RParen && second == SyntaxKind::Arrow {
            return Some(false);
        }
        if is_identifier(first)
            && (second == SyntaxKind::Comma || (second == SyntaxKind::RParen && third == SyntaxKind::Arrow))
        {
            return Some(false);
        }
        if is_identifier(first) && second == SyntaxKind::Arrow {
            return None;
        }
        let typed_start = first == SyntaxKind::At || first.is_modifier() || look.is_primitive() || is_identifier(first);
        if typed_start && self.typed_lambda_arrow() {
            return Some(true);
        }
        None
    }

    fn typed_lambda_arrow(&self) -> bool {
        let mut look = Look::from(self);
        if !look.eat(SyntaxKind::LParen) {
            return false;
        }
        if look.kind() == SyntaxKind::RParen {
            look.bump();
            return look.kind() == SyntaxKind::Arrow;
        }
        loop {
            look.skip_mods();
            if look.kind() == SyntaxKind::Var && is_identifier(look.nth(1)) {
                look.bump();
            } else if !look.scan_type() {
                return false;
            }
            if look.kind() == SyntaxKind::DotDotDot {
                look.bump();
            }
            if !is_identifier(look.kind()) {
                return false;
            }
            look.bump();
            if look.eat(SyntaxKind::Comma) && look.kind() != SyntaxKind::RParen {
                continue;
            }
            break;
        }
        look.eat(SyntaxKind::RParen) && look.kind() == SyntaxKind::Arrow
    }

    fn parse_lambda(&mut self) {
        let typed = self.lambda_decision().unwrap_or(false);
        self.open(SyntaxKind::LambdaExpr);
        if is_identifier(self.kind()) && self.nth(1) == SyntaxKind::Arrow {
            self.open(SyntaxKind::ParamList);
            self.open(SyntaxKind::Parameter);
            self.bump_as(SyntaxKind::Ident);
            self.finish();
            self.finish();
        } else {
            self.open(SyntaxKind::ParamList);
            self.expect(SyntaxKind::LParen, "expected '('");
            if !self.at(SyntaxKind::RParen) && !self.eof() {
                loop {
                    self.open(SyntaxKind::Parameter);
                    if typed {
                        self.parse_modifier_list();
                        if self.at(SyntaxKind::Var) && is_identifier(self.nth(1)) {
                            self.bump();
                        } else {
                            self.parse_type();
                        }
                        self.eat(SyntaxKind::DotDotDot);
                    }
                    self.expect_identifier("expected parameter name");
                    self.finish();
                    if !self.eat(SyntaxKind::Comma) || self.at(SyntaxKind::RParen) {
                        break;
                    }
                }
            }
            self.expect(SyntaxKind::RParen, "expected ')'");
            self.finish();
        }
        self.expect(SyntaxKind::Arrow, "expected '->'");
        if self.at(SyntaxKind::LBrace) {
            self.parse_block();
        } else if self.can_start_expr() {
            self.parse_expr();
        } else {
            self.open(SyntaxKind::Error);
            self.set_message("expected lambda body");
            self.finish();
        }
        self.finish();
    }

    fn type_then_colon_colon(&self) -> bool {
        let mut look = Look::from(self);
        if !(look.is_primitive() || is_identifier(look.kind())) {
            return false;
        }
        look.scan_type() && look.kind() == SyntaxKind::ColonColon
    }

    fn parse_type_method_ref(&mut self) {
        self.open(SyntaxKind::MethodRefExpr);
        self.parse_type();
        self.finish_method_ref_tail();
    }

    fn finish_method_ref_tail(&mut self) {
        self.expect(SyntaxKind::ColonColon, "expected '::'");
        if self.at(SyntaxKind::Lt) {
            self.parse_type_args();
        }
        if !self.eat_identifier() && !self.eat(SyntaxKind::New) {
            self.expect_identifier("expected name");
        }
        self.finish();
    }

    fn parse_primary(&mut self) {
        if Look::from(self).scan_primitive_class_literal() {
            self.open(SyntaxKind::ClassLiteralExpr);
            self.bump();
            while self.eat(SyntaxKind::LBracket) {
                self.bump(); // The complete lookahead proved an empty dimension.
            }
            self.bump(); // Dot.
            self.bump(); // Class keyword.
            self.finish();
            return;
        }
        match self.kind() {
            SyntaxKind::IntLiteral
            | SyntaxKind::LongLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::DoubleLiteral
            | SyntaxKind::CharLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::TextBlock
            | SyntaxKind::True
            | SyntaxKind::False
            | SyntaxKind::Null => self.leaf_expr(SyntaxKind::LiteralExpr),
            SyntaxKind::This => self.leaf_expr(SyntaxKind::ThisExpr),
            SyntaxKind::Super => self.leaf_expr(SyntaxKind::SuperExpr),
            SyntaxKind::Ident | SyntaxKind::Var => {
                self.open(SyntaxKind::ReferenceExpr);
                self.bump_as(SyntaxKind::Ident);
                self.finish();
            }
            SyntaxKind::LParen => {
                self.open(SyntaxKind::ParenExpr);
                self.bump();
                self.parse_expr();
                self.expect(SyntaxKind::RParen, "expected ')'");
                self.finish();
            }
            SyntaxKind::Switch => self.recover_switch(),
            SyntaxKind::New => self.parse_new(),
            SyntaxKind::LBrace => self.parse_array_init(),
            _ => {
                self.open(SyntaxKind::Error);
                self.set_message("expected expression");
                if !self.eof()
                    && !matches!(
                        self.kind(),
                        SyntaxKind::Semi
                            | SyntaxKind::RParen
                            | SyntaxKind::RBrace
                            | SyntaxKind::RBracket
                            | SyntaxKind::Comma
                    )
                {
                    self.bump();
                }
                self.finish();
            }
        }
    }

    fn leaf_expr(&mut self, kind: SyntaxKind) {
        self.open(kind);
        self.bump();
        self.finish();
    }

    fn parse_postfix(&mut self) {
        loop {
            if self.at(SyntaxKind::Lt) && self.type_args_then_colon_colon() {
                let base = self.pop();
                self.start(SyntaxKind::MethodRefExpr);
                self.push(base);
                self.parse_type_args();
                self.finish_method_ref_tail();
            } else if self.at(SyntaxKind::ColonColon) {
                let base = self.pop();
                self.start(SyntaxKind::MethodRefExpr);
                self.push(base);
                self.finish_method_ref_tail();
            } else if self.at(SyntaxKind::LParen) {
                let callee = self.pop();
                self.start(SyntaxKind::CallExpr);
                self.push(callee);
                self.parse_args();
                self.finish();
            } else if self.at(SyntaxKind::LBracket) {
                let base = self.pop();
                self.start(SyntaxKind::IndexExpr);
                self.push(base);
                self.bump();
                if !self.at(SyntaxKind::RBracket) {
                    self.parse_expr();
                }
                self.expect(SyntaxKind::RBracket, "expected ']'");
                self.finish();
            } else if self.at(SyntaxKind::Dot) {
                let base = self.pop();
                self.start(SyntaxKind::FieldExpr);
                self.push(base);
                self.bump();
                // Reuse the existing lookahead grammar. An incomplete
                // list keeps ordinary recovery; dot/less-than comparisons are
                // not reinterpreted as invocation type arguments.
                let mut look = Look::from(&*self);
                let typed = self.at(SyntaxKind::Lt) && look.scan_args() && is_identifier(look.kind());
                if typed {
                    let arguments = self.parse_type_args();
                    if !valid_invocation_type_args(&arguments, false) {
                        self.open(SyntaxKind::Error);
                        self.set_message("invalid explicit invocation type arguments");
                        self.finish();
                    }
                    self.expect_identifier("expected method name");
                    if !self.at(SyntaxKind::LParen) {
                        self.expect(SyntaxKind::LParen, "expected invocation arguments");
                    }
                } else if !self.eat_identifier()
                    && !self.eat(SyntaxKind::This)
                    && !self.eat(SyntaxKind::Super)
                    && !self.eat(SyntaxKind::ClassKw)
                {
                    self.expect_identifier("expected name");
                }
                self.finish();
            } else if matches!(self.kind(), SyntaxKind::PlusPlus | SyntaxKind::MinusMinus) {
                let base = self.pop();
                self.start(SyntaxKind::UnaryExpr);
                self.push(base);
                self.bump();
                self.finish();
            } else {
                break;
            }
        }
    }

    fn parse_new(&mut self) {
        self.open(SyntaxKind::NewExpr);
        self.bump();
        self.parse_creator_type();
        if self.at(SyntaxKind::LParen) {
            self.parse_args();
            if self.at(SyntaxKind::LBrace) {
                self.parse_body(SyntaxKind::ClassBody);
            }
        } else {
            let mut dims = false;
            while self.eat(SyntaxKind::LBracket) {
                dims = true;
                if !self.at(SyntaxKind::RBracket) {
                    self.parse_expr();
                }
                self.expect(SyntaxKind::RBracket, "expected ']'");
            }
            if !dims {
                self.expect(SyntaxKind::LParen, "expected '(' or '['");
            }
            if self.at(SyntaxKind::LBrace) {
                self.parse_array_init();
            }
        }
        self.finish();
    }

    fn parse_creator_type(&mut self) {
        self.open(SyntaxKind::Type);
        while self.at(SyntaxKind::At) {
            self.parse_annotation();
        }
        if !self.eat_primitive() {
            self.parse_type_rest(false);
        }
        self.finish();
    }

    fn parse_array_init(&mut self) {
        self.parse_array_values(false);
    }

    fn parse_array_values(&mut self, annotation_values: bool) {
        self.open(SyntaxKind::ArrayInit);
        self.expect(SyntaxKind::LBrace, "expected '{'");
        if !self.at(SyntaxKind::RBrace) {
            loop {
                if annotation_values {
                    self.parse_element_value();
                } else if self.at(SyntaxKind::LBrace) {
                    self.parse_array_init();
                } else {
                    self.parse_expr();
                }
                if !self.eat(SyntaxKind::Comma) || self.at(SyntaxKind::RBrace) {
                    break;
                }
            }
        }
        self.expect(SyntaxKind::RBrace, "expected '}'");
        self.finish();
    }

    fn parse_args(&mut self) {
        self.open(SyntaxKind::ArgList);
        self.expect(SyntaxKind::LParen, "expected '('");
        if !self.at(SyntaxKind::RParen) && !self.eof() {
            loop {
                self.parse_expr();
                if !self.eat(SyntaxKind::Comma) || self.at(SyntaxKind::RParen) {
                    break;
                }
            }
        }
        self.expect(SyntaxKind::RParen, "expected ')'");
        self.finish();
    }
}

#[derive(Clone, Copy)]
enum MemberShape {
    Initializer,
    Type,
    Constructor,
    Method,
    Field,
}

fn can_start_unary(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Ident
            | SyntaxKind::Var
            | SyntaxKind::IntLiteral
            | SyntaxKind::LongLiteral
            | SyntaxKind::FloatLiteral
            | SyntaxKind::DoubleLiteral
            | SyntaxKind::CharLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::TextBlock
            | SyntaxKind::True
            | SyntaxKind::False
            | SyntaxKind::Null
            | SyntaxKind::This
            | SyntaxKind::Super
            | SyntaxKind::New
            | SyntaxKind::LParen
            | SyntaxKind::Bang
            | SyntaxKind::Tilde
            | SyntaxKind::Plus
            | SyntaxKind::Minus
            | SyntaxKind::PlusPlus
            | SyntaxKind::MinusMinus
    )
}

fn infix_power(kind: SyntaxKind) -> Option<(u8, u8)> {
    Some(match kind {
        SyntaxKind::Eq
        | SyntaxKind::PlusEq
        | SyntaxKind::MinusEq
        | SyntaxKind::StarEq
        | SyntaxKind::SlashEq
        | SyntaxKind::PercentEq
        | SyntaxKind::AmpEq
        | SyntaxKind::PipeEq
        | SyntaxKind::CaretEq
        | SyntaxKind::LtLtEq
        | SyntaxKind::GtGtEq
        | SyntaxKind::GtGtGtEq => (2, 1),
        SyntaxKind::Question => (4, 3),
        SyntaxKind::PipePipe => (6, 6),
        SyntaxKind::AmpAmp => (8, 8),
        SyntaxKind::Pipe => (10, 10),
        SyntaxKind::Caret => (12, 12),
        SyntaxKind::Amp => (14, 14),
        SyntaxKind::EqEq | SyntaxKind::NotEq => (16, 16),
        SyntaxKind::Lt | SyntaxKind::Gt | SyntaxKind::LtEq | SyntaxKind::GtEq | SyntaxKind::Instanceof => (18, 18),
        SyntaxKind::LtLt | SyntaxKind::GtGt | SyntaxKind::GtGtGt => (20, 20),
        SyntaxKind::Plus | SyntaxKind::Minus => (22, 22),
        SyntaxKind::Star | SyntaxKind::Slash | SyntaxKind::Percent => (24, 24),
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct Look<'a> {
    tokens: &'a [Token],
    i: usize,
    pending: u8,
}

impl<'a> Look<'a> {
    fn from(parser: &'a Parser<'_>) -> Self {
        Self { tokens: &parser.tokens, i: parser.pos, pending: 0 }
    }

    fn skip(&mut self) {
        while self.i < self.tokens.len() && self.tokens[self.i].kind.is_trivia() {
            self.i += 1;
        }
    }

    fn kind(&mut self) -> SyntaxKind {
        if self.pending > 0 {
            return SyntaxKind::Gt;
        }
        self.skip();
        if self.i >= self.tokens.len() { SyntaxKind::Eof } else { self.tokens[self.i].kind }
    }

    /// `n == 0` is the current non-trivia token.
    fn nth(&mut self, n: usize) -> SyntaxKind {
        let saved = *self;
        for _ in 0..n {
            self.bump();
        }
        let kind = self.kind();
        *self = saved;
        kind
    }

    fn bump(&mut self) {
        if self.pending > 0 {
            self.pending -= 1;
            return;
        }
        self.skip();
        if self.i < self.tokens.len() {
            self.i += 1;
        }
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.kind() == kind {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_gt(&mut self) -> bool {
        if self.pending > 0 {
            self.pending -= 1;
            return true;
        }
        self.skip();
        if self.i >= self.tokens.len() {
            return false;
        }
        match self.tokens[self.i].kind {
            SyntaxKind::Gt => {
                self.i += 1;
                true
            }
            SyntaxKind::GtGt => {
                self.i += 1;
                self.pending = 1;
                true
            }
            SyntaxKind::GtGtGt => {
                self.i += 1;
                self.pending = 2;
                true
            }
            _ => false,
        }
    }

    fn is_gt(&mut self) -> bool {
        matches!(self.kind(), SyntaxKind::Gt | SyntaxKind::GtGt | SyntaxKind::GtGtGt)
    }

    fn is_primitive(&mut self) -> bool {
        matches!(
            self.kind(),
            SyntaxKind::BooleanKw
                | SyntaxKind::Byte
                | SyntaxKind::CharKw
                | SyntaxKind::Short
                | SyntaxKind::IntKw
                | SyntaxKind::LongKw
                | SyntaxKind::FloatKw
                | SyntaxKind::DoubleKw
                | SyntaxKind::Void
        )
    }

    fn scan_primitive_class_literal(&mut self) -> bool {
        if !self.is_primitive() {
            return false;
        }
        let is_void = self.kind() == SyntaxKind::Void;
        self.bump();
        if !is_void {
            while self.eat(SyntaxKind::LBracket) {
                if !self.eat(SyntaxKind::RBracket) {
                    return false;
                }
            }
        }
        self.eat(SyntaxKind::Dot) && self.eat(SyntaxKind::ClassKw)
    }

    fn skip_annotations(&mut self) {
        while self.kind() == SyntaxKind::At {
            self.skip_annotation();
        }
    }

    fn skip_mods(&mut self) {
        loop {
            if self.kind() == SyntaxKind::At && self.nth(1) != SyntaxKind::InterfaceKw {
                self.skip_annotation();
            } else if self.kind().is_modifier() {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn skip_annotation(&mut self) {
        if !self.eat(SyntaxKind::At) {
            return;
        }
        if is_identifier(self.kind()) {
            self.bump();
            while self.eat(SyntaxKind::Dot) && is_identifier(self.kind()) {
                self.bump();
            }
        }
        if self.eat(SyntaxKind::LParen) {
            let mut depth = 1i32;
            while self.kind() != SyntaxKind::Eof && depth > 0 {
                match self.kind() {
                    SyntaxKind::LParen => depth += 1,
                    SyntaxKind::RParen => depth -= 1,
                    _ => {}
                }
                self.bump();
            }
        }
    }

    fn scan_type(&mut self) -> bool {
        self.skip_annotations();
        if self.eat(SyntaxKind::Question) {
            if self.eat(SyntaxKind::Extends) || self.eat(SyntaxKind::Super) {
                return self.scan_type();
            }
            return true;
        }
        if self.is_primitive() {
            self.bump();
        } else if !self.scan_class() {
            return false;
        }
        while self.eat(SyntaxKind::LBracket) {
            if !self.eat(SyntaxKind::RBracket) {
                return false;
            }
        }
        true
    }

    fn scan_class(&mut self) -> bool {
        let mut type_boundary = false;
        loop {
            if !type_segment(self.kind(), self.nth(1), type_boundary) {
                return false;
            }
            self.bump();
            if self.kind() == SyntaxKind::Lt {
                if !self.scan_args() {
                    return false;
                }
                type_boundary = true;
            }
            if !self.eat(SyntaxKind::Dot) {
                return true;
            }
        }
    }

    fn scan_args(&mut self) -> bool {
        if !self.eat(SyntaxKind::Lt) {
            return false;
        }
        if self.is_gt() {
            return self.eat_gt();
        }
        loop {
            if !self.scan_type() {
                return false;
            }
            if self.eat(SyntaxKind::Comma) && !self.is_gt() {
                continue;
            }
            return self.eat_gt();
        }
    }

    fn scan_type_params(&mut self) -> bool {
        if !self.eat(SyntaxKind::Lt) {
            return false;
        }
        loop {
            if self.kind() != SyntaxKind::Ident {
                return false;
            }
            self.bump();
            if self.eat(SyntaxKind::Extends) {
                if !self.scan_type() {
                    return false;
                }
                while self.eat(SyntaxKind::Amp) {
                    if !self.scan_type() {
                        return false;
                    }
                }
            }
            if !self.eat(SyntaxKind::Comma) {
                break;
            }
        }
        self.eat_gt()
    }
}
