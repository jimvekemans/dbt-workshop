//! `dev`, `val` and `prd` are aliases of `LDV`, `ABT` and `PRD`, as the dbt workshop's `vdp.toml` uses them.

use std::path::Path;
use std::process::{Command, Output};

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

/// A project laid out like the workshop's: `dev`, `val` and `prd` sections, `dev` as the default environment and the
/// run results in a database per environment prefix.
fn workshop_project(dir: &Path) {
    std::fs::write(
        dir.join("dbt_project.yml"),
        "name: workshop\nprofile: workshop\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("vdp.toml"),
        "[core]\ndefault_environment = \"dev\"\n\
         [secrets]\nprecedence = [\"env\"]\n\
         [environments.dev]\nvault_url = \"\"\nca_certificate = \"\"\n\
         [environments.val]\nvault_url = \"\"\nca_certificate = \"\"\n\
         [environments.prd]\nvault_url = \"\"\nca_certificate = \"\"\n\
         [qa]\nrun_results_table = \"{env_short}_vdp_mgmt.dbt_run_results\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("target/manifest.json"), "{\"nodes\":{}}").unwrap();
}

/// `vdp` in `dir`, without any `DBT_DEPLOY_TARGET` of the test process, and with `deploy_target` when given.
fn vdp(dir: &Path, deploy_target: Option<&str>, args: &[&str]) -> Output {
    let mut command = Command::new(VDP);
    command
        .args(["--no-vault"])
        .args(args)
        .current_dir(dir)
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_PROJECT_DIR")
        .env_remove("DBT_DEPLOY_TARGET");
    if let Some(target) = deploy_target {
        command.env("DBT_DEPLOY_TARGET", target);
    }
    command.output().unwrap()
}

fn environment_line(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find(|l| l.starts_with("environment"))
        .unwrap_or_else(|| {
            panic!(
                "no environment line in:\n{stdout}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            )
        })
        .to_owned()
}

#[test]
fn the_aliases_select_the_built_in_environments() {
    let dir = tempfile::tempdir().unwrap();
    workshop_project(dir.path());
    let cases: [(Option<&str>, &[&str], &str); 7] = [
        (None, &["env", "show"], "LDV"),
        (None, &["--env", "dev", "env", "show"], "LDV"),
        (None, &["--env", "VAL", "env", "show"], "ABT"),
        (None, &["--env", "prd", "env", "show"], "PRD"),
        (None, &["--env", "cbt", "env", "show"], "CBT"),
        (Some("val"), &["env", "show"], "ABT"),
        (Some("dev"), &["env", "show"], "LDV"),
    ];
    for (target, args, expected) in cases {
        let out = vdp(dir.path(), target, args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert_eq!(
            environment_line(&out).split_whitespace().last(),
            Some(expected),
            "{target:?} {args:?}"
        );
    }
}

#[test]
fn an_unknown_environment_lists_the_four_built_ins_once() {
    let dir = tempfile::tempdir().unwrap();
    workshop_project(dir.path());
    for (target, args) in [
        (None, &["--env", "xyz", "env", "show"][..]),
        (Some("xyz"), &["env", "show"][..]),
    ] {
        let out = vdp(dir.path(), target, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("'xyz'"), "{stderr}");
        assert!(
            stderr.contains("(configured: LDV, ABT, CBT, PRD;"),
            "no dev/val/prd duplicates expected: {stderr}"
        );
    }
}

/// `{env_short}` keeps the workshop's `dev_vdp_mgmt` database although the environment is now `LDV`. The run fails
/// on the missing connection; the error names the table it was going to read.
#[cfg(feature = "qa")]
#[test]
fn env_short_renders_the_workshop_run_results_table() {
    let dir = tempfile::tempdir().unwrap();
    workshop_project(dir.path());
    let cases: [(Option<&str>, &[&str], &str); 4] = [
        (None, &["--env", "dev", "qa", "view-candidates"], "dev"),
        (Some("dev"), &["qa", "view-candidates"], "dev"),
        (None, &["--env", "val", "qa", "view-candidates"], "val"),
        (Some("prd"), &["qa", "view-candidates"], "prd"),
    ];
    for (target, args, prefix) in cases {
        let out = vdp(dir.path(), target, args);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let table = format!("reading {prefix}_vdp_mgmt.dbt_run_results");
        assert!(stderr.contains(&table), "{target:?} {args:?}: {stderr}");
    }
}
