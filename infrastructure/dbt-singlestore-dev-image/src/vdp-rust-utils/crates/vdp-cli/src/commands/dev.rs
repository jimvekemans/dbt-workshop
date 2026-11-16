use anyhow::Result;
use vdp_core::{Context, Outcome};

use crate::args::DevCommand;
use crate::commands::dbt::engine_binary;

pub(super) fn run(ctx: &Context, command: &DevCommand) -> Result<Outcome> {
    match command {
        DevCommand::Grammar { dbt, out } => {
            let binary = match dbt {
                Some(path) => path.clone(),
                None => engine_binary(ctx.config())?,
            };
            let grammar = vdp_dbt::flags::generate::from_binary(&binary)?;
            let json = serde_json::to_string_pretty(&grammar)? + "\n";
            match out {
                Some(path) => std::fs::write(path, json)?,
                None => print!("{json}"),
            }
            Ok(Outcome::Success)
        }
    }
}
