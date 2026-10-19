//! `.matrixdata`: keyframed bone animation. exe reader: `FUN_00532e70`
//! (`thiscall(ECX = 0x14-byte object, buf, size)`, `ret 8`), reached from the
//! `sourcefile` line of an animation descriptor (`FUN_00538840`).
//!
//! ```text
//! file  := u32 bones, bones x bone, bones x name-line
//! bone  := u32 key_count, key_count x key
//! key   := f32 time, f32 q[4], f32 d[3]            (32 bytes)
//! name  := bytes up to and including '\n'
//! ```
//!
//! The exe sizes its arrays from the *first* bone's key count and skips every
//! later bone's count word unread, so all bones share `key_count` keys and
//! `times` come from bone 0 only. It stores keys key-major
//! (`(key * bones + bone) * 7` floats), mirrored by [`Animation::keys`].
//! `q` is a unit quaternion on every stock key (checked by the corpus test,
//! `x y z w` order inferred from `w ~ 1` on identity keys); `d` is near unit
//! length on most keys and looks like the bone translation divided by its
//! bind-pose length (compare `.bon` translation direction); that role is an
//! inference from the data, the exe only copies the seven floats. Names are
//! the lines minus their last byte (the `'\n'`), capped at 255 bytes, in bone
//! order, and match the `.bon`/`.skl` bone names (corpus test).

use crate::lines::{cap255, latin1, Lines};
use slr_fmt_core::{FormatError, Result};

/// One key: seven floats as the exe stores them (the time lives in
/// [`Animation::times`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    /// Rotation quaternion `(x, y, z, w)`; unit length on every shipped key.
    pub rotation: [f32; 4],
    /// Three floats after the quaternion (inferred role: translation
    /// relative to the bind-pose bone length).
    pub direction: [f32; 3],
}

/// A parsed `.matrixdata` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub bone_count: usize,
    /// Keys per bone (the first bone's declared count).
    pub key_count: usize,
    /// Key times in seconds, from bone 0.
    pub times: Vec<f32>,
    /// Key-major: index `key * bone_count + bone` (the exe's layout).
    pub keys: Vec<Key>,
    /// The count word each bone declared; the exe only honours the first.
    pub declared_key_counts: Vec<u32>,
    pub bone_names: Vec<String>,
}

impl Animation {
    pub fn key(&self, bone: usize, key: usize) -> &Key {
        &self.keys[key * self.bone_count + bone]
    }
}

fn word(d: &[u8], at: usize) -> Result<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(FormatError::Truncated {
            at,
            need: 4,
            have: d.len().saturating_sub(at),
        })
}

fn float(d: &[u8], at: usize) -> f32 {
    f32::from_bits(u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]]))
}

