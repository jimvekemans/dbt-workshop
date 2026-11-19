use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A release fix version: `<year>_DROP<n>` or `<year>_EDROP<n>`. `2026_DROP5` and `2026_DROP05` are equal.
#[derive(Debug, Clone)]
pub struct FixVersion {
    pub year: u16,
    pub emergency: bool,
    pub number: u8,
    name: String,
}

impl FixVersion {
    /// The name as Jira spells it, for example `2026_DROP05`.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for FixVersion {
    type Err = String;

    /// The names the Python code accepted: `^\d{4}_E?DROP\d{1,2}$`.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("`{name}` is not a DROP fix version (like 2026_DROP05)");
        let (year, rest) = name.split_once('_').ok_or_else(invalid)?;
        if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        let (emergency, number) = match rest.strip_prefix("EDROP") {
            Some(number) => (true, number),
            None => (false, rest.strip_prefix("DROP").ok_or_else(invalid)?),
        };
        if number.is_empty() || number.len() > 2 || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        Ok(Self {
            year: year.parse().map_err(|_| invalid())?,
            emergency,
            number: number.parse().map_err(|_| invalid())?,
            name: name.to_owned(),
        })
    }
}

impl Ord for FixVersion {
    /// Year, then DROP before EDROP (as the Python string comparison had it), then the number numerically.
    fn cmp(&self, other: &Self) -> Ordering {
        (self.year, self.emergency, self.number).cmp(&(other.year, other.emergency, other.number))
    }
}

impl PartialEq for FixVersion {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for FixVersion {}

impl PartialOrd for FixVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for FixVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(name: &str) -> FixVersion {
        name.parse().unwrap()
    }

    #[test]
    fn parses_what_python_accepted() {
        assert_eq!(v("2026_DROP05").number, 5);
        assert!(v("2026_EDROP1").emergency);
        for bad in [
            "Nog in te plannen",
            "2026_DROP",
            "2026_DROP123",
            "26_DROP1",
            "2026-DROP1",
            "2026_drop1",
        ] {
            assert!(bad.parse::<FixVersion>().is_err(), "{bad}");
        }
    }

    #[test]
    fn orders_numerically_but_otherwise_like_python() {
        // Python: "2026_DROP5" > "2026_DROP10" as strings.
        assert!(v("2026_DROP10") > v("2026_DROP5"));
        assert_eq!(v("2026_DROP05"), v("2026_DROP5"));
        // Unchanged from Python: year first, and an EDROP after every DROP of that year.
        assert!(v("2027_DROP01") > v("2026_EDROP09"));
        assert!(v("2026_EDROP01") > v("2026_DROP12"));
    }
}
