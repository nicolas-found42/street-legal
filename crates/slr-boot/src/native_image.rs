//! Original image data used by address-preserving native ports. Only the
//! initialized .rdata/.data bytes are loaded; x86 instructions are never run.
use slr_physics::space::Space;

pub struct ImageData(Vec<(u32, Vec<u8>)>);
impl ImageData {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let word = |offset: usize| -> Result<u32, String> {
            let data = bytes
                .get(offset..offset.checked_add(4).ok_or("PE offset overflow")?)
                .ok_or("truncated PE word")?;
            Ok(u32::from_le_bytes(data.try_into().unwrap()))
        };
        let short = |offset: usize| -> Result<u16, String> {
            let data = bytes
                .get(offset..offset.checked_add(2).ok_or("PE offset overflow")?)
                .ok_or("truncated PE short")?;
            Ok(u16::from_le_bytes(data.try_into().unwrap()))
        };
        if bytes.get(..2) != Some(b"MZ") {
            return Err("missing DOS header".into());
        }
        let pe = word(0x3c)? as usize;
        if bytes.get(pe..pe.checked_add(4).ok_or("PE offset overflow")?) != Some(b"PE\0\0")
            || short(pe + 4)? != 0x14c
            || short(pe + 24)? != 0x10b
            || word(pe + 52)? != 0x400000
        {
            return Err("native image data requires build-606 i386 PE at 0x400000".into());
        }
        let table = pe + 24 + short(pe + 20)? as usize;
        let count = short(pe + 6)? as usize;
        let mut sections = vec![];
        let mut names = std::collections::BTreeSet::new();
        for i in 0..count {
            let at = table + 40 * i;
            let header = bytes.get(at..at + 40).ok_or("truncated PE section table")?;
            if !matches!(&header[..8], b".rdata\0\0" | b".data\0\0\0") {
                continue;
            }
            if !names.insert(header[..8].to_vec()) {
                return Err("duplicate native image data section".into());
            }
            if word(at + 36)? & 0x20000000 != 0 {
                return Err("executable native data section".into());
            }
            let size = word(at + 8)?.min(word(at + 16)?) as usize;
            let start = word(at + 20)? as usize;
            let data = bytes
                .get(start..start.checked_add(size).ok_or("PE data overflow")?)
                .ok_or("truncated native image data")?;
            let address = 0x400000u32
                .checked_add(word(at + 12)?)
                .ok_or("PE address overflow")?;
            address
                .checked_add(size as u32)
                .ok_or("PE virtual data overflow")?;
            sections.push((address, data.to_vec()));
        }
        if sections.len() != 2 {
            return Err("missing or duplicate native image data sections".into());
        }
        Ok(Self(sections))
    }
    pub fn initialize(&self, memory: &mut Space) {
        for (address, bytes) in &self.0 {
            for (index, byte) in bytes.iter().enumerate() {
                memory.w8(*address + index as u32, *byte);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn initializes_declared_data_without_executable_sections_or_padding() {
        let mut pe = vec![0; 512];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        pe[64..68].copy_from_slice(b"PE\0\0");
        pe[68..70].copy_from_slice(&0x14cu16.to_le_bytes());
        pe[70..72].copy_from_slice(&3u16.to_le_bytes());
        pe[84..86].copy_from_slice(&64u16.to_le_bytes());
        pe[88..90].copy_from_slice(&0x10bu16.to_le_bytes());
        pe[116..120].copy_from_slice(&0x400000u32.to_le_bytes());
        for (i, name) in [b".text\0\0\0", b".rdata\0\0", b".data\0\0\0"]
            .iter()
            .enumerate()
        {
            let section = 152 + i * 40;
            pe[section..section + 8].copy_from_slice(*name);
            for (offset, value) in [
                (8, 2),
                (12, 0x1000 + i as u32 * 0x1000),
                (16, 4),
                (20, 400 + i as u32 * 4),
            ] {
                pe[section + offset..section + offset + 4].copy_from_slice(&value.to_le_bytes());
            }
            pe[400 + i * 4..404 + i * 4].copy_from_slice(&[1 + i as u8, 7, 8, 9]);
        }
        let image = ImageData::parse(&pe).unwrap();
        let mut memory = Space::new();
        image.initialize(&mut memory);
        assert_eq!(memory.r32(0x401000), 0);
        assert_eq!(memory.r32(0x402000), 0x702);
        assert_eq!(memory.r32(0x403000), 0x703);
        assert!(ImageData::parse(&pe[..409]).is_err());
        pe[228..232].copy_from_slice(&0x20000000u32.to_le_bytes());
        assert!(ImageData::parse(&pe).is_err());
    }
}
