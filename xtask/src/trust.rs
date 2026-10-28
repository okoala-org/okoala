// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `check-trust`: no AI co-author trailer on commits touching T0, and the lint policy of the workspace.

use crate::git;
use crate::levels::{Level, Levels};
use std::path::Path;

/// Names that mark a co-author as an AI (matched case-insensitively against the whole trailer value).
/// The only place this list lives.
pub const AI_MARKERS: &[&str] = &[
    "claude",
    "anthropic",
    "gpt",
    "openai",
    "copilot",
    "gemini",
    "codex",
];

/// Co-author trailer values of a commit message that name an AI.
pub fn ai_coauthors(message: &str) -> Vec<String> {
    message
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            if !key.trim().eq_ignore_ascii_case("co-authored-by") {
                return None;
            }
            let lower = value.to_ascii_lowercase();
            AI_MARKERS
                .iter()
                .any(|m| lower.contains(m))
                .then(|| value.trim().to_owned())
        })
        .collect()
}

/// Default range: the commits not on `origin/main`; without that ref, only HEAD.
fn default_range(root: &Path) -> String {
    if git::ref_exists(root, "origin/main") {
        "origin/main..HEAD".to_owned()
    } else {
        "HEAD".to_owned()
    }
}

fn check_commits(root: &Path, range: &str) -> Result<usize, String> {
    let levels = Levels::parse(
        &std::fs::read_to_string(root.join("TRUST_LEVELS.toml"))
            .map_err(|e| format!("TRUST_LEVELS.toml: {e}"))?,
    )?;
    let mut offenders = 0;
    let commits = git::commits(root, range)?;
    for hash in &commits {
        let authors = ai_coauthors(&git::message(root, hash)?);
        if authors.is_empty() {
            continue;
        }
        let t0: Vec<String> = git::changed_paths(root, hash)?
            .into_iter()
            .filter(|p| levels.level_of(p) == Level::T0)
            .collect();
        if t0.is_empty() {
            continue;
        }
        offenders += 1;
        eprintln!(
            "commit {hash} touches T0 and has an AI co-author trailer ({}):",
            authors.join("; ")
        );
        for p in t0 {
            eprintln!("    {p}");
        }
    }
    println!(
        "check-trust: {} commit(s) in {range}, {offenders} offending",
        commits.len()
    );
    Ok(offenders)
}

