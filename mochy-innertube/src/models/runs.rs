//! `models/Runs.kt` — text runs and their segment helpers.

use crate::models::endpoint::NavigationEndpoint;
use serde::{Deserialize, Serialize};

/// `Runs.kt:6-8`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Runs {
    pub runs: Option<Vec<Run>>,
}

impl Runs {
    /// The concatenated text of all runs (a convenience the callers build
    /// inline upstream as `joinToString { it.text }`).
    pub fn text(&self) -> String {
        self.runs
            .as_ref()
            .map(|runs| {
                runs.iter()
                    .map(|r| r.text.as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    }

    pub fn first(&self) -> Option<&Run> {
        self.runs.as_ref()?.first()
    }

    pub fn last(&self) -> Option<&Run> {
        self.runs.as_ref()?.last()
    }
}

/// `Runs.kt:10-14`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub text: String,
    pub navigation_endpoint: Option<NavigationEndpoint>,
}

/// `List<Run>.splitBySeparator()` (`Runs.kt:16-29`): split on the literal
/// ` • ` separator runs.
pub fn split_by_separator(runs: &[Run]) -> Vec<Vec<Run>> {
    let mut result: Vec<Vec<Run>> = Vec::new();
    let mut current: Vec<Run> = Vec::new();
    for run in runs {
        if run.text == " • " {
            result.push(current);
            current = Vec::new();
        } else {
            current.push(run.clone());
        }
    }
    result.push(current);
    result
}

/// `List<List<Run>>.clean()` (`Runs.kt:31-38`): drop the first segment when
/// its first run has no navigation endpoint and its text carries no `&`
/// or `,` (the "Song • ..." prefix that is not an artist link).
pub fn clean(segments: &[Vec<Run>]) -> Vec<Vec<Run>> {
    let keep = match segments.first().and_then(|first| first.first()) {
        // getOrNull(0)?.getOrNull(0) == null -> `null != false` keeps
        None => true,
        // `navigationEndpoint != null || text?.contains("[&,]") != false`
        Some(first) => first.navigation_endpoint.is_some() || first.text.contains(['&', ',']),
    };
    if keep {
        segments.to_vec()
    } else {
        segments.iter().skip(1).cloned().collect()
    }
}

/// `List<Run>.oddElements()` (`Runs.kt:40-43`): every other run — the
/// artist names between the ` • ` separators.
pub fn odd_elements(runs: &[Run]) -> Vec<Run> {
    runs.iter()
        .enumerate()
        .filter(|(index, _)| index % 2 == 0)
        .map(|(_, run)| run.clone())
        .collect()
}

/// `String.looksLikeTimestamp()` (`Runs.kt:45-47`):
/// `^\d{1,2}[:.]\d{2}([:.]\d{2})?$` (Kotlin `\d` = Unicode Nd; the regex
/// crate's `\d` is Unicode Nd too).
pub fn looks_like_timestamp(text: &str) -> bool {
    use regex::Regex;
    use std::sync::OnceLock;
    static TIMESTAMP: OnceLock<Regex> = OnceLock::new();
    TIMESTAMP
        .get_or_init(|| Regex::new(r"^\d{1,2}[:.]\d{2}([:.]\d{2})?$").unwrap())
        .is_match(text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str) -> Run {
        Run {
            text: text.into(),
            navigation_endpoint: None,
        }
    }

    /// Runs.kt:16-29 — the separator run itself drops out.
    #[test]
    fn split_on_the_separator() {
        let runs = vec![run("A"), run(" • "), run("B"), run(" • "), run("C")];
        assert_eq!(
            split_by_separator(&runs),
            vec![vec![run("A")], vec![run("B")], vec![run("C")]]
        );
        // no separator: one segment
        assert_eq!(split_by_separator(&[run("A")]), vec![vec![run("A")]]);
    }

    /// Runs.kt:31-38 — a plain-text first segment is dropped, a linked or
    /// "&,"-bearing one is kept.
    #[test]
    fn clean_drops_the_unlinked_prefix() {
        let plain = vec![vec![run("Songs")], vec![run("Artist")]];
        assert_eq!(clean(&plain), vec![vec![run("Artist")]]);

        let with_comma = vec![vec![run("A & B")], vec![run("Artist")]];
        assert_eq!(clean(&with_comma), with_comma.clone());

        let mut linked = run("Artist");
        linked.navigation_endpoint = Some(NavigationEndpoint::default());
        let segments = vec![vec![linked.clone()], vec![run("Album")]];
        assert_eq!(clean(&segments), segments);
    }

    /// Runs.kt:40-43.
    #[test]
    fn odd_elements_takes_every_other() {
        let runs = vec![run("a"), run(" • "), run("b"), run(" • "), run("c")];
        assert_eq!(odd_elements(&runs), vec![run("a"), run("b"), run("c")]);
    }

    /// Runs.kt:45-47.
    #[test]
    fn timestamp_shape() {
        assert!(looks_like_timestamp("3:45"));
        assert!(looks_like_timestamp("1:02:03"));
        assert!(looks_like_timestamp("12.30"));
        assert!(!looks_like_timestamp("Song"));
        assert!(!looks_like_timestamp("123:45"));
        // trim is applied first
        assert!(looks_like_timestamp(" 3:45 "));
    }
}
