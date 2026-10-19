//! `java.lang.Object` / `java.lang.System` / `java.lang.GameType` natives
//! (M1-08, issue #35).
//!
//! One module per class family so the ledger rows stay independent:
//!
//! - [`object`] — `Object.toString`/`disableGC`/`enableGC`/`hashCode`
//!   (rows O1..O4) plus the object/GC-list model they run behind.
//! - [`system`] — all registered System natives, including payload-only
//!   arraycopy and the compiler/package/configuration wrappers.
//! - [`gametype`] — notifications, timers, callbacks and instance creation.
//! - [`engine`] — the required memory/ABI boundary for engine collaborators;
//!   missing backends fail explicitly. Wrapper coverage does not implement
//!   their compiler, package, resource or game-manager subsystems.

pub mod engine;
pub mod gametype;
pub mod object;
pub mod system;
