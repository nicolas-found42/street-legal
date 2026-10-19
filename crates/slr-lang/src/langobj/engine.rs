//! Required engine boundary for native wrappers. A backend supplies the real
//! compiler, package manager and game objects. Missing backends fail explicitly;
//! their results are never fabricated by the native layer.
use std::cell::RefCell;

/// One engine call in the original ABI: ECX plus stack arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub address: u32,
    pub this: u32,
    pub args: Vec<u32>,
}

/// Engine memory and operation boundary. `scratch` is zeroed temporary storage
/// valid until the current native returns. Backends may reclaim it afterwards.
pub trait Engine {
    fn read(&self, address: u32) -> u32;
    fn write(&mut self, address: u32, value: u32);
    fn scratch(&mut self, words: usize) -> u32;
    fn call(&mut self, call: Call) -> u32;
}
thread_local! {
    static ENGINE: RefCell<Option<Box<dyn Engine>>> = RefCell::new(None);
}

pub fn install(engine: Box<dyn Engine>) {
    ENGINE.with(|slot| *slot.borrow_mut() = Some(engine));
}
pub fn reset() {
    ENGINE.with(|slot| *slot.borrow_mut() = None);
}
pub fn with_engine<R>(f: impl FnOnce(&mut dyn Engine) -> R) -> R {
    ENGINE.with(|slot| {
        let mut slot = slot.borrow_mut();
        f(slot
            .as_deref_mut()
            .expect("native requires an installed engine backend"))
    })
}
pub(crate) fn invoke(engine: &mut dyn Engine, address: u32, this: u32, args: &[u32]) -> u32 {
    engine.call(Call {
        address,
        this,
        args: args.to_vec(),
    })
}

/// Unlink the common four-word intrusive link `{prev,next,value,owner}`.
pub(crate) fn unlink(engine: &mut dyn Engine, link: u32) {
    let owner = engine.read(link + 12);
    if owner == 0 {
        return;
    }
    let prev = engine.read(link);
    let next = engine.read(link + 4);
    engine.write(if prev == 0 { owner + 0x48 } else { prev + 4 }, next);
    if next != 0 {
        engine.write(next, prev);
    }
}
