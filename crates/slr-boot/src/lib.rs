//! Headless bootstrap operations with explicit engine boundaries.
pub mod clock_replay;
pub mod frame_timer;
pub mod settings;

pub mod native_event_route;
pub mod native_vm_platform;
#[cfg(test)]
mod oracle_fixture;
