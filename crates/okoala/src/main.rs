// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! The `okoala` command.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use okoala::{roles, serve};
use okoala_config::ProcessEnvironment;

#[derive(Debug, Parser)]
#[command(
    name = "okoala",
    version,
    about = "OKoala mail, calendar, contacts and chat server"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Load and validate the configuration, then run the selected roles.
    #[command(
        long_about = "Load and validate the configuration, then run the selected roles.\n\n\
        This build has no roles yet: `serve` loads and validates the configuration, reports every \
        error, and exits (0 if the configuration is valid). Values come from the TOML file and from \
        OKOALA_<SECTION>__<KEY> environment variables; secrets are read from the files or variables the \
        configuration names."
    )]
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
struct ServeArgs {
    /// The configuration file (TOML).
    #[arg(long, value_name = "PATH")]
    config: PathBuf,
    /// The roles to run, comma-separated (default: all). This build has none.
    #[arg(long, value_name = "ROLES", value_delimiter = ',')]
    roles: Option<Vec<String>>,
    /// Only load and validate the configuration; exit non-zero if it is invalid.
    #[arg(long)]
    check: bool,
}

fn main() -> ExitCode {
    // Before anything else, in particular before any secret is loaded.
    if let Err(e) = okoala::hardening::disable_core_dumps() {
        eprintln!("okoala: {e}");
        return ExitCode::FAILURE;
    }
    // `try_parse` rather than `parse`, which exits the process itself.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            };
        }
    };
    match cli.command {
        Command::Serve(args) => match run_serve(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(errors) => {
                for e in errors {
                    eprintln!("okoala: {e}");
                }
                ExitCode::FAILURE
            }
        },
    }
}

/// Loads the configuration and selects the roles, reporting the errors of both in one run.
fn run_serve(args: &ServeArgs) -> Result<(), Vec<serve::ServeError>> {
    let table = roles::all();
    let config = serve::load(&args.config, &table, &ProcessEnvironment);
    let selected = serve::select(&table, args.roles.as_deref());
    let (config, selected) = match (config, selected) {
        (Ok(config), Ok(selected)) => (config, selected),
        (config, selected) => {
            return Err([config.err(), selected.err()]
                .into_iter()
                .flatten()
                .collect());
        }
    };
    let sections: Vec<_> = config.section_names().collect();
    println!(
        "configuration {} is valid (sections: {})",
        args.config.display(),
        sections.join(", ")
    );
    if !args.check {
        // No role crate exists yet, so nothing runs; running `selected` comes with the first one.
        println!(
            "no role can run yet in this build ({} selected); exiting",
            selected.len()
        );
    }
    // `config` (and the secrets in it) is dropped, and zeroized, here.
    drop(config);
    Ok(())
}
