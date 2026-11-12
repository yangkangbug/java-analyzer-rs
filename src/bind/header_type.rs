//! Syntax-only declaration inventory evidence, not a usable Java type.
//! Keep ancestor grammar and strict invocation/return evidence independent.
use super::{GreenNode, REFERENCE_PROOF_DEPTH, REFERENCE_PROOF_WORK, SyntaxKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Return,
    Argument,
}

pub(super) fn complete_return_type(node: &GreenNode) -> bool {
    HeaderTypeProof { work_left: REFERENCE_PROOF_WORK }.ty(node, Role::Return, 0)
}

struct HeaderTypeProof {
    work_left: usize,
}

impl HeaderTypeProof {
    fn enter(&mut self, node: &GreenNode, depth: usize) -> bool {
        if depth > REFERENCE_PROOF_DEPTH {
            return false;
        }
        let Some(left) = self.work_left.checked_sub(node.children().len().saturating_add(1)) else {
            return false;
        };
        self.work_left = left;
        true
    }

    fn ty(&mut self, node: &GreenNode, role: Role, depth: usize) -> bool {
        if node.kind() != SyntaxKind::Type || !self.enter(node, depth) {
            return false;
        }
        // Wildcard bounds are Question + Extends/Super + Name in one Type node.
        let mut wildcard = 0;
        let mut base = false;
        let mut primitive = None;
        let mut rank = 0;
        let mut open_dimension = false;
        for child in node.children() {
            if let Some(token) = child.as_token() {
                if token.kind().is_trivia() {
                    continue;
                }
                match token.kind() {
                    SyntaxKind::Question if !base && wildcard == 0 && role == Role::Argument => wildcard = 1,
                    SyntaxKind::Extends | SyntaxKind::Super if !base && wildcard == 1 => wildcard = 2,
                    kind @ (SyntaxKind::BooleanKw
                    | SyntaxKind::Byte
                    | SyntaxKind::CharKw
                    | SyntaxKind::Short
                    | SyntaxKind::IntKw
                    | SyntaxKind::LongKw
                    | SyntaxKind::FloatKw
                    | SyntaxKind::DoubleKw
                    | SyntaxKind::Void)
                        if !base && wildcard != 1 =>
                    {
                        base = true;
                        primitive = Some(kind);
                    }
                    SyntaxKind::LBracket if base && !open_dimension && rank < 255 => {
                        open_dimension = true;
                        rank += 1;
                    }
                    SyntaxKind::RBracket if open_dimension => open_dimension = false,
                    _ => return false,
                }
            } else if let Some(name) = child.as_node() {
                if base || wildcard == 1 || name.kind() != SyntaxKind::Name || !self.name(name, depth) {
                    return false;
                }
                base = true;
            }
        }
        if wildcard == 1 {
            return !base; // A bare ?, never ?[] or a wildcard as an ordinary return.
        }
        if !base || open_dimension {
            return false;
        }
        match primitive {
            Some(SyntaxKind::Void) => role == Role::Return && wildcard == 0 && rank == 0,
            Some(_) => (role == Role::Return && wildcard == 0) || rank > 0,
            None => true,
        }
    }

    fn name(&mut self, node: &GreenNode, depth: usize) -> bool {
        if !self.enter(node, depth) {
            return false;
        }
        let mut state = 0;
        for child in node.children() {
            if let Some(token) = child.as_token() {
                if token.kind().is_trivia() {
                    continue;
                }
                state = match (state, token.kind()) {
                    (0, SyntaxKind::Ident) => 1,
                    (1 | 2, SyntaxKind::Dot) => 0,
                    _ => return false,
                };
            } else if let Some(args) = child.as_node() {
                if state != 1 || args.kind() != SyntaxKind::TypeArgList || !self.arguments(args, depth) {
                    return false;
                }
                state = 2;
            }
        }
        matches!(state, 1 | 2)
    }

    fn arguments(&mut self, node: &GreenNode, depth: usize) -> bool {
        if !self.enter(node, depth) {
            return false;
        }
        let mut state = 0;
        for child in node.children() {
            if let Some(token) = child.as_token() {
                if token.kind().is_trivia() {
                    continue;
                }
                state = match (state, token.kind()) {
                    (0, SyntaxKind::Lt) => 1,
                    (2, SyntaxKind::Comma) => 3,
                    (2, SyntaxKind::Gt) => 4,
                    _ => return false,
                };
            } else if let Some(ty) = child.as_node() {
                if !matches!(state, 1 | 3) || !self.ty(ty, Role::Argument, depth + 1) {
                    return false;
                }
                state = 2;
            }
        }
        state == 4 // Empty/diamond and trailing-comma lists never prove inventory.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParsedFile;

    fn first_type(node: &GreenNode) -> Option<&GreenNode> {
        if node.kind() == SyntaxKind::Type {
            return Some(node);
        }
        node.child_nodes().find_map(|child| first_type(child))
    }

    fn node_cost(node: &GreenNode) -> usize {
        node.children().len() + 1 + node.child_nodes().map(|child| node_cost(child)).sum::<usize>()
    }

    #[test]
    fn work_accounting_covers_each_node_and_its_children_at_the_exact_boundary() {
        let parsed = ParsedFile::parse("class Host { Box<? extends Value[]> other() { return null; } }");
        assert!(parsed.errors().is_empty());
        let ty = first_type(parsed.root()).unwrap();
        let cost = node_cost(ty);
        let mut enough = HeaderTypeProof { work_left: cost };
        assert!(enough.ty(ty, Role::Return, 0));
        assert_eq!(enough.work_left, 0);
        assert!(!HeaderTypeProof { work_left: cost - 1 }.ty(ty, Role::Return, 0));
    }
}
