use std::fmt;

use tree_sitter::{Node, Parser, Tree};

use crate::{DagError, TextEdit};

/// A DAG file held in memory together with its syntax tree.
pub struct DagDocument {
    name: String,
    text: String,
    tree: Tree,
}

impl DagDocument {
    /// Parses `text` as Python; `name` identifies the DAG in errors.
    ///
    /// # Errors
    ///
    /// [`DagError::Unparseable`] when the text cannot be parsed as Python.
    pub fn parse(name: impl Into<String>, text: impl Into<String>) -> Result<Self, DagError> {
        let name = name.into();
        let text = text.into();
        let tree =
            parse_python(&text).ok_or_else(|| DagError::Unparseable { dag: name.clone() })?;
        Ok(Self { name, text, tree })
    }

    /// The DAG's name (its file name).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The root node of the syntax tree.
    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// The source text of `node`.
    pub fn node_text(&self, node: Node<'_>) -> &str {
        &self.text[node.byte_range()]
    }

    /// Returns a new document with the edits applied and re-parsed.
    ///
    /// # Errors
    ///
    /// [`DagError::OverlappingEdits`] when `edits` overlap; [`DagError::Unparseable`] when the result is not parseable
    /// Python. `transformer` names the transformer in the error.
    pub fn with_edits(&self, edits: Vec<TextEdit>, transformer: &str) -> Result<Self, DagError> {
        let text =
            TextEdit::apply_all(&self.text, edits).ok_or_else(|| DagError::OverlappingEdits {
                dag: self.name.clone(),
                transformer: transformer.to_owned(),
            })?;
        Self::parse(self.name.clone(), text)
    }

    /// All nodes in document order.
    pub fn descendants(&self) -> Vec<Node<'_>> {
        let mut nodes = Vec::new();
        let mut cursor = self.tree.walk();
        let mut stack = vec![self.root()];
        while let Some(node) = stack.pop() {
            nodes.push(node);
            let children: Vec<_> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
        nodes
    }

    /// `name = <call to function>(...)` assignments, as (assignment, call) pairs.
    pub fn call_assignments<'t>(
        &'t self,
        variable: &str,
        function: &str,
    ) -> Vec<(Node<'t>, Node<'t>)> {
        self.descendants()
            .into_iter()
            .filter(|n| n.kind() == "assignment")
            .filter_map(|assignment| {
                let left = assignment.child_by_field_name("left")?;
                let right = assignment.child_by_field_name("right")?;
                let callee = right.child_by_field_name("function")?;
                (left.kind() == "identifier"
                    && self.node_text(left) == variable
                    && right.kind() == "call"
                    && self.node_text(callee) == function)
                    .then_some((assignment, right))
            })
            .collect()
    }

    /// Whether a call has a keyword argument with this name.
    pub fn has_keyword_argument(&self, call: Node<'_>, keyword: &str) -> bool {
        let Some(arguments) = call.child_by_field_name("arguments") else {
            return false;
        };
        let mut cursor = arguments.walk();
        arguments.named_children(&mut cursor).any(|arg| {
            arg.kind() == "keyword_argument"
                && arg
                    .child_by_field_name("name")
                    .is_some_and(|name| self.node_text(name) == keyword)
        })
    }

    /// Whitespace between `(` and the first argument (`"\n\t\t"`), used to insert a new
    /// first argument with matching layout.
    pub fn argument_indent(&self, call: Node<'_>) -> Option<(usize, String)> {
        let arguments = call.child_by_field_name("arguments")?;
        let open = arguments.start_byte() + 1;
        let mut cursor = arguments.walk();
        let first = arguments.named_children(&mut cursor).next();
        let indent = first.map_or_else(
            || " ".to_owned(),
            |f| self.text[open..f.start_byte()].to_owned(),
        );
        Some((open, indent))
    }

    /// Leading whitespace of the line a node starts on.
    pub fn line_indent(&self, node: Node<'_>) -> &str {
        let start = node.start_byte();
        let line_start = self.text[..start].rfind('\n').map_or(0, |i| i + 1);
        &self.text[line_start..start]
    }

    /// Whether the tree contains syntax errors; returns the first error position.
    pub fn first_error(&self) -> Option<(usize, usize)> {
        if !self.root().has_error() {
            return None;
        }
        self.descendants()
            .into_iter()
            .find(|n| n.is_error() || n.is_missing())
            .map(|n| (n.start_position().row + 1, n.start_position().column + 1))
            .or(Some((0, 0)))
    }
}

impl fmt::Debug for DagDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DagDocument")
            .field("name", &self.name)
            .field("bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

fn parse_python(text: &str) -> Option<Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    parser.parse(text, None)
}

/// Unquoted content of a Python string literal node (`"a"`, `'a'`).
pub(crate) fn string_content<'a>(doc: &'a DagDocument, node: Node<'_>) -> Option<&'a str> {
    (node.kind() == "string").then(|| {
        doc.node_text(node)
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .trim_matches(|c| c == '"' || c == '\'')
    })
}
