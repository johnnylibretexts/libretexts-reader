//! The one flag that stops an import in flight.
//!
//! Cooperative, and modelled on `SupertonicDownloadCancel`: the check lives in
//! the progress callback the fetch loops already call and `?`-propagate, so
//! every place an import reports progress is also a place it can be stopped.
//! That costs at most one more page -- progress is reported after a page
//! lands, so a cancel takes effect before the *next* request rather than
//! interrupting the one in flight.
//!
//! Only the catalog importers carry it. EPUB, PDF, pasted text and article
//! URLs are a single local read or one request, and finish before a reader
//! could reach for Cancel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{AppError, AppResult};

/// What a cancelled import fails with.
pub const IMPORT_CANCELLED: &str = "Import cancelled.";

/// A handle on the cancel flag for the import in flight.
///
/// Cloning shares the flag rather than copying it: the command that requests
/// the cancel and the import that observes it are different tasks holding this
/// through Tauri's managed state.
#[derive(Clone, Default)]
pub struct ImportCancel(Arc<AtomicBool>);

impl ImportCancel {
    /// Ask the import in flight to stop. Takes effect at its next page.
    pub fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Forget a cancel that has already done its work.
    ///
    /// Called when an import starts, never when one ends. A cancel can land
    /// after the fetch loop has exited -- pressed in the moment between the
    /// last page and the persist -- and clearing on the way out would swallow
    /// it, leaving the flag set for the next import to die on for no reason
    /// the reader could see.
    pub fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    /// Fail the import the reader asked to stop.
    pub fn check(&self) -> AppResult<()> {
        if self.0.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled(IMPORT_CANCELLED.into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ImportCancel, IMPORT_CANCELLED};

    #[test]
    fn an_untouched_flag_lets_an_import_run() {
        let cancel = ImportCancel::default();

        assert!(cancel.check().is_ok());
    }

    #[test]
    fn a_requested_cancel_fails_the_import_by_a_name_the_webview_can_match() {
        let cancel = ImportCancel::default();
        cancel.request();

        let error = cancel.check().expect_err("a cancelled import should fail");

        assert!(
            error.to_string().contains(IMPORT_CANCELLED),
            "the failure should carry the cancellation marker, got: {error}"
        );
    }

    /// The flag is cleared when an import *starts*, never when one ends.
    ///
    /// A cancel can land after the fetch loop has already exited -- the reader
    /// pressing it in the moment between the last page and the persist. Clearing
    /// on the way out would swallow that request and leave the flag set for the
    /// next import, which would then die on its first page for no reason the
    /// reader could see.
    #[test]
    fn clearing_on_start_lets_the_next_import_run() {
        let cancel = ImportCancel::default();
        cancel.request();
        assert!(cancel.check().is_err());

        cancel.clear();

        assert!(
            cancel.check().is_ok(),
            "the next import must not inherit the last one's cancel"
        );
    }

    /// Cloning hands out another handle to the same flag, not a copy of it --
    /// the command that cancels and the import that checks are different tasks
    /// holding the state separately.
    #[test]
    fn a_clone_shares_the_flag_rather_than_copying_it() {
        let cancel = ImportCancel::default();
        let handle = cancel.clone();

        handle.request();

        assert!(
            cancel.check().is_err(),
            "a cancel through one handle must be visible through the other"
        );
    }
}
