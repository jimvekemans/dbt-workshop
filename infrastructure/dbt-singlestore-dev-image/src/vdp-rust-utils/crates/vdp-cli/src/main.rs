use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use vdp_cli::args::Cli;
use vdp_cli::{bootstrap, commands};

fn main() -> ExitCode {
    let mut raw = std::env::args_os();
    let program = raw.next().unwrap_or_default();
    // Installed as `dbt` (a link to `vdp`), it is the dbt wrapper and every argument belongs to dbt.
    if invoked_as_dbt(&program) {
        bootstrap::init_logging(false, "warn");
        let dbt_args: Vec<OsString> = raw.collect();
        return exit(commands::dbt::wrapper(
            &commands::dbt::multicall_globals(),
            &dbt_args,
        ));
    }
    let cli = Cli::parse();
    bootstrap::init_logging(cli.verbose, if cli.is_wrapper() { "warn" } else { "info" });
    exit(commands::run(cli))
}

fn invoked_as_dbt(program: &OsString) -> bool {
    Path::new(program)
        .file_name()
        .is_some_and(|name| name == "dbt" || name == "dbt.exe")
}

fn exit(result: anyhow::Result<vdp_core::Outcome>) -> ExitCode {
    match result {
        Ok(outcome) => ExitCode::from(outcome.exit_code()),
        Err(error) => {
            eprintln!("vdp: error: {error:#}");
            ExitCode::from(2)
        }
    }
}
