//! Isolated carrier for the archived Bochs-derived value experiment.
//! No production dependency, trust classification, or acceptance result.
mod ffi;
#[rustfmt::skip]
pub mod intel_trig;
use std::sync::{Mutex, MutexGuard};
static SF_LOCK: Mutex<()> = Mutex::new(());
fn lock() -> MutexGuard<'static, ()> {
    SF_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
#[derive(Clone, Copy, Debug)]
pub struct X87(ffi::ExtF80);
impl X87 {
    pub fn from_bytes(bytes: [u8; 10]) -> Self {
        Self(ffi::ExtF80::from_bytes(bytes))
    }
    pub fn to_bytes(self) -> [u8; 10] {
        self.0.to_bytes()
    }
    pub fn from_f32(value: f32) -> Self {
        let _guard = lock();
        Self(ffi::f32_to_ext(value))
    }
    pub fn from_i32(value: i32) -> Self {
        let _guard = lock();
        Self(ffi::i32_to_ext(value))
    }
}
