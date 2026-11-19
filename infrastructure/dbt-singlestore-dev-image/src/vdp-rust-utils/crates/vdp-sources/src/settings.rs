use serde::{Deserialize, Serialize};

/// `[sources]`: source file maintenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SourcesSettings {
    /// Where generated `sources_<database>.yml` files live, relative to the dbt project.
    pub generated_dir: String,
    /// Databases `vdp sources rewrite` regenerates (the list `update_source_files.py` had).
    pub databases: Vec<String>,
    /// Write column names into source tables.
    pub include_columns: bool,
    /// `name` (alphabetical, as the Python utilities did) or `ordinal` (table order).
    pub column_order: String,
    /// For projects whose databases carry the environment in their name (`dev_landing`, `val_landing`): how the
    /// databases vdp reads map to what source files say. Unset (the built-in default): a source's database is the
    /// physical name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_prefix: Option<DatabasePrefix>,
}

/// `[sources.database_prefix]`: a database `<physical><rest>` is written as the source `<rest>` with database and
/// schema `<jinja><rest>`, and a source whose database is `<jinja><rest>` is read from `<physical><rest>`.
///
/// ```toml
/// [sources.database_prefix]
/// physical = "dev_"
/// jinja = "{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_"
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabasePrefix {
    /// The prefix of the databases vdp connects to (the environment the developer works on).
    pub physical: String,
    /// What a source file says instead (whitespace and quote style do not matter when reading).
    pub jinja: String,
}

/// Where a source lives: its name in the files and its `database`/`schema` value there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTarget {
    pub name: String,
    pub database: String,
}

impl Default for SourcesSettings {
    fn default() -> Self {
        Self {
            generated_dir: "models/sources".into(),
            databases: [
                "edw_dv_bdv",
                "edw_dv_rdv",
                "edw_fmc",
                "edw_inc",
                "information_schema",
                "tenant_ai",
                "tenant_leertijd",
                "vdp_mgmt",
                "vdp_psa",
            ]
            .map(String::from)
            .to_vec(),
            include_columns: true,
            column_order: "name".into(),
            database_prefix: None,
        }
    }
}

/// Without whitespace, `"` as `'`: `{{ env_var("X") }}` and `{{env_var('X')}}` are the same expression.
fn normalized(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '"' { '\'' } else { c })
        .collect()
}

impl SourcesSettings {
    /// The name of the config section these settings are read from (`[sources]`).
    pub const SECTION: &'static str = "sources";

    /// The source a physical database is written as: `dev_landing` is `landing` with database
    /// `{{ … }}_landing` under [`DatabasePrefix`]; otherwise name and database are the physical name.
    pub fn target_for(&self, physical: &str) -> SourceTarget {
        if let Some(prefix) = &self.database_prefix
            && let Some(rest) = physical.strip_prefix(prefix.physical.as_str())
            && !rest.is_empty()
            && !prefix.physical.is_empty()
        {
            return SourceTarget {
                name: rest.to_owned(),
                database: format!("{}{rest}", prefix.jinja),
            };
        }
        SourceTarget {
            name: physical.to_owned(),
            database: physical.to_owned(),
        }
    }

    /// The physical database behind a source's `database` (or `schema`) value: the value itself, or for Jinja the
    /// [`DatabasePrefix`] mapping; `None` when it is Jinja the mapping does not cover (only dbt can tell).
    pub fn physical_of(&self, value: &str) -> Option<String> {
        if !value.contains("{{") && !value.contains("{%") {
            return Some(value.to_owned());
        }
        let prefix = self.database_prefix.as_ref()?;
        let value = normalized(value);
        let jinja = normalized(&prefix.jinja);
        let rest = value.strip_prefix(jinja.as_str())?;
        (!rest.is_empty() && !rest.contains('{')).then(|| format!("{}{rest}", prefix.physical))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workshop() -> SourcesSettings {
        SourcesSettings {
            database_prefix: Some(DatabasePrefix {
                physical: "dev_".into(),
                jinja: "{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_".into(),
            }),
            ..SourcesSettings::default()
        }
    }

    #[test]
    fn the_prefix_maps_both_ways() {
        let settings = workshop();
        assert_eq!(
            settings.target_for("dev_landing"),
            SourceTarget {
                name: "landing".into(),
                database: "{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_landing".into()
            }
        );
        assert_eq!(
            settings.target_for("information_schema").name,
            "information_schema"
        );
        assert_eq!(
            settings
                .physical_of("{{env_var(\"DBT_DEPLOY_TARGET\", \"dev\")}}_landing")
                .as_deref(),
            Some("dev_landing")
        );
        assert_eq!(
            settings.physical_of("vdp_mgmt").as_deref(),
            Some("vdp_mgmt")
        );
        assert_eq!(settings.physical_of("{{ var('DB') }}"), None);
        // Without the option, Jinja stays dbt's business and names are physical (the default behaviour).
        let defaults = SourcesSettings::default();
        assert_eq!(
            defaults.physical_of("{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_landing"),
            None
        );
        assert_eq!(defaults.target_for("dev_landing").database, "dev_landing");
    }
}
