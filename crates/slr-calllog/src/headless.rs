//! Ordered, per-thread capture shared by the headless native backends.
//!
//! A replay driver resets this stream at session start and drains it at replay
//! boundaries. Only explicit headless installers append here; ordinary native
//! implementations keep their existing subsystem-local records. This stream
//! records native calls, not an emulation of D3D9, DirectSound or DirectInput.
use crate::LogRecord;
use std::cell::RefCell;

thread_local! {
    static RECORDS: RefCell<Vec<LogRecord>> = const { RefCell::new(Vec::new()) };
}

/// Append one native call in execution order.
pub fn record(call: LogRecord) {
    RECORDS.with(|records| records.borrow_mut().push(call));
}

/// Take this thread's ordered calls, leaving the capture empty.
pub fn take_records() -> Vec<LogRecord> {
    RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()))
}

/// Start a fresh capture on this thread.
pub fn reset() {
    RECORDS.with(|records| records.borrow_mut().clear());
}
