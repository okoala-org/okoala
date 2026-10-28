// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Repository checks and helpers: `cargo xtask <command>`.

mod git;
mod levels;
mod spdx;
mod trust;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <command>

commands:
  check-spdx                  every tracked .rs file has the copyright + SPDX header
  check-trust [RANGE]         no AI co-author trailer on commits touching T0 paths, and the lint policy
                              RANGE is a git range such as origin/main..HEAD, or a single commit;
                              default: origin/main..HEAD, or only HEAD when origin/main does not exist
  levels PATH...              print the trust level of each path (see TRUST_LEVELS.toml)";

fn run(args: &[String]) -> Result<(), String> {
    let root = git::toplevel()?;
    match args.first().map(String::as_str) {
        Some("check-spdx") if args.len() == 1 => spdx::run(&root),
        Some("check-trust") if args.len() <= 2 => {
            trust::run(&root, args.get(1).map(String::as_str))
        }
        Some("levels") if args.len() >= 2 => trust::levels(&root, &args[1..]),
        _ => Err(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}