fn lint_problems(root_manifest: &str, members: &[(String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    match root_manifest.parse::<toml::Table>() {
        Err(e) => problems.push(format!("Cargo.toml: {e}")),
        Ok(t) => {
            let unsafe_code = t
                .get("workspace")
                .and_then(|w| w.get("lints"))
                .and_then(|l| l.get("rust"))
                .and_then(|r| r.get("unsafe_code"))
                .and_then(|v| v.as_str());
            if unsafe_code != Some("forbid") {
                problems.push(
                    "Cargo.toml: [workspace.lints.rust] must keep unsafe_code = \"forbid\""
                        .to_owned(),
                );
            }
        }
    }
    for (path, text) in members {
        match text.parse::<toml::Table>() {
            Err(e) => problems.push(format!("{path}: {e}")),
            Ok(t) => {
                let inherits = t
                    .get("lints")
                    .and_then(|l| l.get("workspace"))
                    .and_then(|v| v.as_bool())
                    == Some(true);
                if !inherits {
                    problems.push(format!("{path}: missing [lints] workspace = true"));
                }
            }
        }
    }
    problems
}

/// Manifest paths of the workspace members (supports `dir/*` globs and plain directories).
fn member_manifests(root: &Path, root_manifest: &str) -> Result<Vec<String>, String> {
    let t: toml::Table = root_manifest
        .parse()
        .map_err(|e| format!("Cargo.toml: {e}"))?;
    let members = t
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .ok_or("Cargo.toml: no [workspace] members")?;
    let mut out = Vec::new();
    for m in members {
        let m = m.as_str().ok_or("workspace members must be strings")?;
        if let Some(dir) = m.strip_suffix("/*") {
            let mut found: Vec<String> = std::fs::read_dir(root.join(dir))
                .map_err(|e| format!("{dir}: {e}"))?
                .filter_map(Result::ok)
                .map(|e| format!("{dir}/{}/Cargo.toml", e.file_name().to_string_lossy()))
                .filter(|p| root.join(p).is_file())
                .collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(format!("{m}/Cargo.toml"));
        }
    }
    Ok(out)
}

fn check_lints(root: &Path) -> Result<usize, String> {
    let root_manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let mut members = Vec::new();
    for path in member_manifests(root, &root_manifest)? {
        let text = std::fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        members.push((path, text));
    }
    let problems = lint_problems(&root_manifest, &members);
    for p in &problems {
        eprintln!("{p}");
    }
    println!(
        "check-trust: lint policy checked on {} member(s), {} problem(s)",
        members.len(),
        problems.len()
    );
    Ok(problems.len())
}

pub fn run(root: &Path, range: Option<&str>) -> Result<(), String> {
    let range = range.map_or_else(|| default_range(root), str::to_owned);
    let a = check_commits(root, &range)?;
    let b = check_lints(root)?;
    if a + b == 0 {
        Ok(())
    } else {
        Err("check-trust: failed".to_owned())
    }
}

pub fn levels(root: &Path, paths: &[String]) -> Result<(), String> {
    let levels = Levels::parse(
        &std::fs::read_to_string(root.join("TRUST_LEVELS.toml"))
            .map_err(|e| format!("TRUST_LEVELS.toml: {e}"))?,
    )?;
    for p in paths {
        println!("{}\t{p}", levels.level_of(p));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ai_trailers() {
        let msg =
            "[X]: thing\n\nbody\n\nCo-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>\n";
        assert_eq!(
            ai_coauthors(msg),
            vec!["Claude Sonnet 5.5 <noreply@anthropic.com>"]
        );
        assert_eq!(
            ai_coauthors("x\n\nco-authored-by: GitHub COPILOT <a@b.c>").len(),
            1
        );
        assert_eq!(
            ai_coauthors("x\n\nCo-authored-by: Bot <noreply@openai.com>").len(),
            1
        );
        assert_eq!(
            ai_coauthors("x\n\nCo-authored-by: Gemini <g@google.com>").len(),
            1
        );
        assert_eq!(ai_coauthors("x\n\nCo-authored-by: Codex <c@d.e>").len(), 1);
        assert_eq!(
            ai_coauthors("x\n\nCo-authored-by: ChatGPT <c@d.e>").len(),
            1
        );
    }

    #[test]
    fn ignores_humans_and_other_text() {
        assert!(ai_coauthors("x\n\nCo-Authored-By: Jane Doe <jane@example.com>").is_empty());
        assert!(ai_coauthors("Mentions Claude in the body, no trailer").is_empty());
        assert!(ai_coauthors("Signed-off-by: Claude <c@d.e>").is_empty());
    }

    const ROOT_OK: &str =
        "[workspace]\nmembers=[\"a\"]\n[workspace.lints.rust]\nunsafe_code = \"forbid\"\n";

    #[test]
    fn lint_policy() {
        let ok = (
            "a/Cargo.toml".to_owned(),
            "[package]\nname=\"a\"\n[lints]\nworkspace = true\n".to_owned(),
        );
        assert!(lint_problems(ROOT_OK, std::slice::from_ref(&ok)).is_empty());

        let missing = (
            "b/Cargo.toml".to_owned(),
            "[package]\nname=\"b\"\n".to_owned(),
        );
        assert_eq!(lint_problems(ROOT_OK, &[ok.clone(), missing]).len(), 1);

        let falsy = (
            "c/Cargo.toml".to_owned(),
            "[lints]\nworkspace = false\n".to_owned(),
        );
        assert_eq!(lint_problems(ROOT_OK, &[falsy]).len(), 1);

        let weak = "[workspace.lints.rust]\nunsafe_code = \"warn\"\n";
        assert_eq!(lint_problems(weak, std::slice::from_ref(&ok)).len(), 1);
        assert_eq!(lint_problems("[workspace]\n", &[ok]).len(), 1);
    }
}
