//! Persistent x86 native scratch stack. Ported instructions explicitly write
//! their call frames, return PCs, locals and saved registers; unwritten matrix
//! padding consequently comes from the preceding real stores in this memory.
use slr_physics::space::Space;
const META: u32 = 0x7ffd_f000;
const DEPTH: u32 = META;
const PENDING: u32 = META + 4;
const ESP: u32 = META + 8;
const STACK_TOP: u32 = 0x7ff0_0000;
const SENTINEL: u32 = 0x7fff_0000;
#[derive(Clone, Copy)]
pub enum Reg {
    Eax,
    Ebx,
    Ecx,
    Edx,
    Esi,
    Edi,
    Ebp,
}
fn slot(reg: Reg) -> u32 {
    META + 12 + 4 * reg as u32
}
pub fn reg(m: &Space, r: Reg) -> u32 {
    m.r32(slot(r))
}
pub fn set_reg(m: &mut Space, r: Reg, value: u32) {
    m.w32(slot(r), value);
}
pub fn sp(m: &Space) -> u32 {
    m.r32(ESP)
}
pub fn set_sp(m: &mut Space, value: u32) {
    m.w32(ESP, value);
}
pub fn active(m: &Space) -> bool {
    m.r32(DEPTH) != 0
}
pub fn push(m: &mut Space, value: u32) {
    let p = sp(m).wrapping_sub(4);
    set_sp(m, p);
    m.w32(p, value);
}
pub fn push_reg(m: &mut Space, r: Reg) {
    push(m, reg(m, r));
}
pub fn alloc(m: &mut Space, bytes: u32) {
    set_sp(m, sp(m).wrapping_sub(bytes));
}
pub fn add_sp(m: &mut Space, bytes: u32) {
    set_sp(m, sp(m).wrapping_add(bytes));
}
pub fn store(m: &mut Space, offset: i32, value: u32) {
    m.w32(sp(m).wrapping_add_signed(offset), value);
}
pub fn load(m: &Space, offset: i32) -> u32 {
    m.r32(sp(m).wrapping_add_signed(offset))
}
pub fn argument(m: &Space, index: usize) -> u32 {
    load(m, 4 + index as i32 * 4)
}

/// Enter a ported function. A pending modeled CALL supplies its actual frame;
/// direct entry uses the oracle driver's documented stack alignment/registers.
/// This driver resets registers, while previous stack bytes remain intact.
/// `args` are machine words in left-to-right ABI order; ECX carries `this`.
#[derive(Clone, Copy)]
pub struct Frame {
    entry: u32,
    saved: [u32; 4],
    arguments: u32,
}
pub fn enter(m: &mut Space, args: &[u32], this: Option<u32>) -> Frame {
    let nested = m.r32(PENDING) != 0;
    if nested {
        m.w32(PENDING, 0);
    } else {
        assert!(
            !active(m),
            "nested native function requires its actual CALL context"
        );
        for r in [
            Reg::Eax,
            Reg::Ebx,
            Reg::Ecx,
            Reg::Edx,
            Reg::Esi,
            Reg::Edi,
            Reg::Ebp,
        ] {
            set_reg(m, r, 0);
        }
        let p = ((STACK_TOP - 0x100 - args.len() as u32 * 4 - 4) & !15) - 4;
        set_sp(m, p);
        m.w32(p, SENTINEL);
        for (i, value) in args.iter().enumerate() {
            m.w32(p + 4 + i as u32 * 4, *value);
        }
        if let Some(this) = this {
            set_reg(m, Reg::Ecx, this);
        }
    }
    let frame = Frame {
        entry: sp(m),
        saved: [Reg::Ebx, Reg::Esi, Reg::Edi, Reg::Ebp].map(|r| reg(m, r)),
        arguments: args.len() as u32 * 4,
    };
    m.w32(DEPTH, m.r32(DEPTH) + 1);
    frame
}
pub fn leave(m: &mut Space, frame: Frame) {
    for (r, value) in [Reg::Ebx, Reg::Esi, Reg::Edi, Reg::Ebp]
        .into_iter()
        .zip(frame.saved)
    {
        set_reg(m, r, value);
    }
    set_sp(m, frame.entry + 4 + frame.arguments);
    m.w32(DEPTH, m.r32(DEPTH) - 1);
}
pub fn function<R>(
    m: &mut Space,
    args: &[u32],
    this: Option<u32>,
    f: impl FnOnce(&mut Space) -> R,
) -> R {
    let frame = enter(m, args, this);
    let result = f(m);
    leave(m, frame);
    result
}

/// Execute an actual CALL after the caller has formed its argument values.
/// The source-derived return PC is the address following that instruction.
/// Caller cleanup is reflected by restoring its pre-argument ESP after return.
pub fn call<R>(
    m: &mut Space,
    return_pc: u32,
    args: &[u32],
    this: Option<u32>,
    f: impl FnOnce(&mut Space) -> R,
) -> R {
    assert!(active(m), "native CALL requires an entered caller frame");
    let caller = sp(m);
    for value in args.iter().rev() {
        push(m, *value);
    }
    push(m, return_pc);
    if let Some(this) = this {
        set_reg(m, Reg::Ecx, this);
    }
    m.w32(PENDING, 1);
    let result = f(m);
    m.w32(PENDING, 0);
    set_sp(m, caller);
    result
}
/// A controlled external collaborator still consumes its actual CALL frame.
/// Its body is outside the port's modeled boundary, like the allocator oracle.
pub fn external<R>(
    m: &mut Space,
    return_pc: u32,
    args: &[u32],
    this: Option<u32>,
    f: impl FnOnce(&mut Space) -> R,
) -> R {
    call(m, return_pc, args, this, |m| function(m, args, this, f))
}

/// The stack/data-copy part of original54e510/54e3b0. The arithmetic caller
/// supplies its instruction-order determinant and untransposed cofactors.
/// No unwritten word is synthesized:54e3b0's twelve-word copy reads the live
/// three stack gaps between its nine explicitly stored cofactor cells.
pub fn inverse_copy(
    m: &mut Space,
    source: u32,
    destination: u32,
    determinant: u32,
    cofactors: [u32; 9],
) {
    function(m, &[source], Some(destination), |m| {
        push_reg(m, Reg::Esi);
        set_reg(m, Reg::Esi, destination);
        push(m, source);
        store(m, 0xc, determinant);
        call(m, 0x54e55f, &[], Some(destination), |m| {
            function(m, &[source], Some(destination), |m| {
                alloc(m, 0x30);
                push_reg(m, Reg::Esi);
                push_reg(m, Reg::Edi);
                let scratch = sp(m) + 8;
                for (i, value) in cofactors.into_iter().enumerate() {
                    m.w32(scratch + (i / 3) as u32 * 16 + (i % 3) as u32 * 4, value);
                }
                m.copy(destination, scratch, 0x30);
            });
        });
        add_sp(m, 4);
    });
}
