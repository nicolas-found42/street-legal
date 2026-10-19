//! `.bon` (bind pose) and `.skl` (bind pose with joint limits): the bone
//! table of a skinned human model, or of a two-bone part mesh
//! (`shock_absorber.bon`, `spring.bon`). Both extensions share one text
//! format and one exe reader, `FUN_00531e90` (`thiscall(ECX = 0x18-byte
//! skeleton, buf, size)`, `ret 8`; reached from the `skeleton %s` line of a
//! model descriptor, `FUN_00549720`; the extension is part of the script
//! string, not a literal in the exe).
//!
//! ```text
//! file  := bone*                          (until the buffer is consumed)
//! bone  := name '\r\n'                    (max 255 bytes)
//!          parent '\r\n'                  (%d, -1 = root)
//!          row0 '\r\n' row1 '\r\n' row2   (%f,%f,%f each: 3x3 rotation)
//!          translation '\r\n'             (%f,%f,%f, stored x 0.1)
//!          lim_x '\r\n' lim_y '\r\n' lim_z   (%f, %f: min, max radians)
//! ```
//!
//! Bones are listed in file order, `parent` indexes that order (the parent
//! may follow its children: `vin.bon` lists `Bip01 Spine` first with parent
//! 3). The `.bon` files carry `-pi..pi` on every axis; the `.skl` files carry
//! the animator's joint limits (a min may exceed its max, e.g. `2.687807,
//! -2.897247`, stored verbatim; the exe does not order them). The two
//! files of a model also differ slightly in bind pose and parent table.
//!
//! The exe's per-bone record is `0xe4` bytes: `+0` parent, `+4` the `next`
//! link below, `+8` the local 4x4 matrix (rows 0..2 as written with column 3
//! zero; row 3 = translation x 0.1 with w = 1), `+0x48` the name, `+0x4c`
//! three minima then `+0x58` three maxima. When the three limit lines do not
//! all scan to two numbers (`count != 6`) the exe stores `-pi`/`+pi`
//! (`0xc0490fdb`/`0x40490fdb`) on every axis. World/inverse matrices at
//! `+0x64` onward (`FUN_0054f3d0`, matrix products) are derived state that
//! this crate does not compute.
//!
//! `next` and `head` are the exe's parent-before-child thread built by
//! `FUN_00530cd0` and walked by its world-matrix pass.

use crate::lines::{cap255, latin1, Lines};
use crate::scan::{expect, scan_float, scan_int, skip_space};
use slr_fmt_core::{FormatError, Result};
use slr_x87::Precision;

/// exe: `DAT_005f08e8` (`0.1f`), the translation scale.
pub const TRANSLATION_SCALE: f32 = 0.1;
/// exe: the `0xc0490fdb` / `0x40490fdb` limit defaults.
pub const DEFAULT_LIMIT: f32 = std::f32::consts::PI;

/// One bone of a bind pose.
#[derive(Debug, Clone, PartialEq)]
pub struct Bone {
    pub name: String,
    /// Index of the parent bone in file order; negative for a root.
    pub parent: i32,
    /// Next bone in the parent-before-child thread, `-1` at the end.
    pub next: i32,
    /// Row-major local matrix: rotation rows, then translation x 0.1, w = 1.
    pub matrix: [f32; 16],
    /// Joint minima per axis (radians).
    pub min: [f32; 3],
    /// Joint maxima per axis (radians).
    pub max: [f32; 3],
    /// False when the exe fell back to `-pi..pi` because a limit line did
    /// not scan to two numbers.
    pub limits_explicit: bool,
}

/// A parsed `.bon` / `.skl` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    pub bones: Vec<Bone>,
    /// First bone of the thread (`-1` when the file is empty).
    pub head: i32,
}

impl Skeleton {
    /// Bone indices in the exe's parent-before-child order, from `head`.
    pub fn traversal(&self) -> impl Iterator<Item = usize> + '_ {
        let mut cur = self.head;
        std::iter::from_fn(move || {
            let i = usize::try_from(cur).ok()?;
            cur = self.bones[i].next;
            Some(i)
        })
    }

    /// Bone index by exact name.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }
}

fn bad(what: &'static str, detail: impl Into<String>) -> FormatError {
    FormatError::Inconsistent {
        what,
        detail: detail.into(),
    }
}

/// `"%f,%f,%f"`.
fn scan_row(l: &[u8], pc: Precision) -> Option<[f32; 3]> {
    let mut p = 0;
    let x = scan_float(l, &mut p, pc)?;
    expect(l, &mut p, b',').then_some(())?;
    let y = scan_float(l, &mut p, pc)?;
    expect(l, &mut p, b',').then_some(())?;
    let z = scan_float(l, &mut p, pc)?;
    Some([x, y, z])
}

