//! `vdp airflow sync --tag <tag>` (`update_airflow.py` / `AirflowManager.sync_project`).
//!
//! On the Airflow host: fetch the dbt project's tags, reset the checkout to the tag and hard-link its DAGs into
//! the DAG folder. Unlike the Python version, the tag is required (it ran `git reset --hard None` without one),
//! host keys are checked, and a failing remote command fails the sync instead of being logged and ignored.

mod settings;

use thiserror::Error;
use vdp_secrets::SecretProvider;
use vdp_ssh::{
    CommandOutput, OpenSshShell, RemoteShell, SecretNames, SshAccess, SshError, shell_quote,
};

pub use settings::AirflowSettings;

#[derive(Debug, Error)]
pub enum AirflowError {
    #[error("`{0}` is not a usable git tag")]
    InvalidTag(String),
    #[error("missing secret: {0}")]
    MissingSecret(String),
    #[error(transparent)]
    Ssh(#[from] SshError),
}

/// A tag or ref name safe to put on a remote command line (and not an option).
pub fn validate_tag(tag: &str) -> Result<(), AirflowError> {
    let valid = !tag.is_empty()
        && !tag.starts_with('-')
        && !tag.contains("..")
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-/+".contains(c));
    if valid {
        Ok(())
    } else {
        Err(AirflowError::InvalidTag(tag.to_owned()))
    }
}

/// The remote command for a tag.
pub fn sync_command(settings: &AirflowSettings, tag: &str) -> Result<String, AirflowError> {
    validate_tag(tag)?;
    let project = shell_quote(&settings.remote_project_dir);
    let mut steps = vec![
        format!("cd {project}"),
        "git fetch --tags --all --prune".to_owned(),
        format!("git reset --hard {}", shell_quote(tag)),
    ];
    if settings.link_dags {
        // The glob must stay outside the quotes.
        steps.push(format!(
            "ln -fn {project}/airflow/dags/*.py {}",
            shell_quote(&settings.remote_dags_dir)
        ));
    }
    Ok(steps.join(" && "))
}

/// Runs the sync on `shell`.
pub fn sync(
    shell: &dyn RemoteShell,
    settings: &AirflowSettings,
    tag: &str,
) -> Result<CommandOutput, AirflowError> {
    let command = sync_command(settings, tag)?;
    tracing::info!(target = %shell.describe(), %tag, "syncing the dbt project on Airflow");
    Ok(shell.run(&command)?)
}

/// An SSH shell to the Airflow host from the secrets (`vdp/airflow` in Vault). The private key is written to a
/// private temporary file that lives as long as the returned guard.
pub fn connect(
    settings: &AirflowSettings,
    secrets: &dyn SecretProvider,
) -> Result<(OpenSshShell, Option<tempfile::NamedTempFile>), AirflowError> {
    let names = SecretNames {
        host: settings.host_secrets.clone(),
        user: settings.user_secrets.clone(),
        key: settings.key_secrets.clone(),
        default_user: None,
        default_port: settings.port,
        known_hosts: (!settings.known_hosts.is_empty())
            .then(|| settings.known_hosts.clone().into()),
    };
    let access = SshAccess::from_secrets(secrets, &names)?
        .ok_or_else(|| AirflowError::MissingSecret(settings.host_secrets.join(" or ")))?;
    Ok((OpenSshShell::new(access.endpoint), access.key_file))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    struct Recording(Mutex<Vec<String>>, bool);

    impl RemoteShell for Recording {
        fn describe(&self) -> String {
            "recording".into()
        }
        fn run(&self, command: &str) -> Result<CommandOutput, SshError> {
            self.0.lock().unwrap().push(command.to_owned());
            if self.1 {
                Err(SshError::Remote {
                    target: "recording".into(),
                    status: "exit status: 128".into(),
                    detail: "fatal: ambiguous argument".into(),
                })
            } else {
                Ok(CommandOutput::default())
            }
        }
    }

    #[test]
    fn the_python_command_with_quoting() {
        let command = sync_command(&AirflowSettings::default(), "v2026.05.1").unwrap();
        assert_eq!(
            command,
            "cd /opt/apache_airflow/dbt/dbt-swf && git fetch --tags --all --prune && git reset --hard v2026.05.1 \
             && ln -fn /opt/apache_airflow/dbt/dbt-swf/airflow/dags/*.py /opt/apache_airflow/dags/dbt"
        );
    }

    #[test]
    fn rejects_dangerous_tags_and_reports_remote_failures() {
        for bad in ["", "--upload-pack=x", "a b", "v1;rm -rf /", "a..b", "None$"] {
            assert!(validate_tag(bad).is_err(), "{bad}");
        }
        let failing = Recording(Mutex::default(), true);
        assert!(sync(&failing, &AirflowSettings::default(), "v1").is_err());
        assert_eq!(failing.0.lock().unwrap().len(), 1);
    }
}
