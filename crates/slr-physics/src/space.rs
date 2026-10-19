//! A flat 32-bit address space for the exact port of the exe's object-graph code.
//!
//! The cfg keyword handlers (M3-09) write into a car-class object, its parameter block and heap
//! nodes through raw offsets and pointers. Keeping those objects at the addresses the original
//! uses (a zeroed bump heap from `0x2000_0000`, the oracle's convention) lets the differential
//! tests compare complete memory images, pointers included, instead of per-field views.

use std::collections::HashMap;

/// First heap address (the Unicorn oracle's heap base).
pub const HEAP_BASE: u32 = 0x2000_0000;
const PAGE: u32 = 4096;

/// Sparse byte-addressable memory plus the bump allocator that stands in for the exe's
/// `malloc` wrapper (`FUN_0054f560`): 16-byte aligned, zeroed, never reused.
#[derive(Clone, Default)]
pub struct Space {
    pages: HashMap<u32, Box<[u8; PAGE as usize]>>,
    heap_ptr: u32,
    /// Word-aligned addresses written since [`Space::begin_journal`] with their value before
    /// the first write.
    journal: Option<HashMap<u32, u32>>,
}

impl Space {
    pub fn new() -> Space {
        Space {
            pages: HashMap::new(),
            heap_ptr: HEAP_BASE,
            journal: None,
        }
    }

    /// Next allocation address (before alignment).
    pub fn heap_ptr(&self) -> u32 {
        self.heap_ptr
    }

    pub fn set_heap_ptr(&mut self, p: u32) {
        self.heap_ptr = p;
    }

    /// `malloc` as the handlers use it: `n == 0` returns NULL, anything else a fresh zeroed
    /// block.
    pub fn malloc(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.alloc(n)
        }
    }

    /// Bump-allocate `n` zeroed bytes, 16-byte aligned.
    pub fn alloc(&mut self, n: u32) -> u32 {
        let p = (self.heap_ptr + 15) & !15;
        self.heap_ptr = p + n;
        p
    }

    fn page_mut(&mut self, a: u32) -> &mut [u8; PAGE as usize] {
        self.pages
            .entry(a & !(PAGE - 1))
            .or_insert_with(|| Box::new([0u8; PAGE as usize]))
    }

    fn note(&mut self, a: u32) {
        if self.journal.is_some() {
            let w = a & !3;
            let old = self.r32_raw(w);
            if let Some(j) = self.journal.as_mut() {
                j.entry(w).or_insert(old);
            }
        }
    }

    fn r32_raw(&self, a: u32) -> u32 {
        u32::from_le_bytes([self.r8(a), self.r8(a + 1), self.r8(a + 2), self.r8(a + 3)])
    }

    pub fn r8(&self, a: u32) -> u8 {
        self.pages
            .get(&(a & !(PAGE - 1)))
            .map_or(0, |p| p[(a & (PAGE - 1)) as usize])
    }

    pub fn w8(&mut self, a: u32, v: u8) {
        self.note(a);
        self.page_mut(a)[(a & (PAGE - 1)) as usize] = v;
    }

    pub fn r32(&self, a: u32) -> u32 {
        self.r32_raw(a)
    }

    pub fn w32(&mut self, a: u32, v: u32) {
        self.note(a);
        if a & 3 != 0 {
            self.note(a + 3);
        }
        for (i, b) in v.to_le_bytes().into_iter().enumerate() {
            self.page_mut(a + i as u32)[((a + i as u32) & (PAGE - 1)) as usize] = b;
        }
    }

    pub fn rf(&self, a: u32) -> f32 {
        f32::from_bits(self.r32(a))
    }

    pub fn wf(&mut self, a: u32, v: f32) {
        self.w32(a, v.to_bits());
    }

    /// Copy `n` bytes (`memcpy`, regions must not overlap).
    pub fn copy(&mut self, dst: u32, src: u32, n: u32) {
        for i in 0..n {
            let b = self.r8(src + i);
            self.w8(dst + i, b);
        }
    }

    /// Start recording which words change (see [`Space::end_journal`]).
    pub fn begin_journal(&mut self) {
        self.journal = Some(HashMap::new());
    }

    /// Words whose value differs from before [`Space::begin_journal`], as `(address, new value)`
    /// sorted by address.
    pub fn end_journal(&mut self) -> Vec<(u32, u32)> {
        let j = self.journal.take().unwrap_or_default();
        let mut out: Vec<(u32, u32)> = j
            .into_iter()
            .filter_map(|(a, old)| {
                let now = self.r32_raw(a);
                (now != old).then_some((a, now))
            })
            .collect();
        out.sort_unstable();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_is_zeroed_aligned_and_journaled() {
        let mut m = Space::new();
        let a = m.alloc(0xa0);
        assert_eq!(a & 15, 0);
        assert!((a..a + 0xa0).all(|b| m.r8(b) == 0));
        m.begin_journal();
        m.w32(a, 1);
        m.w32(a + 4, 2);
        m.w32(a, 1); // write-back of the same value is not a change
        assert_eq!(m.end_journal(), vec![(a, 1), (a + 4, 2)]);
    }
}
