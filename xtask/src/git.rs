// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Thin wrappers over the `git` command line.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn toplevel() -> Result<PathBuf, String> {
    let out = git(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(out.trim()))
}

pub fn ls_files(root: &Path) -> Result<Vec<String>, String> {
    Ok(git(root, &["ls-files", "-z"])?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

pub fn ref_exists(root: &Path, rev: &str) -> bool {
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ],
    )
    .is_ok()
}

/// Commits of `range`. A range without `..` names a single commit.
pub fn commits(root: &Path, range: &str) -> Result<Vec<String>, String> {
    let out = if range.contains("..") {
        git(root, &["rev-list", range])?
    } else {
        git(root, &["rev-list", "-1", range])?
    };
    Ok(out.lines().map(str::to_owned).collect())
}

pub fn message(root: &Path, hash: &str) -> Result<String, String> {
    git(root, &["show", "-s", "--format=%B", hash])
}

/// Paths a commit touches (for merges, against every parent).
pub fn changed_paths(root: &Path, hash: &str) -> Result<Vec<String>, String> {
    let out = git(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "-m",
            "--root",
            "-z",
            hash,
        ],
    )?;
    Ok(out
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}
