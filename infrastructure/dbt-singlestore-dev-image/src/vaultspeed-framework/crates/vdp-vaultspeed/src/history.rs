//! The object history: every statement of every run, linked to the database object it changes, so that an
//! object's lifespan (created, altered, loaded, renamed, dropped) can be read back.
//!
//! The engine only knows the [`ChangeRecorder`] port (defined in the domain crate). The DuckDB implementation
//! lives in its own crate (`vdp-vaultspeed-history`). The history never decides what runs (the ledger does), so
//! a failing recorder is logged and never stops a deployment.

use std::sync::Mutex;

use vdp_vaultspeed_domain::ChangeEvent;
pub use vdp_vaultspeed_domain::{ChangeRecorder, HistoryError, NoHistory};

/// Keeps events in memory; the test double.
#[derive(Debug, Default)]
pub struct MemoryHistory {
    events: Mutex<Vec<ChangeEvent>>,
}

impl MemoryHistory {
    /// Creates an empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// The events recorded so far, in order.
    pub fn events(&self) -> Vec<ChangeEvent> {
        self.events.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl ChangeRecorder for MemoryHistory {
    fn record(&self, event: &ChangeEvent) -> Result<(), HistoryError> {
        self.events
            .lock()
            .map_err(|_| HistoryError::new("memory history mutex poisoned"))?
            .push(event.clone());
        Ok(())
    }
}