/// `"%f, %f"` (the space skips input whitespace before the second number).
fn scan_limit(l: &[u8], pc: Precision) -> Option<[f32; 2]> {
    let mut p = 0;
    let lo = scan_float(l, &mut p, pc)?;
    expect(l, &mut p, b',').then_some(())?;
    skip_space(l, &mut p);
    let hi = scan_float(l, &mut p, pc)?;
    Some([lo, hi])
}

/// exe: FUN_00530cd0
///
/// Builds the parent-before-child thread; returns the new head.
fn link(bones: &mut [Bone]) -> Result<i32> {
    for b in bones.iter_mut() {
        b.next = -1;
    }
    let n = bones.len();
    let mut head = -1;
    for i in 0..n {
        let mut tail = i;
        let mut cur = bones[i].next;
        let mut steps = 0;
        while let Ok(c) = usize::try_from(cur) {
            tail = c;
            cur = bones[c].next;
            steps += 1;
            if steps > n {
                return Err(bad("skeleton", "cyclic parent table"));
            }
        }
        match usize::try_from(bones[i].parent) {
            Err(_) => {
                bones[tail].next = head;
                head = i as i32;
            }
            Ok(p) => {
                bones[tail].next = bones[p].next;
                bones[p].next = i as i32;
            }
        }
    }
    Ok(head)
}

/// Parse a `.bon` or `.skl` file at the production precision (x87 PC=24).
/// Delegates to [`parse_skeleton_with`].
pub fn parse_skeleton(data: &[u8]) -> Result<Skeleton> {
    parse_skeleton_with(data, Precision::F32)
}

