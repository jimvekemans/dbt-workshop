use super::DagTransformer;
use crate::{DagContext, DagDocument, DagError, TextEdit};

/// Ensures `from <module> import <name>` exists, inserted right after the
/// `from <anchor> import ...` statement (or after the last import).
#[derive(Debug, Clone)]
pub struct EnsureImport {
    id: String,
    module: String,
    name: String,
    anchor_module: String,
}

impl EnsureImport {
    /// Ensures `from module import name`, placed after `from anchor_module import ...` when there is one.
    pub fn new(module: &str, name: &str, anchor_module: &str) -> Self {
        Self {
            id: format!("import:{module}.{name}"),
            module: module.to_owned(),
            name: name.to_owned(),
            anchor_module: anchor_module.to_owned(),
        }
    }
}

impl DagTransformer for EnsureImport {
    fn id(&self) -> &str {
        &self.id
    }

    fn edits(&self, doc: &DagDocument, _: &DagContext) -> Result<Vec<TextEdit>, DagError> {
        let imports: Vec<_> = doc
            .descendants()
            .into_iter()
            .filter(|n| n.kind() == "import_from_statement" || n.kind() == "import_statement")
            .collect();
        let module_of = |node: tree_sitter::Node<'_>| {
            node.child_by_field_name("module_name")
                .map(|m| doc.node_text(m).to_owned())
        };
        let already = imports.iter().any(|import| {
            module_of(*import).as_deref() == Some(self.module.as_str()) && {
                let mut cursor = import.walk();
                import
                    .children_by_field_name("name", &mut cursor)
                    .any(|name| {
                        doc.node_text(name).split_whitespace().next() == Some(self.name.as_str())
                    })
            }
        });
        if already {
            return Ok(Vec::new());
        }
        let line = format!("from {} import {}", self.module, self.name);
        let anchor = imports
            .iter()
            .find(|i| module_of(**i).as_deref() == Some(self.anchor_module.as_str()))
            .or_else(|| imports.last());
        Ok(vec![match anchor {
            Some(node) => TextEdit::insert(node.end_byte(), format!("\n{line}")),
            None => TextEdit::insert(0, format!("{line}\n")),
        }])
    }
}
