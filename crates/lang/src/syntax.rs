//! Tree-sitter plumbing shared by language frontends.

use std::ops::ControlFlow;
use std::time::{Duration, Instant};

use ripplepath_core::{Fingerprint, FingerprintBuilder, Span};
use tree_sitter::{Node, ParseOptions, Parser, Tree};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("grammar could not be loaded: {0}")]
    Grammar(String),
    #[error("parsing exceeded the {}ms budget", .0.as_millis())]
    Timeout(Duration),
}

/// Parses with a wall-clock budget. Tree-sitter is fast on real code, but adversarial input
/// (pathological nesting, megabytes of one expression) can make any GLR-style parser slow; a
/// budget turns that into a recorded parse failure instead of a hung analysis.
pub fn parse(language: &tree_sitter::Language, source: &str, budget: Duration) -> Result<Tree, ParseError> {
    let mut parser = Parser::new();
    parser.set_language(language).map_err(|e| ParseError::Grammar(e.to_string()))?;
    let deadline = Instant::now() + budget;
    let mut on_progress = |_: &tree_sitter::ParseState| {
        if Instant::now() > deadline { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
    };
    let options = ParseOptions::new().progress_callback(&mut on_progress);
    let bytes = source.as_bytes();
    let mut read = |offset: usize, _| if offset < bytes.len() { &bytes[offset..] } else { &[][..] };
    parser.parse_with_options(&mut read, None, Some(options)).ok_or(ParseError::Timeout(budget))
}

pub fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

pub fn line(node: Node<'_>) -> u32 {
    node.start_position().row as u32 + 1
}

pub fn span(node: Node<'_>) -> Span {
    Span { start_line: line(node), end_line: node.end_position().row as u32 + 1 }
}

pub fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Lines with ERROR or MISSING nodes, capped: the point is to flag the file, not to list every
/// cascade error a broken file produces.
pub fn syntax_error_lines(tree: &Tree) -> Vec<u32> {
    const CAP: usize = 20;
    let root = tree.root_node();
    if !root.has_error() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            lines.push(line(node));
            if lines.len() >= CAP {
                break;
            }
            continue;
        }
        if node.has_error() {
            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Fingerprint of the leaf tokens under `node`, skipping comments and the subtrees in `exclude`.
///
/// Hashing tokens rather than raw text makes formatting- and comment-only edits invisible, which
/// is what "did this symbol change?" should mean. Excluding nested members gives each symbol its
/// *own* fingerprint so that editing a method does not mark its class modified.
pub fn fingerprint(node: Node<'_>, source: &str, exclude: &[usize], is_comment: impl Fn(&str) -> bool) -> Fingerprint {
    let mut builder = FingerprintBuilder::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if exclude.contains(&current.id()) || is_comment(current.kind()) {
            continue;
        }
        if current.child_count() == 0 {
            if current.is_missing() {
                builder.token(current.kind());
            } else {
                builder.token(text(current, source));
            }
            continue;
        }
        let mut cursor = current.walk();
        let children: Vec<Node<'_>> = current.children(&mut cursor).collect();
        // Reverse so tokens are popped in source order; order is part of the fingerprint.
        stack.extend(children.into_iter().rev());
    }
    builder.finish()
}