/// exe: FUN_00532e70
///
/// Parse a `.matrixdata` file.
pub fn parse_animation(d: &[u8]) -> Result<Animation> {
    let bones = word(d, 0)? as usize;
    let mut a = Animation {
        bone_count: bones,
        key_count: 0,
        times: Vec::new(),
        keys: Vec::new(),
        declared_key_counts: Vec::new(),
        bone_names: Vec::new(),
    };
    let mut at = 4;
    if bones > d.len() {
        return Err(FormatError::Truncated {
            at,
            need: bones,
            have: d.len(),
        });
    }
    for b in 0..bones {
        let declared = word(d, at)?;
        at += 4;
        a.declared_key_counts.push(declared);
        if b == 0 {
            a.key_count = declared as usize;
            if a.key_count.saturating_mul(32).saturating_mul(bones) > d.len() {
                return Err(FormatError::Truncated {
                    at,
                    need: a.key_count.saturating_mul(32),
                    have: d.len().saturating_sub(at),
                });
            }
            a.keys = vec![
                Key {
                    rotation: [0.0; 4],
                    direction: [0.0; 3],
                };
                bones * a.key_count
            ];
        }
        for k in 0..a.key_count {
            if at + 32 > d.len() {
                return Err(FormatError::Truncated {
                    at,
                    need: 32,
                    have: d.len().saturating_sub(at),
                });
            }
            if b == 0 {
                a.times.push(float(d, at));
            }
            let f = |i: usize| float(d, at + 4 + 4 * i);
            a.keys[k * bones + b] = Key {
                rotation: [f(0), f(1), f(2), f(3)],
                direction: [f(4), f(5), f(6)],
            };
            at += 32;
        }
    }
    let mut lines = Lines::new(d, at);
    for _ in 0..bones {
        let line = lines.next_line().ok_or(FormatError::Truncated {
            at: d.len(),
            need: 1,
            have: 0,
        })?;
        // exe: NUL written over the line's last byte, then copied capped at 255.
        let name = line.split_last().map_or(&[][..], |(_, rest)| rest);
        a.bone_names.push(latin1(cap255(name)));
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    /// bones x keys with a bone-tagged payload; names LF-terminated.
    fn build(bones: u32, keys: u32, names: &[&str]) -> Vec<u8> {
        let mut d = bones.to_le_bytes().to_vec();
        for b in 0..bones {
            d.extend(keys.to_le_bytes());
            for k in 0..keys {
                let t = k as f32 * 0.5;
                let p = b as f32 * 10.0 + k as f32;
                d.extend(le(&[
                    t,
                    p,
                    p + 0.25,
                    p + 0.5,
                    p + 0.75,
                    p + 0.125,
                    1.0,
                    2.0,
                ]));
            }
        }
        for n in names {
            d.extend(n.as_bytes());
            d.push(b'\n');
        }
        d
    }

    #[test]
    fn parses_header_times_and_key_major_keys() {
        let a = parse_animation(&build(2, 3, &["Bip01 Spine", "Bip01"])).unwrap();
        assert_eq!((a.bone_count, a.key_count), (2, 3));
        assert_eq!(a.times, vec![0.0, 0.5, 1.0]);
        assert_eq!(a.bone_names, vec!["Bip01 Spine", "Bip01"]);
        let k = a.key(1, 2);
        assert_eq!(k.rotation, [12.0, 12.25, 12.5, 12.75]);
        assert_eq!(k.direction, [12.125, 1.0, 2.0]);
        // exe layout: (key * bones + bone), key-major.
        assert_eq!(a.keys[2 * 2 + 1], *k);
    }

    #[test]
    fn zero_bones_is_an_empty_animation() {
        let a = parse_animation(&0u32.to_le_bytes()).unwrap();
        assert_eq!((a.bone_count, a.key_count, a.keys.len()), (0, 0, 0));
    }

    #[test]
    fn only_the_first_key_count_is_used() {
        // exe `FUN_00532e70`: later per-bone counts are skipped unread.
        let mut d = build(2, 1, &["a", "b"]);
        let second = 4 + 4 + 32;
        d[second..second + 4].copy_from_slice(&9u32.to_le_bytes());
        let a = parse_animation(&d).unwrap();
        assert_eq!(a.key_count, 1);
        assert_eq!(a.declared_key_counts, vec![1, 9]);
    }

    #[test]
    fn name_line_loses_its_last_byte_then_caps_at_255() {
        let long = "n".repeat(300);
        let a = parse_animation(&build(1, 1, &[&long])).unwrap();
        assert_eq!(a.bone_names[0].len(), 255);
        let a = parse_animation(&build(1, 1, &["a\r"])).unwrap();
        assert_eq!(a.bone_names[0], "a\r");
    }

    #[test]
    fn truncated_input_is_an_error() {
        let d = build(2, 3, &["a", "b"]);
        assert!(parse_animation(&d[..30]).is_err());
        assert!(parse_animation(&d[..d.len() - 3]).is_err());
        assert!(parse_animation(&[]).is_err());
    }
}
