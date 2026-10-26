//! Source namespace construction and raw CRT find state. Each entered source
//! body retains its actual CALL frame. Environmental allocation and Win32 find
//! leaves are explicit collaborators; semantic FindHost records are not inputs.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
#[path = "native_vm_find.rs"]
mod find;
#[path = "native_vm_namespace_map.rs"]
mod mapping;
#[path = "native_vm_find_sort.rs"]
mod sorting;

pub trait Environment: Allocator {
    fn free(&mut self, m: &mut Space, pointer: u32) -> Result<u32, String>;
    fn find_first(&mut self, m: &mut Space, pattern: u32, destination: u32) -> Result<u32, String>;
    fn find_next(&mut self, m: &mut Space, handle: u32, destination: u32) -> Result<u32, String>;
    fn find_close(&mut self, m: &mut Space, handle: u32) -> Result<u32, String>;
}
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! {static OBSERVER:std::cell::Cell<Option<StoreObserver>>=const {std::cell::Cell::new(None)};}
#[cfg(test)]
pub(crate) fn set_test_store_observer(observer: Option<fn(u32, u32, u32)>) {
    OBSERVER.with(|slot| slot.set(observer));
}
fn observe(address: u32, size: u32, value: u32) {
    #[cfg(test)]
    OBSERVER.with(|slot| {
        if let Some(f) = slot.get() {
            f(address, size, value);
        }
    });
    #[cfg(not(test))]
    let _ = (address, size, value);
}
fn write(m: &mut Space, address: u32, value: u32) {
    observe(address, 4, value);
    m.w32(address, value);
}
fn byte(m: &mut Space, address: u32, value: u8) {
    observe(address, 1, value.into());
    m.w8(address, value);
}
// Unicorn's raw MOV-byte write observation carries the complete source
// register. Retain that stronger operand observation as well as the actual
// low-byte memory write; callers compare it without masking the fixture.
fn byte_from_cl(m: &mut Space, address: u32) {
    let value = stack::reg(m, Reg::Ecx);
    observe(address, 1, value);
    m.w8(address, value as u8);
}
fn set(m: &mut Space, register: Reg, value: u32) {
    stack::set_reg(m, register, value);
}
fn low(m: &mut Space, register: Reg, value: u8) {
    set(
        m,
        register,
        (stack::reg(m, register) & !255) | u32::from(value),
    );
}
fn scan(m: &mut Space, source: u32) -> Result<u32, String> {
    let mut length = 0;
    while length < 0x100000 {
        if m.r8(source.wrapping_add(length)) == 0 {
            set(m, Reg::Edi, source.wrapping_add(length + 1));
            set(m, Reg::Ecx, length + 1);
            return Ok(length + 1);
        }
        length += 1;
    }
    Err("NativeVmNamespace: unbounded name input".into())
}
// Actual551120 ->5548c0. The helper's caller has already formed its cdecl
// argument words; no separate stack entry or captured output is synthesized.
fn bounded_copy(m: &mut Space, destination: u32, source: u32, capacity: u32) {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, capacity);
        set(m, Reg::Ecx, source);
        set(m, Reg::Edx, destination);
        stack::push(m, capacity);
        stack::push(m, source);
        stack::push(m, destination);
        stack::call(m, 0x551134, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, destination);
                stack::push_reg(m, Reg::Ebx);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                set(m, Reg::Esi, source);
                let mut ch = m.r8(source);
                low(m, Reg::Ecx, ch);
                stack::push_reg(m, Reg::Edi);
                set(m, Reg::Edi, 1);
                set(m, Reg::Edx, destination);
                let mut offset = 0;
                if ch != 0 {
                    set(m, Reg::Ebx, capacity);
                    loop {
                        let index = stack::reg(m, Reg::Edi);
                        set(m, Reg::Ebp, index);
                        set(m, Reg::Edi, index + 1);
                        if index as i32 >= capacity as i32 {
                            break;
                        }
                        byte_from_cl(m, destination + offset);
                        offset += 1;
                        ch = m.r8(source + offset);
                        low(m, Reg::Ecx, ch);
                        set(m, Reg::Edx, destination + offset);
                        set(m, Reg::Esi, source + offset);
                        if ch == 0 {
                            break;
                        }
                    }
                }
                stack::add_sp(m, 12);
                byte(m, destination + offset, 0);
                stack::add_sp(m, 4);
            });
        });
        stack::add_sp(m, 12);
    });
}
// Actual554860, including its saved ESI and CL/EDX final scratch.
fn strcpy(m: &mut Space, destination: u32, source: u32) {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, destination);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, source);
        let mut ch = m.r8(source);
        low(m, Reg::Ecx, ch);
        set(m, Reg::Edx, destination);
        let mut offset = 0;
        while ch != 0 {
            byte_from_cl(m, destination + offset);
            offset += 1;
            ch = m.r8(source + offset);
            low(m, Reg::Ecx, ch);
            set(m, Reg::Edx, destination + offset);
            set(m, Reg::Esi, source + offset);
        }
        byte(m, destination + offset, 0);
        stack::add_sp(m, 4);
    });
}

