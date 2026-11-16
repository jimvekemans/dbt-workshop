/// Levenshtein distance, case-insensitive.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current.push(
                (previous[j] + cost)
                    .min(previous[j + 1] + 1)
                    .min(current[j] + 1),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

/// Up to three candidates close to `name`, closest first.
pub(crate) fn closest(name: &str, candidates: &[String]) -> Vec<String> {
    let limit = (name.chars().count() / 3).max(2);
    let mut scored: Vec<(usize, &String)> = candidates
        .iter()
        .map(|c| (distance(name, c), c))
        .filter(|(d, c)| *d <= limit || c.to_lowercase().contains(&name.to_lowercase()))
        .collect();
    scored.sort();
    scored.into_iter().take(3).map(|(_, c)| c.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_typos_and_case() {
        let tables = vec![
            "customer".to_owned(),
            "orders".to_owned(),
            "lineitem".to_owned(),
        ];
        assert_eq!(closest("custmer", &tables), ["customer"]);
        assert_eq!(closest("ORDERS", &tables), ["orders"]);
        assert!(closest("zzz", &tables).is_empty());
    }
}
