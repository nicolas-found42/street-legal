//! The exe's line reader (`FUN_005511c0` -> `FUN_00554d60`): copies bytes up
//! to and including `'\n'`, or up to a NUL, and reports how many bytes it
//! consumed. A buffer that ends without either terminator is read as if a NUL
//! followed it (the exe reads one byte past the end).

pub(crate) struct Lines<'a> {
    data: &'a [u8],
    /// Consumed bytes; `len + 1` once the virtual NUL after the buffer was read.
    pub pos: usize,
}

impl<'a> Lines<'a> {
    pub fn new(data: &'a [u8], pos: usize) -> Self {
        Self { data, pos }
    }

    /// True while the exe's `consumed < size` loop condition holds.
    pub fn has_more(&self) -> bool {
        self.pos < self.data.len()
    }

    /// Next line including a trailing `'\n'` if present (a NUL terminator is
    /// consumed but not returned). `None` when no bytes remain.
    pub fn next_line(&mut self) -> Option<&'a [u8]> {
        if !self.has_more() {
            return None;
        }
        let start = self.pos;
        for (i, &b) in self.data[start..].iter().enumerate() {
            match b {
                b'\n' => {
                    self.pos = start + i + 1;
                    return Some(&self.data[start..=start + i]);
                }
                0 => {
                    self.pos = start + i + 1;
                    return Some(&self.data[start..start + i]);
                }
                _ => {}
            }
        }
        self.pos = self.data.len() + 1;
        Some(&self.data[start..])
    }
}

/// `FUN_00551120(dst, src, 0x100)`: copy up to 255 bytes, stopping at NUL.
pub(crate) fn cap255(line: &[u8]) -> &[u8] {
    let n = line.iter().take(255).take_while(|&&b| b != 0).count();
    &line[..n]
}

/// Bytes as Latin-1 text (one char per byte; the files are ASCII).
pub(crate) fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}
