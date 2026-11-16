use std::path::PathBuf;

/// Where and how to connect. Host keys are always checked strictly; `known_hosts` overrides the user's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshEndpoint {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
    pub known_hosts: Option<PathBuf>,
}

impl SshEndpoint {
    pub fn destination(&self) -> String {
        format!("{}@{}", self.user, self.host)
    }

    /// The options shared by `ssh` and `sftp` (which spell the port flag differently).
    pub(crate) fn common_options(&self) -> Vec<String> {
        let mut options = vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=yes".into(),
        ];
        if let Some(identity) = &self.identity_file {
            options.extend(["-i".into(), identity.display().to_string()]);
            options.extend(["-o".into(), "IdentitiesOnly=yes".into()]);
        }
        if let Some(known_hosts) = &self.known_hosts {
            options.extend([
                "-o".into(),
                format!("UserKnownHostsFile={}", known_hosts.display()),
            ]);
        }
        options
    }
}
