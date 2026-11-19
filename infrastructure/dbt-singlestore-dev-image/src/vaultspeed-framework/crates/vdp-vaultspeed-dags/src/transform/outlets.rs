use super::{DagTransformer, SourceResolver};
use crate::{DagContext, DagDocument, DagError, TextEdit};

/// Gives every mapping task inside the `for map, info in mappings.items()` loop a
/// `SingleStore` dataset outlet, for the mapping kinds that land in the vault:
///
/// ```python
/// if map.startswith(("hub", "lnk", ...)):
///     dataset_outlet = Dataset(f"s2://host:port/{schema}/{map}")
/// else:
///     dataset_outlet = None
/// task = SingleStoreOperator(
///     outlets=dataset_outlet,
/// ```
#[derive(Debug, Clone)]
pub struct MappingTaskOutlets {
    task_variable: String,
    operator: String,
    loop_variable: String,
    prefixes: Vec<String>,
}

impl MappingTaskOutlets {
    /// Creates the transformer: mapping names that start with one of `prefixes` get a dataset outlet.
    pub fn new(prefixes: Vec<String>) -> Self {
        Self {
            task_variable: "task".into(),
            operator: "SingleStoreOperator".into(),
            loop_variable: "map".into(),
            prefixes,
        }
    }
}

impl DagTransformer for MappingTaskOutlets {
    fn id(&self) -> &'static str {
        "outlets:mapping-tasks"
    }

    fn edits(&self, doc: &DagDocument, ctx: &DagContext) -> Result<Vec<TextEdit>, DagError> {
        let prefixes = self
            .prefixes
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let uri = format!(
            "s2://{}:{}/{{info['src_objects'][0]['schema_name'].replace('`', '')}}/{{{}}}",
            ctx.singlestore_host, ctx.singlestore_port, self.loop_variable
        );
        let mut edits = Vec::new();
        for (assignment, call) in doc.call_assignments(&self.task_variable, &self.operator) {
            if doc.has_keyword_argument(call, "outlets") {
                continue;
            }
            let indent = doc.line_indent(assignment);
            let (open, argument_indent) = doc.argument_indent(call).expect("a call has arguments");
            edits.push(TextEdit::insert(
                assignment.start_byte(),
                format!(
                    "if {loop}.startswith(({prefixes})):\n{indent}\tdataset_outlet = Dataset(f\"{uri}\")\n\
                     {indent}else:\n{indent}\tdataset_outlet = None\n{indent}",
                    loop = self.loop_variable,
                ),
            ));
            edits.push(TextEdit::insert(
                open,
                format!("{argument_indent}outlets=dataset_outlet,"),
            ));
        }
        Ok(edits)
    }
}

/// Gives the `fmc_load_success` task a `vdp://<env>/<database>/<source>` outlet so
/// downstream DAGs can be scheduled on a completed source load.
#[derive(Debug, Clone)]
pub struct LoadSuccessOutlet {
    task_variable: String,
    operator: String,
    sources: SourceResolver,
}

impl LoadSuccessOutlet {
    /// Creates the transformer; `sources` determines the source system of the DAG.
    pub fn new(sources: SourceResolver) -> Self {
        Self {
            task_variable: "fmc_load_success".into(),
            operator: "SingleStoreOperator".into(),
            sources,
        }
    }
}

impl DagTransformer for LoadSuccessOutlet {
    fn id(&self) -> &'static str {
        "outlets:load-success"
    }

    fn edits(&self, doc: &DagDocument, ctx: &DagContext) -> Result<Vec<TextEdit>, DagError> {
        let targets: Vec<_> = doc
            .call_assignments(&self.task_variable, &self.operator)
            .into_iter()
            .filter(|(_, call)| !doc.has_keyword_argument(*call, "outlets"))
            .collect();
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let source = self
            .sources
            .resolve(doc.name())
            .ok_or_else(|| DagError::UnknownSource {
                dag: doc.name().to_owned(),
            })?;
        let uri = format!("vdp://{}/{}/{source}", ctx.dataset_env, ctx.database);
        Ok(targets
            .into_iter()
            .map(|(_, call)| {
                let (open, indent) = doc.argument_indent(call).expect("a call has arguments");
                TextEdit::insert(open, format!("{indent}outlets=Dataset(\"{uri}\"),"))
            })
            .collect())
    }
}
