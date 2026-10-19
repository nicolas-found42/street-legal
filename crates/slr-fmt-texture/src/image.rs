//! Decoded-image carrier shared by the DDS/PNG/TGA wrappers.

/// A decoded image: tightly packed RGBA8, row-major, top row first.
///
/// Pixel values are tier D (ADR 0009): they come from third-party decoders
/// (the exe statically links its own libpng/libjpeg and the D3D9 runtime
/// decodes DDS), so only the dimensions and source format are asserted
/// against file headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, R G B A.
    pub pixels: Vec<u8>,
}

impl Rgba8Image {
    pub(crate) fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        debug_assert_eq!(pixels.len(), width as usize * height as usize * 4);
        Self {
            width,
            height,
            pixels,
        }
    }

    /// RGBA of the pixel at `(x, y)` (top-left origin).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}