/// exe: FUN_00531e90
///
/// As [`parse_skeleton`] with an explicit x87 precision control for the
/// scanner (the game runs at 24 bits, ADR 0007; the oracle vectors cover
/// 24 and 53).
pub fn parse_skeleton_with(data: &[u8], pc: Precision) -> Result<Skeleton> {
    let mut lines = Lines::new(data, 0);
    let mut bones: Vec<Bone> = Vec::new();
    while lines.has_more() {
        let idx = bones.len();
        let mut line = |what: &'static str| {
            lines
                .next_line()
                .ok_or_else(|| bad("skeleton", format!("bone {idx}: missing {what} line")))
        };
        let raw = cap255(line("name")?);
        let name = raw
            .len()
            .checked_sub(2)
            .map(|n| latin1(&raw[..n]))
            .ok_or_else(|| bad("skeleton", format!("bone {idx}: name line shorter than 2")))?;
        let parent_line = line("parent")?;
        let parent = scan_int(parent_line, &mut 0)
            .ok_or_else(|| bad("skeleton", format!("bone {idx}: bad parent index")))?;
        let mut matrix = [0.0f32; 16];
        for r in 0..4 {
            let l = line("matrix")?;
            let v = scan_row(l, pc)
                .ok_or_else(|| bad("skeleton", format!("bone {idx}: bad matrix row {r}")))?;
            for c in 0..3 {
                matrix[r * 4 + c] = if r == 3 {
                    v[c] * TRANSLATION_SCALE
                } else {
                    v[c]
                };
            }
        }
        matrix[15] = 1.0;
        let lim = [line("limit")?, line("limit")?, line("limit")?].map(|l| scan_limit(l, pc));
        let (min, max, limits_explicit) = match lim {
            [Some(x), Some(y), Some(z)] => ([x[0], y[0], z[0]], [x[1], y[1], z[1]], true),
            _ => ([-DEFAULT_LIMIT; 3], [DEFAULT_LIMIT; 3], false),
        };
        bones.push(Bone {
            name,
            parent,
            next: -1,
            matrix,
            min,
            max,
            limits_explicit,
        });
    }
    let n = bones.len();
    if let Some(b) = bones
        .iter()
        .position(|b| usize::try_from(b.parent).is_ok_and(|p| p >= n))
    {
        return Err(bad(
            "skeleton",
            format!(
                "bone {b}: parent {} out of range ({n} bones)",
                bones[b].parent
            ),
        ));
    }
    let head = link(&mut bones)?;
    Ok(Skeleton { bones, head })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bone(name: &str, parent: i32, tr: &str, limits: [&str; 3]) -> String {
        format!(
            "{name}\r\n{parent}\r\n1.000000, 0.000000, 0.000000\r\n\
             0.000000, 1.000000, 0.000000\r\n0.000000, 0.000000, 1.000000\r\n\
             {tr}\r\n{}\r\n{}\r\n{}\r\n",
            limits[0], limits[1], limits[2]
        )
    }
    const FULL: [&str; 3] = ["-3.141593, 3.141593"; 3];

    fn parse(text: &str) -> Skeleton {
        parse_skeleton(text.as_bytes()).unwrap()
    }

    #[test]
    fn bone_fields_and_translation_scale() {
        let s = parse(&bone("Bip01 R Thigh", -1, "0.1, 0.3, 123456.789012", FULL));
        let b = &s.bones[0];
        assert_eq!(b.name, "Bip01 R Thigh");
        assert_eq!(b.parent, -1);
        // Rows 0..2 as written, column 3 zero; row 3 is the translation
        // times 0.1f with w = 1 (oracle bit patterns).
        let bits: Vec<u32> = b.matrix.iter().map(|f| f.to_bits()).collect();
        assert_eq!(
            bits,
            [
                0x3f800000, 0, 0, 0, 0, 0x3f800000, 0, 0, 0, 0, 0x3f800000, 0, 0x3c23d70b,
                0x3cf5c290, 0x4640e6b7, 0x3f800000
            ]
        );
        assert!(b.limits_explicit);
        // The exe scanner is not correctly rounded: "3.141593" scans to
        // 0x40490fdb (f32::from_str gives 0x40490fdc).
        assert_eq!(b.min.map(f32::to_bits), [0xc0490fdb; 3]);
        assert_eq!(b.max.map(f32::to_bits), [0x40490fdb; 3]);
    }

    #[test]
    fn limits_are_min_then_max_per_axis() {
        let s = parse(&bone(
            "a",
            -1,
            "0, 0, 0",
            ["-0.261799, 0.5", "-0.785398, 0.25", "2.687807, -2.897247"],
        ));
        // Oracle bit patterns: the exe's x87 accumulation lands one ulp
        // below the correctly rounded value on these decimals.
        let b = &s.bones[0];
        assert_eq!(
            b.min.map(f32::to_bits),
            [0xbe860a84, 0xbf490fd7, 0x402c0506]
        );
        assert_eq!(
            b.max.map(f32::to_bits),
            [0x3f000000, 0x3e800000, 0xc0396c7e]
        );
    }

    #[test]
    fn short_limit_line_falls_back_to_pi_for_all_axes() {
        let s = parse(&bone("a", -1, "0, 0, 0", ["-1, 1", "-0.5", "-1, 1"]));
        let b = &s.bones[0];
        assert!(!b.limits_explicit);
        assert_eq!(b.min, [-std::f32::consts::PI; 3]);
        assert_eq!(b.max, [std::f32::consts::PI; 3]);
    }

    #[test]
    fn links_thread_parents_before_children() {
        // Oracle (Unicorn, 0x531e90) `next`/head for these inputs.
        let chain = parse(&format!(
            "{}{}{}",
            bone("a", -1, "0, 0, 0", FULL),
            bone("b", 0, "0, 0, 0", FULL),
            bone("c", 1, "0, 0, 0", FULL)
        ));
        assert_eq!(chain.head, 0);
        assert_eq!(
            chain.bones.iter().map(|b| b.next).collect::<Vec<_>>(),
            [1, 2, -1]
        );
        let fwd = parse(&format!(
            "{}{}{}",
            bone("leaf", 2, "0, 0, 0", FULL),
            bone("mid", 2, "0, 0, 0", FULL),
            bone("root", -1, "0, 0, 0", FULL)
        ));
        assert_eq!(fwd.head, 2);
        assert_eq!(
            fwd.bones.iter().map(|b| b.next).collect::<Vec<_>>(),
            [-1, 0, 1]
        );
        assert_eq!(fwd.traversal().collect::<Vec<_>>(), [2, 1, 0]);
        let roots = parse(&format!(
            "{}{}",
            bone("bone00", -1, "0, 0, 0", FULL),
            bone("bone01", -1, "0, 0, 0", FULL)
        ));
        assert_eq!(roots.head, 1);
        assert_eq!(
            roots.bones.iter().map(|b| b.next).collect::<Vec<_>>(),
            [-1, 0]
        );
    }

    #[test]
    fn name_loses_two_bytes_so_lf_only_files_lose_a_character() {
        let lf = bone("abcdef", -1, "0, 0, 0", FULL).replace("\r\n", "\n");
        assert_eq!(parse(&lf).bones[0].name, "abcde");
    }

    #[test]
    fn name_is_capped_at_255_before_the_two_byte_chop() {
        let long = "n".repeat(300);
        assert_eq!(
            parse(&bone(&long, -1, "0, 0, 0", FULL)).bones[0].name.len(),
            253
        );
    }

    #[test]
    fn rejects_unparseable_and_out_of_range_input() {
        let ok = bone("a", -1, "0, 0, 0", FULL);
        assert!(parse_skeleton(&ok.as_bytes()[..ok.len() - 30]).is_err());
        assert!(parse_skeleton(bone("a", 5, "0, 0, 0", FULL).as_bytes()).is_err());
        assert!(parse_skeleton(bone("a", -1, "0; 0; 0", FULL).as_bytes()).is_err());
        assert!(parse_skeleton(b"").unwrap().bones.is_empty());
    }
}
