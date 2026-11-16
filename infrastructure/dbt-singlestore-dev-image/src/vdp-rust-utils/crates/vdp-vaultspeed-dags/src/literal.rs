use std::fmt;

/// A Python literal to embed into generated code.
#[derive(Debug, Clone, PartialEq)]
pub enum PyLiteral {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<PyLiteral>),
}

impl PyLiteral {
    /// Converts configuration values (JSON/TOML data) to Python literals.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        Some(match value {
            serde_json::Value::Null => Self::None,
            serde_json::Value::Bool(b) => Self::Bool(*b),
            serde_json::Value::Number(n) => Self::Int(n.as_i64()?),
            serde_json::Value::String(s) => Self::Str(s.clone()),
            serde_json::Value::Array(items) => {
                Self::List(items.iter().map(Self::from_json).collect::<Option<_>>()?)
            }
            serde_json::Value::Object(_) => return None,
        })
    }
}

impl fmt::Display for PyLiteral {
    /// Renders like Python's `json.dumps` for strings and lists, which is what the
    /// Python framework emitted (`["a", "b"]`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Bool(true) => f.write_str("True"),
            Self::Bool(false) => f.write_str("False"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Str(s) => f.write_str(&serde_json::Value::String(s.clone()).to_string()),
            Self::List(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_python_syntax() {
        let value = serde_json::json!(["a@b", "c\"d"]);
        assert_eq!(
            PyLiteral::from_json(&value).unwrap().to_string(),
            r#"["a@b", "c\"d"]"#
        );
        assert_eq!(PyLiteral::Bool(false).to_string(), "False");
        assert_eq!(
            PyLiteral::Str("S2_dv_pool".into()).to_string(),
            "\"S2_dv_pool\""
        );
    }
}
