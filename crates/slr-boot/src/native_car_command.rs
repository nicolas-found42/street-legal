//! Bounded original458c00 native car command dispatch.
use slr_physics::space::Space;

/// Original message16 add_mslot branch. The record retains live configured
/// slot/descriptor pointers and copies the slot's complete transform.
pub fn command(
    m: &mut Space,
    _template: u32,
    object: u32,
    direct: bool,
    _sender: u32,
    message: u32,
    text: &str,
) -> Result<(), String> {
    if !direct {
        return Err(
            "NativeMessageBoundary: indirect car command requires original4265c0 routing".into(),
        );
    }
    if message != 16 {
        return Err(format!("NativeMessageBoundary: car458c00 message{message}"));
    }
    let input = format!("_\0{text}");
    let scan = slr_physics::cfg::scan(input.as_bytes(), "%*s %d %d");
    if text.split_whitespace().next() != Some("add_mslot") {
        return Err(format!("NativeMessageBoundary: car458c00 command{text}"));
    }
    let slot = scan.d(0).unwrap_or(0) as u32;
    let descriptor = scan.d(1).unwrap_or(0) as u32;
    if slot == 0 {
        return Err("NativeMessageBoundary: add_mslot dereferences null slot".into());
    }
    let record = m.malloc(0x54);
    m.w32(record, 0x5f1184);
    for (offset, value) in [(12, slot), (16, descriptor), (20, 0)] {
        m.w32(record + offset, value);
    }
    for axis in 0..3 {
        m.w32(record + 24 + axis * 4, m.r32(slot + 0x2c + axis * 4));
    }
    for word in 0..12 {
        m.w32(record + 0x24 + word * 4, m.r32(slot + 0x38 + word * 4));
    }
    let tail = m.r32(object + 0x20ac);
    if tail == 0 {
        return Err("NativeMessageBoundary: car mslot list lacks sentinel".into());
    }
    m.w32(tail + 4, record);
    m.w32(record + 4, object + 0x20a4);
    m.w32(record + 8, tail);
    m.w32(object + 0x20ac, record);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_add_mslot_preserves_full_records_and_intrusive_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-command-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_car_command_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let k = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let slots = m.alloc(0x68 * 3);
            let descriptors = m.alloc(0x100 * 3);
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            for i in 0..row["count"].as_u64().unwrap() as u32 {
                command(
                    &mut m,
                    k,
                    object,
                    true,
                    0,
                    16,
                    &format!("add_mslot {} {}", slots + i * 0x68, descriptors + i * 0x100),
                )
                .unwrap();
            }
            for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "count={} word{i}",
                    row["count"]
                );
            }
        }
    }
}
