//! Brace-block reparse.
//!
//! Copyright 2000-2025 JetBrains s.r.o. and contributors.
//! Copyright 2026 OpenIDEA contributors.
//!
//! `balanced_brace_block` reimplements `SyntaxBuilderUtil.checkBraceBalance`
//! (`requireOuterBraces = true`) from intellij-community commit
//! 840d3be11dfdeada533d05052008b48816b6cc4e. See `NOTICE` in this crate.
//! The surrounding edit driver is original. It does not use PSI, VirtualFile,
//! or ProgressManager.

use std::sync::Arc;

use crate::green::{GreenElement, GreenNode};
use crate::kind::SyntaxKind;
use crate::lex::{Token, lex};
use crate::parse::parse_fragment;

/// True when `tokens` are a `{ ... }` block whose braces balance and the
/// closing `}` is the last token. Trivia after that `}` makes this false,
/// matching the IntelliJ check, so callers must not include it.
pub fn balanced_brace_block(tokens: &[Token]) -> bool {
    let mut index = 0;
    let current = |index: usize| tokens.get(index).map(|token| token.kind);
    if current(index) != Some(SyntaxKind::LBrace) {
        return false;
    }
    index += 1;
    let mut balance = 1i32;
    loop {
        let kind = current(index);
        let Some(kind) = kind else {
            return balance == 0;
        };
        if balance == 0 {
            return false;
        }
        if kind == SyntaxKind::LBrace {
            balance += 1;
        } else if kind == SyntaxKind::RBrace {
            balance -= 1;
        }
        index += 1;
    }
}

#[derive(Clone, Debug)]
struct Hit {
    path: Vec<usize>,
    kind: SyntaxKind,
    start: u32,
    end: u32,
}

pub fn reparse_root(
    root: &Arc<GreenNode>,
    old: &str,
    new_text: &str,
    edit_start: u32,
    edit_end: u32,
) -> Option<(Arc<GreenNode>, SyntaxKind)> {
    let mut hits = Vec::new();
    let mut path = Vec::new();
    collect(root, 0, edit_start, edit_end, &mut path, &mut hits);
    hits.sort_by_key(|hit| std::cmp::Reverse(hit.path.len()));
    let delta = new_text.len() as i32 - old.len() as i32;
    for hit in hits {
        let new_start = hit.start as usize;
        let new_end = hit.end as i32 + delta;
        if new_end < new_start as i32 {
            continue;
        }
        let new_end = new_end as usize;
        if new_end > new_text.len() || !new_text.is_char_boundary(new_start) || !new_text.is_char_boundary(new_end) {
            continue;
        }
        let slice = &new_text[new_start..new_end];
        let tokens = lex(slice);
        if !balanced_brace_block(&tokens) {
            continue;
        }
        let replacement = parse_fragment(hit.kind, slice);
        // Recovery can stop early or consume the closing brace in a descendant.
        // A balanced token slice and complete consumption do not prove the
        // replacement root itself owns the fragment's opening/closing braces.
        if replacement.kind() != hit.kind
            || replacement.utf8_len() as usize != slice.len()
            || brace_interior(&replacement, 0).is_none()
        {
            continue;
        }
        return Some((splice(root, &hit.path, replacement), hit.kind));
    }
    None
}

fn collect(node: &GreenNode, base: u32, edit_start: u32, edit_end: u32, path: &mut Vec<usize>, hits: &mut Vec<Hit>) {
    if node.kind().is_reparseable()
        && let Some((open_end, close_start)) = brace_interior(node, base)
        && edit_start >= open_end
        && edit_end <= close_start
    {
        hits.push(Hit { path: path.clone(), kind: node.kind(), start: base, end: base + node.utf8_len() });
    }
    // Recovery may have reclassified the surrounding construct (for example,
    // a broken annotation becomes an error followed by an ordinary expression).
    // Recheck that decision at this node or its nearest reparseable ancestor,
    // rather than repairing a descendant while retaining stale recovery siblings.
    if node.child_nodes().any(|child| child.kind() == SyntaxKind::Error) {
        return;
    }
    let mut offset = base;
    for (index, child) in node.children().iter().enumerate() {
        if let Some(child) = child.as_node() {
            // Selector errors and label errors/unsupported tokens can roll back
            // the entire switch. Reparse its enclosing block rather than splice
            // a nested block without rechecking that commit boundary. Ordinary
            // statement blocks in the switch body remain independently eligible.
            // Annotation contents also affect enclosing declaration lookahead
            // and catch-annotation validation, not just their nested block tree.
            if !matches!(node.kind(), SyntaxKind::SwitchLabel | SyntaxKind::Annotation | SyntaxKind::AnnotationType | SyntaxKind::AnnotationDefault)
                && (node.kind() != SyntaxKind::SwitchStmt || child.kind() == SyntaxKind::SwitchBody)
                // Monitor errors determine the synchronized header's validity.
                // Recheck that ancestor instead of splicing a lambda/anonymous
                // body inside the monitor. Its ordinary body remains eligible.
                && (node.kind() != SyntaxKind::SynchronizedStmt || child.kind() == SyntaxKind::Block)
            {
                path.push(index);
                collect(child, offset, edit_start, edit_end, path, hits);
                path.pop();
            }
        }
        offset += child.utf8_len();
    }
}

fn brace_interior(node: &GreenNode, base: u32) -> Option<(u32, u32)> {
    // Both the retained node and a replacement must directly own their exact
    // boundary tokens. Recovery nodes inside either boundary cannot certify it.
    let first = node.children().first()?.as_token()?;
    let last = node.children().last()?.as_token()?;
    if first.kind() != SyntaxKind::LBrace || last.kind() != SyntaxKind::RBrace {
        return None;
    }
    Some((base + first.utf8_len(), base + node.utf8_len() - last.utf8_len()))
}

fn splice(node: &Arc<GreenNode>, path: &[usize], replacement: Arc<GreenNode>) -> Arc<GreenNode> {
    if path.is_empty() {
        return replacement;
    }
    let index = path[0];
    let mut children = node.children().to_vec();
    let GreenElement::Node(child) = &children[index] else {
        return Arc::clone(node);
    };
    children[index] = GreenElement::Node(splice(child, &path[1..], replacement));
    GreenNode::from_parts(node.kind(), children, node.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::lex;

    #[test]
    fn brace_rule_matches_the_intellij_check() {
        let balanced = |text: &str| balanced_brace_block(&lex(text));
        assert!(balanced("{ }"));
        assert!(balanced("{ { } }"));
        assert!(balanced(r#"{ String s = "{"; }"#));
        assert!(!balanced("{"));
        assert!(!balanced("{ } }"));
        assert!(!balanced("{ } x"));
        assert!(!balanced("x"));
    }
}
