// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared test helpers: section types and temporary secret files.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use okoala_config::{Section, SectionDescriptor};
use okoala_secret::Secret;
use serde::Deserialize;

/// A value that must never appear in any error or `Debug` output.
pub const TEST_SECRET: &str = "s3cr3t-Value-0f-the-Test";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    #[serde(default = "default_port")]
    pub port: u16,
    pub hostname: String,
    #[serde(default)]
    pub tls: Tls,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ciphers: Vec<String>,
}

fn default_port() -> u16 {
    8080
}

impl Section for Http {
    const NAME: &'static str = "http";
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Internal {
    #[serde(deserialize_with = "okoala_config::secret::string")]
    pub token: Secret<String>,
    #[serde(default, deserialize_with = "okoala_config::secret::optional_string")]
    pub hook_token: Option<Secret<String>>,
    #[serde(default, deserialize_with = "okoala_config::secret::optional_bytes")]
    pub seed: Option<Secret<Vec<u8>>>,
}

impl Section for Internal {
    const NAME: &'static str = "internal";
}

pub fn registry() -> okoala_config::Registry {
    let mut registry = okoala_config::Registry::new();
    registry.register(SectionDescriptor::of::<Http>()).unwrap();
    registry
        .register(SectionDescriptor::of::<Internal>())
        .unwrap();
    registry
}

/// A fresh, empty directory for one test.
pub fn temp_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("okoala-config-tests")
        .join(format!("{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes `contents` to `dir/name` with `mode` and returns the path.
pub fn secret_file(dir: &Path, name: &str, contents: &[u8], mode: u32) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    set_mode(&path, mode);
    path
}

#[cfg(unix)]
pub fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[cfg(not(unix))]
pub fn set_mode(_path: &Path, _mode: u32) {}

/// A config with the required `http` and `internal` sections, the token read from `token_path`.
pub fn config_with_token(token_path: &Path) -> String {
    format!(
        "[http]\nhostname = \"mail.example.com\"\n\n[internal]\ntoken = {{ file = '{}' }}\n",
        token_path.display()
    )
}

/// Every message of a failed load, and its `Display`, must not contain the test secret.
pub fn assert_no_secret(errors: &okoala_config::ConfigErrors) {
    let all = format!("{errors}\n{errors:?}");
    assert!(!all.contains(TEST_SECRET), "secret leaked: {all}");
}
