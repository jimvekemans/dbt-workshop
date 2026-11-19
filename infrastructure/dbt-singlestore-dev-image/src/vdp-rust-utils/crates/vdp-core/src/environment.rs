use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::CoreError;

/// A deployment environment: the built-in ones (`LDV`, `ABT`, `CBT`, `PRD`, always known) or any other name a project
/// configures under `[environments.<name>]`. Which names are valid is the configuration's business
/// ([`crate::Config::environment_named`]); this type only carries the name.
///
/// The short names `dev`, `val` and `prd` are aliases of the built-ins `LDV`, `ABT` and `PRD` (see
/// [`Environment::ALIASES`]): [`Environment::named`] resolves them, so `--env dev`, `$DBT_DEPLOY_TARGET=dev`, an
/// `[environments.dev]` section and `default_environment = "dev"` all mean `LDV`. [`Environment::short_name`] is the
/// way back.
///
/// The name is interned, so the type stays `Copy` like the enum it replaced.
#[derive(Clone, Copy)]
pub struct Environment(&'static str);

/// Names interned so far (the built-ins are `'static` already).
static INTERNED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

impl Environment {
    /// The LDV environment.
    pub const LDV: Self = Self("LDV");
    /// The ABT environment.
    pub const ABT: Self = Self("ABT");
    /// The CBT environment.
    pub const CBT: Self = Self("CBT");
    /// The PRD environment.
    pub const PRD: Self = Self("PRD");

    /// The built-in environments, lowest first. The built-in configuration knows these four.
    pub const ALL: [Self; 4] = [Self::LDV, Self::ABT, Self::CBT, Self::PRD];

    /// Short names of built-in environments, as the dbt workshop calls them: `dev` is `LDV`, `val` is `ABT` and `prd`
    /// is `PRD` (`CBT` has none). Matched case-insensitively, like the built-in names themselves.
    pub const ALIASES: [(&'static str, Self); 3] =
        [("dev", Self::LDV), ("val", Self::ABT), ("prd", Self::PRD)];

    /// `LDV`, `ABT`, `CBT`, `PRD`, or a configured name as written in the configuration.
    pub const fn as_str(self) -> &'static str {
        self.0
    }

    /// `ldv`, ... as used in Vault paths (`vdp/ldv/dbt`).
    pub fn lower(self) -> String {
        self.0.to_ascii_lowercase()
    }

    /// The short name of the environment: `dev` for `LDV`, `val` for `ABT`, `prd` for `PRD` (the inverse of
    /// [`Environment::ALIASES`]), and the lower-case name for every other one (`cbt`, `qa-1`). The `{env_short}`
    /// placeholder of the configuration renders it, where `{env}` renders [`Environment::lower`] (`ldv`).
    pub fn short_name(self) -> String {
        Self::ALIASES
            .iter()
            .find(|(_, env)| *env == self)
            .map_or_else(|| self.lower(), |(alias, _)| (*alias).to_owned())
    }

    /// The built-in environment `name` stands for: a built-in name or one of the [`Environment::ALIASES`], matched
    /// case-insensitively after trimming. `None` for any other name.
    pub fn built_in(name: &str) -> Option<Self> {
        let name = name.trim();
        Self::ALL
            .into_iter()
            .find(|e| e.0.eq_ignore_ascii_case(name))
            .or_else(|| {
                Self::ALIASES
                    .iter()
                    .find(|(alias, _)| alias.eq_ignore_ascii_case(name))
                    .map(|(_, env)| *env)
            })
    }

    /// One of the four built-in environments.
    pub fn is_built_in(self) -> bool {
        Self::ALL.contains(&self)
    }

    /// A name as given: a built-in or one of its [`Environment::ALIASES`] matched case-insensitively (`prd` is `PRD`,
    /// `dev` is `LDV`, `val` is `ABT`), anything else kept as written. Names are letters, digits, `_` and `-`.
    ///
    /// # Errors
    ///
    /// [`CoreError::UnknownEnvironment`] when `value` is empty or has a character other than letters, digits, `_` and `-`.
    pub fn named(value: &str) -> Result<Self, CoreError> {
        let value = value.trim();
        if let Some(env) = Self::built_in(value) {
            return Ok(env);
        }
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(CoreError::UnknownEnvironment {
                name: value.to_owned(),
                known: Self::ALL.iter().map(|e| e.0.to_owned()).collect(),
            });
        }
        let mut interned = INTERNED.lock().unwrap_or_else(PoisonError::into_inner);
        let name = if let Some(name) = interned.get(value) {
            *name
        } else {
            let name: &'static str = Box::leak(value.to_owned().into_boxed_str());
            interned.insert(name);
            name
        };
        Ok(Self(name))
    }

    /// Position for ordering: the built-in environments in their order, then the others by name.
    fn rank(self) -> usize {
        Self::ALL
            .iter()
            .position(|e| e.0 == self.0)
            .unwrap_or(Self::ALL.len())
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::LDV
    }
}

impl PartialEq for Environment {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Environment {}

impl Hash for Environment {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Ord for Environment {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank()
            .cmp(&other.rank())
            .then_with(|| self.0.cmp(other.0))
    }
}

impl PartialOrd for Environment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Debug for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Environment({})", self.0)
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl FromStr for Environment {
    type Err = CoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::named(value)
    }
}

impl Serialize for Environment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for Environment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn built_ins_parse_case_insensitively() {
        assert_eq!("abt".parse::<Environment>().unwrap(), Environment::ABT);
        assert_eq!(" PRD ".parse::<Environment>().unwrap(), Environment::PRD);
        assert_eq!(Environment::CBT.lower(), "cbt");
        assert!(Environment::CBT.is_built_in());
    }

    #[test]
    fn dev_val_prd_are_aliases_of_the_built_ins() {
        for dev in ["dev", "DEV", " Dev "] {
            assert_eq!(Environment::named(dev).unwrap(), Environment::LDV, "{dev}");
        }
        for val in ["val", "VAL", " Val "] {
            assert_eq!(Environment::named(val).unwrap(), Environment::ABT, "{val}");
        }
        for prd in ["prd", "Prd", " PRD "] {
            assert_eq!(Environment::named(prd).unwrap(), Environment::PRD, "{prd}");
        }
        // Resolved, not kept as written: the alias leaves no trace in the name.
        assert_eq!(Environment::named("dev").unwrap().as_str(), "LDV");
        assert_eq!("val".parse::<Environment>().unwrap(), Environment::ABT);
        // CBT has no alias, and other names stay custom.
        assert_eq!(Environment::named("cbt").unwrap(), Environment::CBT);
        let custom = Environment::named("qa-1").unwrap();
        assert_eq!(custom.as_str(), "qa-1");
        assert!(!custom.is_built_in());
        assert_eq!(
            Environment::named("development").unwrap().as_str(),
            "development"
        );
        assert_eq!(Environment::built_in("xyz"), None);
        assert_eq!(Environment::built_in(" DEV "), Some(Environment::LDV));
        // Deserialisation goes through `named`.
        let holder: BTreeMap<String, Environment> = toml::from_str("env = \"dev\"").unwrap();
        assert_eq!(holder["env"], Environment::LDV);
    }

    #[test]
    fn short_names_are_the_inverse_of_the_aliases() {
        assert_eq!(Environment::LDV.short_name(), "dev");
        assert_eq!(Environment::ABT.short_name(), "val");
        assert_eq!(Environment::PRD.short_name(), "prd");
        assert_eq!(Environment::CBT.short_name(), "cbt");
        assert_eq!(Environment::named("Qa-1").unwrap().short_name(), "qa-1");
        for (alias, env) in Environment::ALIASES {
            assert_eq!(Environment::named(&env.short_name()).unwrap(), env);
            assert_eq!(env.short_name(), alias);
        }
    }

    #[test]
    fn other_names_are_kept_as_written_and_interned() {
        let dev = Environment::named("dev_unit").unwrap();
        assert_eq!(dev.as_str(), "dev_unit");
        assert!(!dev.is_built_in());
        assert_eq!(dev, Environment::named("dev_unit").unwrap());
        assert!(std::ptr::eq(
            dev.as_str(),
            Environment::named("dev_unit").unwrap().as_str()
        ));
        for bad in ["", "release/1", "a b", "x.y"] {
            assert!(Environment::named(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_built_in_order_comes_first() {
        let mut envs = [
            Environment::named("val_unit").unwrap(),
            Environment::PRD,
            Environment::named("dev_unit2").unwrap(),
            Environment::LDV,
            Environment::CBT,
            Environment::ABT,
        ];
        envs.sort();
        let names: Vec<&str> = envs.iter().map(|e| e.as_str()).collect();
        assert_eq!(names, ["LDV", "ABT", "CBT", "PRD", "dev_unit2", "val_unit"]);
    }
}