/// The actual bootstrap caller supplies parent0, path0, name60ab64.
/// exe: 0x41cc00
pub fn construct_root(
    m: &mut Space,
    this: u32,
    environment: &mut impl Environment,
) -> Result<u32, String> {
    construct(m, this, 0, 0, 0x60ab64, environment)
}
pub fn construct(
    m: &mut Space,
    this: u32,
    parent: u32,
    path: u32,
    name: u32,
    environment: &mut impl Environment,
) -> Result<u32, String> {
    if this == 0 || name == 0 {
        return Err(
            "NativeVmNamespace: absent namespace/name or unresolved constructor path mapping"
                .into(),
        );
    }
    stack::function(m, &[parent, path, name], Some(this), |m| {
        set(m, Reg::Eax, parent);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, this);
        if parent == 0 {
            set(m, Reg::Eax, this);
        }
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Ebx, name);
        stack::push_reg(m, Reg::Edi);
        write(m, this + 12, stack::reg(m, Reg::Eax));
        set(m, Reg::Eax, 0);
        let n = scan(m, name)?;
        stack::push(m, n);
        let buffer = stack::external(m, 0x41cc27, &[], None, |m| environment.allocate(m, n))?;
        set(m, Reg::Eax, buffer);
        set(m, Reg::Edx, buffer);
        if buffer == 0 {
            return Err("NativeVmNamespace: name allocation failure".into());
        }
        set(m, Reg::Eax, 0);
        write(m, this, buffer);
        let n = scan(m, name)?;
        stack::push(m, n);
        stack::push(m, name);
        stack::push(m, buffer);
        stack::call(m, 0x41cc3e, &[], None, |m| bounded_copy(m, buffer, name, n));
        stack::push(m, 12);
        let children = stack::external(m, 0x41cc45, &[], None, |m| environment.allocate(m, 12))?;
        set(m, Reg::Eax, children);
        stack::add_sp(m, 20);
        stack::add_sp(m, 8);
        if children == 0 {
            return Err("NativeVmNamespace: child container allocation failure".into());
        }
        for offset in [0, 8, 4] {
            write(m, children + offset, 0);
        }
        write(m, this + 4, children);
        set(m, Reg::Eax, path);
        write(m, this + 8, 0);
        if path != 0 {
            stack::push(m, path);
            set(m, Reg::Ecx, this);
            stack::call(m, 0x41cc80, &[], Some(this), |m| {
                mapping::attach_path(m, this, path, environment)
            })?;
        }
        set(m, Reg::Eax, this);
        stack::add_sp(m, 4);
        Ok(this)
    })
}

