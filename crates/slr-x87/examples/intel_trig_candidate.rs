//! Emit diagnostic candidate values for independent oracle comparison.
//! This executable supplies no production float trust or acceptance status.
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.expect("input line");
        let bits = u32::from_str_radix(line.trim().trim_start_matches("0x"), 16).expect("f32 word");
        match slr_x87::intel_trig::candidate_f32(bits) {
            Ok((sin, cos, c2)) => println!(
                "{bits:08x} {} {} {}",
                hex(sin.to_bytes()),
                hex(cos.to_bytes()),
                u8::from(c2)
            ),
            Err(error) => println!("{bits:08x} unsupported {error}"),
        }
    }
}
fn hex(bytes: [u8; 10]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
