//! `vdp env doctor` driver lookup.

use std::process::Command;

const VDP: &str = env!("CARGO_BIN_EXE_vdp");

#[test]
fn env_doctor_honours_vdp_adbc_driver() {
    let dir = tempfile::tempdir().unwrap();
    let driver = dir.path().join("custom_adbc.so");
    std::fs::write(&driver, b"").unwrap();

    let out = Command::new(VDP)
        .args(["--no-vault", "env", "doctor"])
        .current_dir(dir.path())
        .env("VDP_ADBC_DRIVER", &driver)
        .env_remove("VDP_CONFIG")
        .env_remove("VDP_PROJECT_DIR")
        .env_remove("DBT_DEPLOY_TARGET")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let expected = format!("ok   {:<22} {}", "singlestore driver", driver.display());
    assert!(
        stdout.lines().any(|line| line.trim_end() == expected),
        "expected `{expected}` in stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
