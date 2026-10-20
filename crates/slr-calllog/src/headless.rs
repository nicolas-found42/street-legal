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
    static OBSERVER: RefCell<Option<fn(&LogRecord)>> = const { RefCell::new(None) };
    static RETAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Subscribe a streaming recorder. The callback runs outside the capture borrow
/// so it may emit correlated diagnostic events. The embedding owns I/O errors.
pub fn configure_stream(observer: Option<fn(&LogRecord)>, retain: bool) {
    OBSERVER.with(|slot| *slot.borrow_mut() = observer);
    RETAIN.with(|slot| slot.set(retain));
}

/// Append one native call in execution order.
pub fn record(call: LogRecord) {
    if let Some(observer) = OBSERVER.with(|slot| *slot.borrow()) {
        observer(&call);
    }
    if RETAIN.with(std::cell::Cell::get) {
        RECORDS.with(|records| records.borrow_mut().push(call));
    }
}

/// Take this thread's ordered calls, leaving the capture empty.
pub fn take_records() -> Vec<LogRecord> {
    RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()))
}

/// Start a fresh capture on this thread.
pub fn reset() {
    RECORDS.with(|records| records.borrow_mut().clear());
}
