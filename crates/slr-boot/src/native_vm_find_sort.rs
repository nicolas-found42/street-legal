//! Actual CRT5d765d quicksort,5d77b1 selection tail and5d77ff byte swap.
//! The only admitted comparator is original54d1a0/554a10 (case folding).
//! This preserves source swaps, saved scratch and caller argument mutations.
use super::*;
fn r(m: &Space, reg: Reg) -> u32 {
    stack::reg(m, reg)
}
fn pop_cx(m: &mut Space) {
    set(m, Reg::Ecx, stack::load(m, 0));
    stack::add_sp(m, 4);
}
fn source_byte(m: &mut Space, destination: u32, register: Reg) {
    let value = r(m, register);
    observe(destination, 1, value);
    m.w8(destination, value as u8);
}
fn swap(m: &mut Space, left: u32, right: u32, size: u32) {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, left);
        set(m, Reg::Ecx, right);
        stack::push_reg(m, Reg::Esi);
        if left == right {
            stack::add_sp(m, 4);
            return;
        }
        set(m, Reg::Edx, size);
        set(m, Reg::Esi, size);
        set(m, Reg::Edx, size.wrapping_sub(1));
        if size == 0 {
            stack::add_sp(m, 4);
            return;
        }
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Esi, size);
        for offset in 0..size {
            low(m, Reg::Ebx, m.r8(right + offset));
            low(m, Reg::Edx, m.r8(left + offset));
            source_byte(m, left + offset, Reg::Ebx);
            set(m, Reg::Eax, left + offset + 1);
            source_byte(m, right + offset, Reg::Edx);
            set(m, Reg::Ecx, right + offset + 1);
            set(m, Reg::Esi, size - offset - 1);
        }
        stack::add_sp(m, 8);
    });
}
fn compare(m: &mut Space, left: u32, right: u32) -> i32 {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, right);
        set(m, Reg::Ecx, left);
        stack::push(m, 1);
        stack::push(m, right);
        stack::push(m, left);
        // The actual54d1a0 CALL returns to54d1b1.
        let answer = stack::call(m, 0x54d1b1, &[], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Edx, left);
                stack::push_reg(m, Reg::Esi);
                set(m, Reg::Esi, right);
                if left == 0 || right == 0 {
                    let result = i32::from(left == right);
                    set(m, Reg::Eax, result as u32);
                    stack::add_sp(m, 4);
                    return result;
                }
                low(m, Reg::Eax, 1);
                let mut offset = 0;
                let result = loop {
                    let mut a = m.r8(left + offset);
                    let mut b = m.r8(right + offset);
                    low(m, Reg::Eax, a);
                    low(m, Reg::Ecx, b);
                    offset += 1;
                    set(m, Reg::Edx, left + offset);
                    set(m, Reg::Esi, right + offset);
                    if a.is_ascii_uppercase() {
                        a += 32;
                        low(m, Reg::Eax, a);
                    }
                    if b.is_ascii_uppercase() {
                        b += 32;
                        low(m, Reg::Ecx, b);
                    }
                    if (a as i8) < (b as i8) {
                        break -1;
                    }
                    if (a as i8) > (b as i8) {
                        break 1;
                    }
                    if a == 0 && b == 0 {
                        break 0;
                    }
                };
                set(m, Reg::Eax, result as u32);
                stack::add_sp(m, 4);
                result
            })
        });
        stack::add_sp(m, 12);
        answer
    })
}
fn comparison(m: &mut Space, return_pc: u32, left: u32, right: u32) -> i32 {
    stack::push(m, right);
    stack::push(m, left);
    let answer = stack::call(m, return_pc, &[], None, |m| compare(m, left, right));
    pop_cx(m);
    pop_cx(m);
    answer
}
fn selection(m: &mut Space, low: u32, high: u32, width: u32) {
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Ebp);
        let fp = stack::sp(m);
        set(m, Reg::Ebp, fp);
        set(m, Reg::Eax, low);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, high);
        if high <= low {
            stack::add_sp(m, 8);
            return;
        }
        set(m, Reg::Ecx, width);
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Eax, low + width);
        stack::push_reg(m, Reg::Esi);
        m.w32(fp + 12, low + width);
        let mut end = high;
        loop {
            let mut maximum = low;
            set(m, Reg::Ebx, low);
            let mut candidate = m.r32(fp + 12);
            set(m, Reg::Esi, candidate);
            while candidate <= end {
                if comparison(m, 0x5d77d8, candidate, maximum) > 0 {
                    maximum = candidate;
                    set(m, Reg::Ebx, maximum);
                }
                candidate += width;
                set(m, Reg::Esi, candidate);
            }
            stack::push(m, width);
            stack::push(m, end);
            stack::push(m, maximum);
            stack::call(m, 0x5d77ef, &[], None, |m| swap(m, maximum, end, width));
            end -= width;
            set(m, Reg::Edi, end);
            stack::add_sp(m, 12);
            if end <= low {
                break;
            }
        }
        stack::add_sp(m, 16);
    });
}
/// Only original54d1a0 case-folded raw-find records are admitted.
/// exe: 0x5d765d
pub(super) fn sort(
    m: &mut Space,
    base: u32,
    count: u32,
    width: u32,
    comparator: u32,
) -> Result<(), String> {
    if comparator != 0x54d1a0 || width != 260 || count > 100000 {
        return Err("NativeVmNamespace: unsupported raw-find CRT qsort input".into());
    }
    stack::function_cdecl(m, &[], |m| {
        stack::push_reg(m, Reg::Ebp);
        let fp = stack::sp(m);
        set(m, Reg::Ebp, fp);
        stack::alloc(m, 0xf8);
        stack::push_reg(m, Reg::Ebx);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, count);
        stack::push_reg(m, Reg::Edi);
        if count < 2 {
            return Ok(());
        }
        m.w32(fp - 4, 0);
        let mut high = (count - 1) * width;
        set(m, Reg::Esi, high);
        set(m, Reg::Ebx, base);
        set(m, Reg::Eax, fp - 0x80);
        m.w32(fp + 8, fp - 0x80);
        set(m, Reg::Eax, fp - 0xf8);
        high += base;
        set(m, Reg::Esi, high);
        m.w32(fp + 12, fp - 0xf8);
        let mut low = base;
        'partitions: loop {
            set(m, Reg::Edi, width);
            let span = high - low;
            set(m, Reg::Eax, span);
            set(m, Reg::Edx, 0);
            let length = span / width + 1;
            set(m, Reg::Edx, span % width);
            set(m, Reg::Eax, length);
            if length <= 8 {
                stack::push(m, comparator);
                stack::push(m, width);
                stack::push(m, high);
                stack::push(m, low);
                stack::call(m, 0x5d76b8, &[], None, |m| selection(m, low, high, width));
                stack::add_sp(m, 16);
            } else {
                let midpoint = low + (length >> 1) * width;
                set(m, Reg::Eax, midpoint);
                stack::push(m, width);
                stack::push(m, low);
                stack::push(m, midpoint);
                stack::call(m, 0x5d76eb, &[], None, |m| swap(m, midpoint, low, width));
                stack::add_sp(m, 12);
                m.w32(fp - 8, low);
                let mut right = high + width;
                set(m, Reg::Edi, right);
                let (left, right) = loop {
                    let mut left = m.r32(fp - 8);
                    loop {
                        left += width;
                        set(m, Reg::Eax, left);
                        m.w32(fp - 8, left);
                        if left > high || comparison(m, 0x5d7705, left, low) > 0 {
                            break;
                        }
                    }
                    loop {
                        right -= width;
                        set(m, Reg::Edi, right);
                        if right <= low || comparison(m, 0x5d7717, right, low) < 0 {
                            break;
                        }
                    }
                    stack::push(m, width);
                    stack::push(m, right);
                    if right >= left {
                        stack::push(m, left);
                        stack::call(m, 0x5d772e, &[], None, |m| swap(m, left, right, width));
                        stack::add_sp(m, 12);
                    } else {
                        stack::push(m, low);
                        stack::call(m, 0x5d7739, &[], None, |m| swap(m, low, right, width));
                        break (left, right);
                    }
                };
                set(m, Reg::Eax, left);
                set(m, Reg::Ecx, right);
                let lhs = right.wrapping_sub(low).wrapping_sub(1);
                set(m, Reg::Ecx, lhs);
                set(m, Reg::Edx, high);
                stack::add_sp(m, 12);
                let rhs = high.wrapping_sub(left);
                set(m, Reg::Edx, rhs);
                if (lhs as i32) >= (rhs as i32) {
                    set(m, Reg::Ecx, width);
                    set(m, Reg::Edx, low + width);
                    if low + width < right {
                        let lows = m.r32(fp + 12);
                        set(m, Reg::Edx, lows);
                        m.w32(fp + 12, lows + 4);
                        let upper = right - width;
                        set(m, Reg::Edi, upper);
                        let highs = m.r32(fp + 8);
                        set(m, Reg::Ecx, highs);
                        m.w32(fp - 4, m.r32(fp - 4) + 1);
                        m.w32(fp + 8, highs + 4);
                        m.w32(lows, low);
                        m.w32(highs, upper);
                    }
                    if left < high {
                        low = left;
                        set(m, Reg::Ebx, low);
                        continue 'partitions;
                    }
                } else {
                    if left < high {
                        let lows = m.r32(fp + 12);
                        set(m, Reg::Ecx, lows);
                        m.w32(fp - 4, m.r32(fp - 4) + 1);
                        m.w32(fp + 12, lows + 4);
                        m.w32(lows, left);
                        let highs = m.r32(fp + 8);
                        set(m, Reg::Eax, highs);
                        m.w32(fp + 8, highs + 4);
                        m.w32(highs, high);
                    }
                    set(m, Reg::Eax, width + low);
                    if width + low < right {
                        high = right - width;
                        set(m, Reg::Edi, high);
                        set(m, Reg::Esi, high);
                        continue 'partitions;
                    }
                }
            }
            let pending = m.r32(fp - 4).wrapping_sub(1);
            m.w32(fp - 4, pending);
            m.w32(fp + 12, m.r32(fp + 12) - 4);
            m.w32(fp + 8, m.r32(fp + 8) - 4);
            if (pending as i32) < 0 {
                break;
            }
            let lows = m.r32(fp + 12);
            set(m, Reg::Eax, lows);
            low = m.r32(lows);
            set(m, Reg::Ebx, low);
            let highs = m.r32(fp + 8);
            set(m, Reg::Eax, highs);
            high = m.r32(highs);
            set(m, Reg::Esi, high);
        }
        let _ = (r(m, Reg::Eax), fp);
        Ok(())
    })
}

#[cfg(test)]
#[path = "native_vm_find_sort_tests.rs"]
mod tests;
