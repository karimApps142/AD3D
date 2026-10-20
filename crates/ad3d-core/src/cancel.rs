use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

/// Cooperative cancellation flag. Cheap to clone; all clones share one flag.
/// Generators check it once per row of cells and return [`Error::Cancelled`].
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

/// The design doc's name for the same type.
pub type Cancel = CancelToken;

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    /// `Err(Cancelled)` once cancelled.
    #[inline]
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() { Err(Error::Cancelled) } else { Ok(()) }
    }
}
