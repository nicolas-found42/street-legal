//! Stream the shipped `slr-x87` operations (approx transcendentals, `fscale`, `frndint`) over the G11 adjudication corpus.
//!
//! Same record formats as `re/x87probe/g11/bochs_stream.cc`: 21-byte input records
//! (op, a[10], b[10]), 22-byte output records (r0[10], r1[10], sw16). Ops 6..7 and `fyl2xp1`
//! are not implemented by the crate and are written as all-0xFF records so a comparison
//! counts them as mismatches instead of silently skipping them.
use slr_x87::trans::{approx_cos, approx_f2xm1, approx_fyl2x, approx_patan, approx_sin};
use slr_x87::{Rounding, X87};
use std::io::{Read, Write};

fn main() {
    // optional x87 control word (hex) as argv[1]; only RC matters, for ops 8 and 9
    let cw = std::env::args()
        .nth(1)
        .map_or(0x7f, |a| u32::from_str_radix(&a, 16).expect("hex cw"));
    let rc = match (cw >> 10) & 3 {
        0 => Rounding::NearestEven,
        1 => Rounding::Down,
        2 => Rounding::Up,
        _ => Rounding::Zero,
    };
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).expect("stdin");
    let (records, rest) = input.as_chunks::<21>();
    assert!(rest.is_empty(), "corpus is whole 21-byte records");
    let mut out = Vec::with_capacity(records.len() * 22);
    for rec in records {
        let a = X87::from_bytes(rec[1..11].try_into().unwrap());
        let b = X87::from_bytes(rec[11..21].try_into().unwrap());
        let r0 = match rec[0] {
            0 => Some(approx_sin(a).value),
            1 => Some(approx_cos(a).value),
            2 => Some(approx_patan(b, a).value), // y = b, x = a
            3 => Some(approx_f2xm1(a).value),
            4 => Some(approx_fyl2x(a, b).value), // x = a, y = b
            8 => Some(a.fscale(b, rc)),
            9 => Some(a.frndint(rc)),
            _ => None,
        };
        match r0 {
            Some(v) => {
                out.extend_from_slice(&v.to_bytes());
                out.extend_from_slice(&[0u8; 12]);
            }
            None => out.extend_from_slice(&[0xFF; 22]),
        }
    }
    std::io::stdout().write_all(&out).expect("stdout");
}