/// Actual raw54d1c0 state uses CRT slot tables and stored260-byte records.
/// exe: 0x54d1c0
pub fn raw_find_next(
    m: &mut Space,
    handle: u32,
    destination: u32,
    directory: u32,
) -> Result<u32, String> {
    if handle >= 32 && !matches!(handle, u32::MAX | 0xffff_fffe) {
        return Err("NativeVmNamespace: unknown raw find-slot identity".into());
    }
    let slot = 0x7663d8u32.wrapping_add(handle.wrapping_mul(4));
    if handle >= 32 && m.r32(slot) != 0 {
        return Err("NativeVmNamespace: failed find sentinel is not empty".into());
    }
    stack::function_cdecl(m, &[handle, destination, directory], |m| {
        set(m, Reg::Edx, handle);
        let records = m.r32(slot);
        set(m, Reg::Ecx, records);
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, slot);
        if records == 0 {
            stack::add_sp(m, 4);
            set(m, Reg::Eax, u32::MAX);
            stack::add_sp(m, 4);
            return Ok(u32::MAX);
        }
        let cursor = 0x7685e0 + handle * 4;
        let index = m.r32(cursor);
        set(m, Reg::Eax, index);
        stack::push_reg(m, Reg::Ebx);
        let total = m.r32(0x768660 + handle * 4);
        set(m, Reg::Ebx, total);
        set(m, Reg::Esi, cursor);
        set(m, Reg::Ebx, stack::load(m, 0));
        stack::add_sp(m, 4);
        if index as i32 >= total as i32 {
            stack::add_sp(m, 4);
            set(m, Reg::Eax, u32::MAX);
            stack::add_sp(m, 4);
            return Ok(u32::MAX);
        }
        let record = records.wrapping_add(index.wrapping_mul(260));
        set(m, Reg::Edx, index.wrapping_mul(65));
        set(m, Reg::Eax, record);
        set(m, Reg::Ecx, destination);
        stack::push(m, record);
        stack::push(m, destination);
        stack::call(m, 0x54d214, &[], None, |m| strcpy(m, destination, record));
        let index = m.r32(cursor);
        set(m, Reg::Eax, index);
        set(m, Reg::Edx, index.wrapping_mul(65));
        let records = m.r32(slot);
        set(m, Reg::Eax, records);
        let flag = m.r32(records + index * 260 + 256);
        set(m, Reg::Ecx, flag);
        set(m, Reg::Edx, directory);
        write(m, directory, flag);
        set(m, Reg::Eax, index);
        stack::add_sp(m, 8);
        set(m, Reg::Eax, index + 1);
        write(m, cursor, index + 1);
        stack::add_sp(m, 4);
        set(m, Reg::Eax, 0);
        stack::add_sp(m, 4);
        Ok(0)
    })
}
/// exe: 0x54d240
pub fn raw_find_close(
    m: &mut Space,
    handle: u32,
    environment: &mut impl Environment,
) -> Result<u32, String> {
    stack::function_cdecl(m, &[handle], |m| {
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, handle);
        if handle >= 32 {
            set(m, Reg::Eax, u32::MAX);
            stack::add_sp(m, 4);
            return Ok(u32::MAX);
        }
        let slot = 0x7663d8 + handle * 4;
        let pointer = m.r32(slot);
        set(m, Reg::Eax, pointer);
        if pointer != 0 {
            stack::push(m, pointer);
            let result = stack::external(m, 0x54d25f, &[], None, |m| environment.free(m, pointer))?;
            set(m, Reg::Eax, result);
            stack::add_sp(m, 4);
        }
        write(m, slot, 0);
        write(m, 0x768660 + handle * 4, 0);
        write(m, 0x7685e0 + handle * 4, 0);
        set(m, Reg::Eax, 0);
        stack::add_sp(m, 4);
        Ok(0)
    })
}

pub fn raw_find_open(
    m: &mut Space,
    pattern: u32,
    destination: u32,
    directory: u32,
    environment: &mut impl Environment,
) -> Result<u32, String> {
    find::open(m, pattern, destination, directory, environment)
}

pub fn map(
    m: &mut Space,
    root: u32,
    path: u32,
    name: u32,
    environment: &mut impl Environment,
) -> Result<u32, String> {
    mapping::map(m, root, path, name, environment)
}

#[cfg(test)]
#[path = "native_vm_namespace_tests.rs"]
mod tests;

#[path = "native_vm_namespace_resolution.rs"]
mod resolution;
pub use resolution::{
    begin_class_lookup, begin_class_resolution, ClassLookup, PendingClassResolution,
};

#[path = "native_vm_class_cache.rs"]
mod cache;
#[cfg(test)]
pub use cache::FileLeaf;
pub use cache::{prepare_class_cache, read_class_input, FileEnvironment};

#[path = "native_vm_source_loader.rs"]
mod source_loader;
pub use source_loader::{
    begin_module_load, begin_next_module, deserialize_constants, deserialize_members,
    finish_class_load, finish_module_tree,
};
