//! Test-only source store observation shared by composed initializer helpers.
//! Observers receive the full source operand, including byte-register high bits.
type Observer = fn(u32, u32, u32);
thread_local! {static OBSERVER: std::cell::Cell<Option<Observer>> = const {std::cell::Cell::new(None)};}
pub fn set(observer: Option<Observer>) {
    OBSERVER.with(|slot| slot.set(observer));
}
pub fn observe(address: u32, size: u32, value: u32) {
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.get() {
            observer(address, size, value);
        }
    });
}
