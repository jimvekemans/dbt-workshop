//! Everything about *where work comes from* and *what happens to it afterwards*.
//!
//! - [`StagingLayout`]: the `vaultspeed/` directory conventions.
//! - [`SourceDiscovery`]: finds sources; [`PmlDiscovery`] always outranks generations.
//! - [`ArtifactReader`]: reads a zip or directory fully into memory (nothing is extracted).
//! - [`SourceHandler`]: turns files into statement groups and DAG files.
//! - [`SourceLifecycle`]: archives zips / consumes staged PML after a run.
//! - [`DeployedSnapshot`]: maintains the human-readable `deployed/` mirror.

mod discovery;
mod error;
mod file;
mod grouping;
mod handler;
mod layout;
mod lifecycle;
mod loader;
mod reader;
mod snapshot;

pub use discovery::{
    DiscoveredSource, PmlDiscovery, ReleaseZipDiscovery, SourceDiscovery, SourceLocation,
};
pub use error::SourceError;
pub use file::SourceFile;
pub use grouping::{DropCreatePairing, GroupingStrategy, PerFileGrouping};
pub use handler::{DagFiles, GenerationHandler, PmlHandler, SourceContent, SourceHandler};
pub use layout::StagingLayout;
pub use lifecycle::{PmlLifecycle, SourceLifecycle, SourceOutcome, ZipArchiveLifecycle};
pub use loader::ArtifactLoader;
pub use reader::{ArtifactReader, DirectoryReader, ZipArchiveReader, open_reader};
pub use snapshot::{DeployedSnapshot, SnapshotMode};
