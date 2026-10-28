// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `check-trust`: no AI co-author trailer on commits touching T0, and the lint policy of the workspace.

use crate::git;
use crate::levels::{Level, Levels};
use std::path::Path;

/// Substrings that mark a name, e-mail or trailer value as an AI or a bot (matched case-insensitively).
/// The only place this list lives. Over-matching is the safe direction: a false positive only matters
/// on a commit that touches T0, where a human can look at it.
pub const AI_MARKERS: &[&str] = &[
    "claude",
    "anthropic",
    "gpt",
    "openai",
    "copilot",
    "gemini",
    "codex",
    "cursor",
    "devin",
    "aider",
    "cline",
    "deepseek",
    "qwen",
    "grok",
    "mistral",
    "llama",
    "amazon q",
    "[bot]",
];

/// Trailer keys whose value may name an AI (compared case-insensitively).
const TRAILER_KEYS: &[&str] = &["co-authored-by", "assisted-by", "generated-by"];

fn names_ai(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    AI_MARKERS.iter().any(|m| lower.contains(m))
}

/// Join continuation lines (starting with space or tab) onto the previous line.
fn unfold(message: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in message.lines() {
        match lines.last_mut() {
            Some(prev) if line.starts_with([' ', '\t']) => {
                prev.push(' ');
                prev.push_str(line.trim());
            }
            _ => lines.push(line.to_owned()),
        }
    }
    lines
}

/// Trailer values of a commit message (Co-Authored-By, Assisted-by, Generated-by) that name an AI.
pub fn ai_coauthors(message: &str) -> Vec<String> {
    unfold(message)
        .iter()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim();
            if !TRAILER_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k)) {
                return None;
            }
            names_ai(value).then(|| value.trim().to_owned())
        })
        .collect()
}

/// Author or committer identities that name an AI.
pub fn ai_identities(identities: &[String]) -> Vec<String> {
    identities.iter().filter(|i| names_ai(i)).cloned().collect()
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
        let mut authors = ai_coauthors(&git::message(root, hash)?);
        authors.extend(ai_identities(&git::identities(root, hash)?));
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
            "commit {hash} touches T0 and names an AI (trailer, author or committer) ({}):",
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
    fn detects_folded_and_other_trailers() {
        assert_eq!(
            ai_coauthors("x\n\nCo-Authored-By:\n Claude <noreply@anthropic.com>\n").len(),
            1
        );
        assert_eq!(
            ai_coauthors("x\n\nCo-Authored-By:\n\tClaude <c@d.e>").len(),
            1
        );
        assert_eq!(
            ai_coauthors("x\n\nCo-Authored-By: Jane\n  Doe <j@d.e>\n").len(),
            0
        );
        assert_eq!(ai_coauthors("x\n\nAssisted-by: Cursor").len(), 1);
        assert_eq!(ai_coauthors("x\n\nGENERATED-BY: Amazon Q").len(), 1);
        assert_eq!(
            ai_coauthors("x\n\nCo-authored-by: dependabot[bot] <a@b.c>").len(),
            1
        );
        assert!(ai_coauthors("x\n\nReviewed-by: Claude <a@b.c>").is_empty());
    }

    #[test]
    fn detects_ai_identities() {
        let ids = vec![
            "Jane <jane@example.com>".to_owned(),
            "Claude <noreply@anthropic.com>".to_owned(),
        ];
        assert_eq!(ai_identities(&ids).len(), 1);
        assert!(ai_identities(&ids[..1]).is_empty());
    }

    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            st.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&st.stderr)
        );
    }

    #[test]
    fn merge_is_diffed_against_first_parent_only() {
        let dir = std::env::temp_dir().join(format!("xtask-merge-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("t0")).expect("mkdir");
        run_git(&dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a"), "1").expect("write");
        run_git(&dir, &["add", "a"]);
        run_git(&dir, &["commit", "-q", "-m", "base"]);
        run_git(&dir, &["checkout", "-q", "-b", "side"]);
        std::fs::write(dir.join("t0/x"), "1").expect("write");
        run_git(&dir, &["add", "t0/x"]);
        run_git(&dir, &["commit", "-q", "-m", "side touches t0"]);
        run_git(&dir, &["checkout", "-q", "main"]);
        std::fs::write(dir.join("a"), "2").expect("write");
        run_git(&dir, &["commit", "-q", "-am", "main change"]);
        run_git(&dir, &["merge", "-q", "--no-ff", "-m", "merge", "side"]);

        // The merge itself, against parent 1 (main), brings t0/x in: it IS listed.
        let paths = git::changed_paths(&dir, "HEAD").expect("paths");
        assert_eq!(paths, vec!["t0/x"]);
        // Against parent 1 a change that only main made is not listed.
        assert!(!paths.contains(&"a".to_owned()));
        // The side commit is listed on its own.
        let side = git::changed_paths(&dir, "HEAD^2").expect("paths");
        assert_eq!(side, vec!["t0/x"]);
        let _ = std::fs::remove_dir_all(&dir);
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
