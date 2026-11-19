//! Publication of rendered DAG bundles.

use std::fmt::Debug;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vdp_ssh::{RemoteTransport, Upload};

use crate::DagError;

/// A file ready for publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedFile {
    pub name: String,
    pub content: String,
}

/// DAGs plus the metadata files they read at parse time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublishBundle {
    pub metadata: Vec<PublishedFile>,
    pub dags: Vec<PublishedFile>,
}

/// Publishes a bundle somewhere. Implementations publish **metadata before DAGs**:
/// Airflow parses a DAG as soon as it appears and the DAG opens its metadata files.
pub trait DagPublisher: Send + Sync + Debug {
    fn describe(&self) -> String;
    fn publish(&self, bundle: &PublishBundle) -> Result<(), DagError>;
}

/// Publishes to several targets in order; stops at the first failure.
#[derive(Debug, Clone)]
pub struct CompositePublisher {
    publishers: Vec<Arc<dyn DagPublisher>>,
}

impl CompositePublisher {
    pub fn new(publishers: Vec<Arc<dyn DagPublisher>>) -> Self {
        Self { publishers }
    }
}

impl DagPublisher for CompositePublisher {
    fn describe(&self) -> String {
        self.publishers
            .iter()
            .map(|p| p.describe())
            .collect::<Vec<_>>()
            .join(" + ")
    }

    fn publish(&self, bundle: &PublishBundle) -> Result<(), DagError> {
        self.publishers.iter().try_for_each(|p| p.publish(bundle))
    }
}

/// Writes `vaultspeed/airflow/{dags,meta}` so the rendered DAGs are committed to git.
/// Each file is written to a temporary sibling and renamed into place.
#[derive(Debug, Clone)]
pub struct RepoDirPublisher {
    dags_dir: PathBuf,
    meta_dir: PathBuf,
}

impl RepoDirPublisher {
    pub const fn new(dags_dir: PathBuf, meta_dir: PathBuf) -> Self {
        Self { dags_dir, meta_dir }
    }
}

impl DagPublisher for RepoDirPublisher {
    fn describe(&self) -> String {
        format!("repository ({})", self.dags_dir.display())
    }

    fn publish(&self, bundle: &PublishBundle) -> Result<(), DagError> {
        for file in &bundle.metadata {
            write_atomically(&self.meta_dir, file)?;
        }
        for file in &bundle.dags {
            write_atomically(&self.dags_dir, file)?;
        }
        Ok(())
    }
}

fn write_atomically(dir: &Path, file: &PublishedFile) -> Result<(), DagError> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| DagError::Io { path, source }
    };
    fs::create_dir_all(dir).map_err(io(dir))?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(io(dir))?;
    temp.write_all(file.content.as_bytes()).map_err(io(dir))?;
    let target = dir.join(&file.name);
    temp.persist(&target).map_err(|e| DagError::Io {
        path: target.clone(),
        source: e.error,
    })?;
    Ok(())
}

/// Publishes to the Airflow host's DAG and metadata directories over any [`RemoteTransport`] (in production
/// [`vdp_ssh::OpenSshTransport`]: strict host keys, temporary name + rename per file).
#[derive(Debug, Clone)]
pub struct SftpPublisher {
    transport: Arc<dyn RemoteTransport>,
    remote_dags_dir: String,
    remote_meta_dir: String,
}

impl SftpPublisher {
    pub fn new(
        transport: Arc<dyn RemoteTransport>,
        remote_dags_dir: String,
        remote_meta_dir: String,
    ) -> Self {
        Self {
            transport,
            remote_dags_dir,
            remote_meta_dir,
        }
    }
}

impl DagPublisher for SftpPublisher {
    fn describe(&self) -> String {
        format!("airflow ({})", self.transport.describe())
    }

    fn publish(&self, bundle: &PublishBundle) -> Result<(), DagError> {
        let join = |dir: &str, name: &str| format!("{}/{name}", dir.trim_end_matches('/'));
        let upload = |dir: &str, file: &PublishedFile| Upload {
            content: file.content.clone().into_bytes(),
            remote_path: join(dir, &file.name),
        };
        let files: Vec<Upload> = bundle
            .metadata
            .iter()
            .map(|f| upload(&self.remote_meta_dir, f))
            .chain(bundle.dags.iter().map(|f| upload(&self.remote_dags_dir, f)))
            .collect();
        self.transport.upload(&files)?;
        tracing::info!(files = files.len(), target = %self.transport.describe(), "uploaded to Airflow");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use vdp_ssh::SshError;

    use super::*;

    #[derive(Debug, Default)]
    struct RecordingTransport(Mutex<Vec<String>>);

    impl RemoteTransport for RecordingTransport {
        fn describe(&self) -> String {
            "recording".into()
        }
        fn upload(&self, files: &[Upload]) -> Result<(), SshError> {
            self.0
                .lock()
                .unwrap()
                .extend(files.iter().map(|f| f.remote_path.clone()));
            Ok(())
        }
    }

    fn bundle() -> PublishBundle {
        PublishBundle {
            metadata: vec![PublishedFile {
                name: "m.json".into(),
                content: "{}".into(),
            }],
            dags: vec![PublishedFile {
                name: "FL_DAG_X.py".into(),
                content: "x = 1\n".into(),
            }],
        }
    }

    #[test]
    fn sftp_uploads_metadata_before_dags() {
        let transport = Arc::new(RecordingTransport::default());
        SftpPublisher::new(
            transport.clone(),
            "/opt/af/dags/".into(),
            "/opt/af/vaultspeed_mtd".into(),
        )
        .publish(&bundle())
        .unwrap();
        assert_eq!(
            *transport.0.lock().unwrap(),
            ["/opt/af/vaultspeed_mtd/m.json", "/opt/af/dags/FL_DAG_X.py"]
        );
    }

    #[test]
    fn repo_publisher_writes_files() {
        let dir = tempfile::tempdir().unwrap();
        RepoDirPublisher::new(dir.path().join("dags"), dir.path().join("meta"))
            .publish(&bundle())
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("dags/FL_DAG_X.py")).unwrap(),
            "x = 1\n"
        );
        assert!(dir.path().join("meta/m.json").exists());
    }
}
