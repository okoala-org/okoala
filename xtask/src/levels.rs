// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Trust levels from `TRUST_LEVELS.toml`: path prefixes, `*` matches one component, the longest
//! matching entry wins, unlisted files are T1.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    T0,
    T1,
    T2,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::T0 => "T0",
            Level::T1 => "T1",
            Level::T2 => "T2",
        })
    }
}

pub struct Levels {
    /// (entry components, level)
    entries: Vec<(Vec<String>, Level)>,
}

fn components(path: &str) -> Vec<&str> {
    path.split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect()
}

impl Levels {
    pub fn parse(text: &str) -> Result<Levels, String> {
        let table: toml::Table = text
            .parse()
            .map_err(|e| format!("TRUST_LEVELS.toml: {e}"))?;
        let mut entries = Vec::new();
        for (key, level) in [("t0", Level::T0), ("t1", Level::T1), ("t2", Level::T2)] {
            let Some(value) = table.get(key) else {
                continue;
            };
            let array = value
                .as_array()
                .ok_or_else(|| format!("{key} must be an array"))?;
            for item in array {
                let entry = item
                    .as_str()
                    .ok_or_else(|| format!("{key} must hold strings"))?;
                let parts = components(entry);
                if parts.is_empty() {
                    return Err(format!("{key} holds an empty path"));
                }
                entries.push((parts.iter().map(|p| (*p).to_owned()).collect(), level));
            }
        }
        Ok(Levels { entries })
    }

    /// Level of `path` (relative to the repository root, `/` separated).
    pub fn level_of(&self, path: &str) -> Level {
        let path = components(path);
        let mut best: Option<(usize, Level)> = None;
        for (entry, level) in &self.entries {
            if entry.len() > path.len() {
                continue;
            }
            if !entry.iter().zip(&path).all(|(e, p)| e == "*" || e == p) {
                continue;
            }
            // Longest wins; on a tie the stricter (lower) level wins.
            let better = match best {
                None => true,
                Some((len, l)) => {
                    entry.len() > len || (entry.len() == len && rank(*level) < rank(l))
                }
            };
            if better {
                best = Some((entry.len(), *level));
            }
        }
        best.map_or(Level::T1, |(_, l)| l)
    }
}

fn rank(level: Level) -> u8 {
    match level {
        Level::T0 => 0,
        Level::T1 => 1,
        Level::T2 => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
t0 = ["crates/secret/src/"]
t1 = ["crates/"]
t2 = ["crates/*/tests/", "xtask/", ".github/"]
"#;

    fn levels() -> Levels {
        Levels::parse(SAMPLE).expect("sample parses")
    }

    #[test]
    fn longest_prefix_wins() {
        let l = levels();
        assert_eq!(l.level_of("crates/secret/src/lib.rs"), Level::T0);
        assert_eq!(l.level_of("crates/secret/src/deep/x.rs"), Level::T0);
        assert_eq!(l.level_of("crates/secret/tests/a.rs"), Level::T2);
        assert_eq!(l.level_of("crates/core/src/lib.rs"), Level::T1);
        assert_eq!(l.level_of("crates/secret/Cargo.toml"), Level::T1);
    }

    #[test]
    fn star_matches_one_component_only() {
        let l = levels();
        assert_eq!(l.level_of("crates/db/tests/x.rs"), Level::T2);
        assert_eq!(l.level_of("crates/db/tests/nested/x.rs"), Level::T2);
        // "tests" must be the third component, not deeper.
        assert_eq!(l.level_of("crates/db/src/tests/x.rs"), Level::T1);
        assert_eq!(l.level_of("crates/tests/x.rs"), Level::T1);
    }

    #[test]
    fn unlisted_is_t1_and_prefix_is_by_component() {
        let l = levels();
        assert_eq!(l.level_of("README.md"), Level::T1);
        assert_eq!(l.level_of("xtask/src/main.rs"), Level::T2);
        assert_eq!(l.level_of("xtask-evil/src/main.rs"), Level::T1);
        assert_eq!(l.level_of("crates/secret/srcx/a.rs"), Level::T1);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(Levels::parse("t0 = 3").is_err());
        assert!(Levels::parse("t0 = [1]").is_err());
        assert!(Levels::parse("t0 = [\"\"]").is_err());
        assert!(Levels::parse("not toml [").is_err());
    }

    #[test]
    fn real_file_parses() {
        let l = Levels::parse(include_str!("../../TRUST_LEVELS.toml")).expect("parses");
        assert_eq!(l.level_of("crates/secret/src/lib.rs"), Level::T0);
    }
}
