// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Core dumps are disabled: in this test process by calling the function, and in the `okoala` process itself,
//! inspected through `/proc` while it waits on its configuration file.

#![cfg(target_os = "linux")]
// Helpers outside `#[test]` functions unwrap too; a failure there is a test failure.
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{DumpableBehavior, Resource, dumpable_behavior, getrlimit};

/// Runs in its own test binary, so changing this process's limits affects no other test.
#[test]
fn disable_core_dumps_sets_the_limit_and_dumpable_flag() {
    okoala::hardening::disable_core_dumps().unwrap();
    let limit = getrlimit(Resource::Core);
    assert_eq!((limit.current, limit.maximum), (Some(0), Some(0)));
    assert_eq!(dumpable_behavior().unwrap(), DumpableBehavior::NotDumpable);
    // Idempotent: a second call (limits already 0) still succeeds.
    okoala::hardening::disable_core_dumps().unwrap();
}

fn temp_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("okoala-hardening-tests")
        .join(format!("{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn core_limit_line(pid: u32) -> Option<String> {
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).ok()?;
    limits
        .lines()
        .find(|l| l.starts_with("Max core file size"))
        .map(str::to_owned)
}

#[test]
fn the_okoala_process_cannot_dump_core() {
    let dir = temp_dir("process");
    // The configuration is a FIFO, so the process blocks reading it, after hardening, until this test writes.
    let config = dir.join("okoala.toml");
    let status = Command::new("mkfifo").arg(&config).status().unwrap();
    assert!(status.success());
    let mut child = Command::new(env!("CARGO_BIN_EXE_okoala"))
        .args(["serve", "--check", "--config", config.to_str().unwrap()])
        .env_clear()
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();

    // Hardening runs right after start; poll until both effects show, or give up after 10 s and assert.
    let as_root = rustix::process::getuid().is_root();
    let deadline = Instant::now() + Duration::from_secs(10);
    let (line, owner) = loop {
        let line = core_limit_line(pid);
        let owner = std::fs::metadata(format!("/proc/{pid}/status"))
            .map(|m| m.uid())
            .ok();
        let limit_zero = line.as_deref().is_some_and(|l| {
            let fields: Vec<_> = l.split_whitespace().collect();
            fields.get(4) == Some(&"0") && fields.get(5) == Some(&"0")
        });
        if (limit_zero && (as_root || owner == Some(0))) || Instant::now() > deadline {
            break (line, owner);
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    // Unblock the process (an empty config is invalid, so it exits 1), then check.
    std::fs::OpenOptions::new()
        .write(true)
        .open(&config)
        .unwrap()
        .write_all(b"")
        .unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(1));

    let line = line.unwrap();
    let fields: Vec<_> = line.split_whitespace().collect();
    assert_eq!(
        (fields.get(4), fields.get(5)),
        (Some(&"0"), Some(&"0")),
        "{line}"
    );
    // A non-dumpable process's /proc entries belong to root (checkable only when the tests do not run as root).
    if !as_root {
        assert_eq!(owner, Some(0), "the okoala process is still dumpable");
    }
}
