// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `okoala serve` and `okoala serve --check`, run as a process.

// Helpers outside `#[test]` functions unwrap too; a failure there is a test failure.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TEST_SECRET: &str = "internal-Secret-for-the-serve-Test";

fn temp_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("okoala-serve-tests")
        .join(format!("{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_secret(dir: &Path, mode: u32) -> PathBuf {
    let path = dir.join("internal-token");
    std::fs::write(&path, format!("{TEST_SECRET}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    path
}

fn write_config(dir: &Path, text: &str) -> PathBuf {
    let path = dir.join("okoala.toml");
    std::fs::write(&path, text).unwrap();
    path
}

fn okoala(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_okoala"))
        .args(args)
        .env_clear()
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn check_accepts_a_valid_config() {
    let dir = temp_dir("valid");
    let secret = write_secret(&dir, 0o400);
    let config = write_config(
        &dir,
        &format!(
            "[internal]\nlisten = \"127.0.0.1:9090\"\ntoken = {{ secret_file = '{}' }}\n",
            secret.display()
        ),
    );
    let out = okoala(&["serve", "--check", "--config", config.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("is valid (sections: internal)"));
    assert!(!text(&out.stdout).contains(TEST_SECRET));
}

#[test]
fn check_reports_every_error_and_fails() {
    let dir = temp_dir("invalid");
    let secret = write_secret(&dir, 0o644);
    let config = write_config(
        &dir,
        &format!(
            "[internal]\ntoken = {{ secret_file = '{}' }}\n\n[imap]\n\n[http]\n",
            secret.display()
        ),
    );
    let out = okoala(&["serve", "--check", "--config", config.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = text(&out.stderr);
    assert!(stderr.contains("3 configuration errors"), "{stderr}");
    assert!(
        stderr.contains("[internal] token:") && stderr.contains("has mode 0644"),
        "{stderr}"
    );
    assert!(stderr.contains("[imap]: unknown section"), "{stderr}");
    assert!(stderr.contains("[http]: unknown section"), "{stderr}");
    assert!(!stderr.contains(TEST_SECRET) && !text(&out.stdout).contains(TEST_SECRET));
}

#[test]
fn check_fails_without_the_internal_token() {
    let dir = temp_dir("no-token");
    let config = write_config(&dir, "");
    let out = okoala(&["serve", "--check", "--config", config.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("[internal]: missing field `token`"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn environment_overrides_reach_the_process() {
    let dir = temp_dir("env");
    let secret = write_secret(&dir, 0o440);
    let config = write_config(&dir, "");
    let out = Command::new(env!("CARGO_BIN_EXE_okoala"))
        .args(["serve", "--check", "--config", config.to_str().unwrap()])
        .env_clear()
        .env("OKOALA_INTERNAL__TOKEN__SECRET_FILE", &secret)
        .env("OKOALA_INTERNAL__LISTEN", "\"[::1]:9091\"")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
}

#[test]
fn serve_without_roles_validates_and_exits() {
    let dir = temp_dir("serve");
    let secret = write_secret(&dir, 0o400);
    let config = write_config(
        &dir,
        &format!(
            "[internal]\ntoken = {{ secret_file = '{}' }}\n",
            secret.display()
        ),
    );
    let out = okoala(&["serve", "--config", config.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("no role can run yet"));
}

#[test]
fn unknown_roles_are_rejected() {
    let dir = temp_dir("roles");
    let config = write_config(&dir, "");
    let out = okoala(&[
        "serve",
        "--config",
        config.to_str().unwrap(),
        "--roles",
        "imap,http",
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("unknown roles imap, http"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn usage_errors_exit_2() {
    assert_eq!(okoala(&["serve"]).status.code(), Some(2));
    assert_eq!(okoala(&["nope"]).status.code(), Some(2));
    assert!(okoala(&["serve", "--help"]).status.success());
}
