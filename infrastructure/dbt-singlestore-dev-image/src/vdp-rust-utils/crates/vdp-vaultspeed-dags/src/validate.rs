use std::fmt::Debug;

use crate::{DagDocument, DagError};

/// Checks a rendered DAG before it may be published.
pub trait DagValidator: Send + Sync + Debug {
    fn validate(&self, doc: &DagDocument) -> Result<(), DagError>;
}

/// Rejects documents with syntax errors.
#[derive(Debug, Clone, Copy, Default)]
pub struct SyntaxValidator;

impl DagValidator for SyntaxValidator {
    fn validate(&self, doc: &DagDocument) -> Result<(), DagError> {
        match doc.first_error() {
            None => Ok(()),
            Some((line, column)) => Err(DagError::Invalid {
                dag: doc.name().to_owned(),
                detail: format!("syntax error near line {line}, column {column}"),
            }),
        }
    }
}
