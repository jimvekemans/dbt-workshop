use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use vdp_cli::args::{Cli, Command};
use vdp_cli::{commands, external, runtime};

fn main() -> ExitCode {
    let mut raw = std::env::args_os();
    let program = raw.next().unwrap_or_default();
    // Installed as `dbt` (a link to `vdp`), it is the dbt wrapper and every argument belongs to dbt.
    if invoked_as_dbt(&program) {
        let dbt_args: Vec<OsString> = raw.collect();
        let global = match commands::dbt::multicall_globals() {
            Ok(global) => global,
            Err(error) => return runtime::exit("vdp", Err(error)),
        };
        runtime::init_logging(&global, true);
        return runtime::exit("vdp", commands::dbt::wrapper(&global, &dbt_args));
    }
    let cli = Cli::parse();
    if let Command::External(external) = &cli.command {
        // The original arguments, not the parsed ones: whatever the other binary understands passes through.
        let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
        return external::run(external, &argv);
    }
    let global = cli.global.clone();
    let is_wrapper = cli.is_wrapper();
    runtime::run(
        runtime::RunContext {
            program: "vdp",
            global: &global,
            is_wrapper,
        },
        || commands::run(cli),
    )
}

fn invoked_as_dbt(program: &OsString) -> bool {
    Path::new(program)
        .file_name()
        .is_some_and(|name| name == "dbt" || name == "dbt.exe")
}
