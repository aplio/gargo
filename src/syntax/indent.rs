use ropey::Rope;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, Tree};

// ---------------------------------------------------------------------------
// Indent query constants per language
// ---------------------------------------------------------------------------

pub const RUST_INDENT_QUERY: &str = r#"[
  (block)
  (declaration_list)
  (field_declaration_list)
  (enum_variant_list)
  (match_block)
  (use_list)
  (arguments)
  (parameters)
  (array_expression)
  (tuple_expression)
  (field_initializer_list)
] @indent"#;

pub const JAVASCRIPT_INDENT_QUERY: &str = r#"[
  (statement_block)
  (class_body)
  (object)
  (array)
  (arguments)
  (formal_parameters)
  (switch_body)
] @indent"#;

pub const TYPESCRIPT_INDENT_QUERY: &str = r#"[
  (statement_block)
  (class_body)
  (object)
  (array)
  (arguments)
  (formal_parameters)
  (switch_body)
  (object_type)
  (enum_body)
  (interface_body)
  (type_parameters)
] @indent"#;

pub const GO_INDENT_QUERY: &str = r#"[
  (block)
  (literal_value)
  (field_declaration_list)
  (argument_list)
  (parameter_list)
  (interface_type)
] @indent"#;

pub const C_INDENT_QUERY: &str = r#"[
  (compound_statement)
  (field_declaration_list)
  (enumerator_list)
  (initializer_list)
  (argument_list)
  (parameter_list)
] @indent"#;

pub const JSON_INDENT_QUERY: &str = r#"[
  (object)
  (array)
] @indent"#;

// ---------------------------------------------------------------------------
// Core functions
// ---------------------------------------------------------------------------

/// Run indent query on tree, count `@indent` nodes containing `cursor_byte`.
///
/// A node "contains" the cursor when `cursor_byte > node.start_byte()` AND
/// `cursor_byte < node.end_byte()` (strict interior – so when the cursor sits
/// right on a closing brace that node is NOT counted).
pub fn calculate_indent_level(
    tree: &Tree,
    indent_query: &Query,
    source: &[u8],
    cursor_byte: usize,
) -> usize {
    let root = tree.root_node();
    let mut query_cursor = tree_sitter::QueryCursor::new();
    let mut captures = query_cursor.captures(indent_query, root, source);

    let mut level: usize = 0;
    while let Some((match_, capture_idx)) = captures.next() {
        let capture = &match_.captures[*capture_idx];
        let node = capture.node;
        if cursor_byte > node.start_byte() && cursor_byte < node.end_byte() {
            level += 1;
        }
    }
    level
}

/// Convert an indent level to a spaces string.
pub fn indent_string(level: usize, tab_width: usize) -> String {
    " ".repeat(level * tab_width)
}

/// Fallback: extract leading whitespace from the given line.
pub fn copy_line_indent(rope: &Rope, line_idx: usize) -> String {
    if line_idx >= rope.len_lines() {
        return String::new();
    }
    let line = rope.line(line_idx);
    let mut indent = String::new();
    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            indent.push(ch);
        } else {
            break;
        }
    }
    indent
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_string_zero() {
        assert_eq!(indent_string(0, 4), "");
    }

    #[test]
    fn indent_string_one_level() {
        assert_eq!(indent_string(1, 4), "    ");
    }

    #[test]
    fn indent_string_two_levels() {
        assert_eq!(indent_string(2, 4), "        ");
    }

    #[test]
    fn indent_string_custom_tab_width() {
        assert_eq!(indent_string(1, 2), "  ");
        assert_eq!(indent_string(2, 2), "    ");
    }

    #[test]
    fn copy_line_indent_spaces() {
        let rope = Rope::from_str("    hello\n");
        assert_eq!(copy_line_indent(&rope, 0), "    ");
    }

    #[test]
    fn copy_line_indent_tabs() {
        let rope = Rope::from_str("\t\thello\n");
        assert_eq!(copy_line_indent(&rope, 0), "\t\t");
    }

    #[test]
    fn copy_line_indent_no_indent() {
        let rope = Rope::from_str("hello\n");
        assert_eq!(copy_line_indent(&rope, 0), "");
    }

    #[test]
    fn copy_line_indent_out_of_range() {
        let rope = Rope::from_str("hello\n");
        assert_eq!(copy_line_indent(&rope, 99), "");
    }

    #[test]
    fn calculate_indent_rust_block() {
        let source = b"fn main() {\n    let x = 1;\n}\n";
        let lang = tree_sitter::Language::new(tree_sitter_rust::LANGUAGE);
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&lang).unwrap();
        let tree = parser.parse(&source[..], None).unwrap();
        let query = Query::new(&lang, RUST_INDENT_QUERY).unwrap();

        // Cursor inside the block body (byte offset of 'l' in 'let')
        let cursor_byte = 16; // inside "    let x = 1;"
        let level = calculate_indent_level(&tree, &query, source, cursor_byte);
        assert_eq!(level, 1);
    }

    #[test]
    fn calculate_indent_outside_block() {
        let source = b"fn main() {\n    let x = 1;\n}\nfn foo() {}\n";
        let lang = tree_sitter::Language::new(tree_sitter_rust::LANGUAGE);
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&lang).unwrap();
        let tree = parser.parse(&source[..], None).unwrap();
        let query = Query::new(&lang, RUST_INDENT_QUERY).unwrap();

        // Cursor after the closing brace, at "fn foo"
        let cursor_byte = 29; // "fn foo"
        let level = calculate_indent_level(&tree, &query, source, cursor_byte);
        assert_eq!(level, 0);
    }

    #[test]
    fn calculate_indent_nested_blocks() {
        let source = b"fn main() {\n    if true {\n        let x = 1;\n    }\n}\n";
        let lang = tree_sitter::Language::new(tree_sitter_rust::LANGUAGE);
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&lang).unwrap();
        let tree = parser.parse(&source[..], None).unwrap();
        let query = Query::new(&lang, RUST_INDENT_QUERY).unwrap();

        // Cursor inside the inner block (at "let x")
        let cursor_byte = 34; // inside "        let x = 1;"
        let level = calculate_indent_level(&tree, &query, source, cursor_byte);
        assert_eq!(level, 2);
    }
}
