//! Ground material tables (`*.scm`): index-to-surface-name plus physics tuning.
//!
//! Text files, one directive per line (`\r\n` endings; `;`, `#` and `/`
//! start a comment — the exe truncates the line at the first of those bytes
//! in `FUN_004b6340`):
//!
//! ```text
//! materials <N>
//! material <i> <name...>        (name is `%31s`: truncated at whitespace)
//! friction <f>                 (applies to the current material)
//! rollres <f>
//! collcoeff <f>
//! skid_PS <hex> / skid_SM <hex> / skid_SMcolor <hex> / skid_SFX <hex>
//! params <name>
//! eof <...>
//! ```
//!
//! `materials N` declares the table size; each `material i name` line stores
//! the name into entry `i` (32-byte field, 0x24-byte entries) and marks it
//! current: the exe clears every entry's present flag first, then sets only
//! entry `i`, so each parameter line that follows applies to the most
//! recently defined material. Stock files use only `materials` + `material`
//! (108 + 75 entries); modded addon maps add `friction` / `rollres` /
//! `collcoeff` / `skid_*` tuning plus `//` comments.
//!
//! The names index the `.scy` surface table 1:1: stock `city.scm` names equal
//! `city.SCY` group names in order (108/108), same for `city2` (75/75).
//! A `#` comment inside a name truncates the stored value (`Material #7678`
//! is stored as `Material`; `1 - Default` as `1`), because comment-stripping
//! runs before the `%d %31s` scan.
//!
//! The reader is the ground-body init `FUN_00463220` (Ghidra
//! `re/out/all_pseudo.c`): the `groundbody` cfg line
//! (`"%x,%f,%f,%f,%f,%f,%f,%s,%s"`, `.rdata 0x00610f10`) carries the `.scm`
//! path; the file is loaded by `FUN_0054c300`, split into lines
//! (`FUN_005511c0`) and dispatched per line by `FUN_004b6340` through the
//! keyword table at `.data 0x00610db8` — `(keyword, handler)` pairs
//! `(materials, 0x4627f0)`, `(material, 0x4629c0)`, `(params, 0x462a70)`,
//! `(friction, 0x462b00)`, `(rollres, 0x462b80)`, `(collcoeff, 0x462bf0)`,
//! `(skid_PS, 0x462c60)`, `(skid_SM, 0x462e00)`, `(skid_SMcolor, …)`,
//! `(skid_SFX, 0x462fa0)`, `(eof, 0x463020)` — each handler skipping its
//! keyword and scanning the remainder with `FUN_00551240`:
//!
//! ```text
//! materials: "%d"            (.rdata 0x00610e10; allocates 0x48/0x24/0xac tables)
//! material:  "%d %31s"       (.rdata 0x00610e40; 32-byte name + present flag)
//! params:    "%31s"          (.rdata 0x00610e48)
//! friction:  "%f"            (.rdata 0x00610e50; +0x50 of the 0xac row, × ground_friction)
//! rollres:   "%f"            (.rdata 0x00610e54; +0x54)
//! collcoeff: "%f"            (.rdata 0x00610e58; +0x58)
//! skid_PS:   "%x"            (.rdata 0x00610e5c)
//! skid_SM:   "%x"            (.rdata 0x00610e70)
//! skid_SFX:  "%x %f"         (.rdata 0x00610e84/0x610e88; eof form)
//! ```
//!
//! Handler behaviour was differential-tested under the Unicorn oracle
//! (`FUN_00551240` with the exact `.rdata` formats on stock/modded lines,
//! and the `material` handler `0x4629c0` storing `19 → "1"`,
//! `74 → "Material"` with the present flag set).

use slr_fmt_core::{FormatError, Result};

/// Maximum name bytes the exe stores per entry (`%31s` + NUL into a 32-byte
/// field). exe: `material` handler `0x4629c0` (`0x610e40`).
pub const MAX_NAME: usize = 31;

/// One `material <i> <name>` entry: the stored (truncated) name.
/// exe: `material` handler `0x4629c0` writes the `%31s` token into the
/// 0x24-byte entry and sets its present flag (entry `i` is then "current").
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialEntry {
    pub index: u32,
    /// Name as stored by the exe: first whitespace-delimited token after the
    /// index, after `;`/`#`/`/` comment truncation, max 31 bytes.
    pub name: String,
}

/// A parameter line attached to the most recently defined material.
/// exe: the `friction`/`rollres`/`collcoeff`/`skid_*` handlers loop the
/// 0x24-byte entries and store into the 0xac row of entries whose present
/// flag is set (i.e. the current one: `material` clears all flags first).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Keyword (`friction`, `rollres`, `collcoeff`, `skid_PS`, `skid_SM`,
    /// `skid_SMcolor`, `skid_SFX`, `params`, `eof`).
    pub keyword: String,
    /// Raw argument text (float, hex, or name, per keyword).
    pub args: Vec<String>,
    /// Index of the material this line applied to.
    pub material: u32,
}

/// A parsed `.scm` ground table.
/// exe: the `.data 0x00610db8` keyword dispatch over the file
/// (`FUN_004b6480` line loop via `FUN_004b6340`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ground {
    /// Declared entry count (`materials N`).
    /// exe: `materials` handler `0x4627f0` (allocates the tables).
    pub count: u32,
    /// `material` entries in file order.
    pub materials: Vec<MaterialEntry>,
    /// Parameter lines (`friction` et al.) in file order.
    pub params: Vec<Param>,
}

/// Keywords the exe dispatches (`FUN_004b6340` table at `0x00610db8`);
/// anything else is a foreign line. `//` comment lines never reach the
/// dispatcher (truncated to empty by the `/` rule first).
const KEYWORDS: &[&str] = &[
    "materials",
    "material",
    "params",
    "friction",
    "rollres",
    "collcoeff",
    "skid_PS",
    "skid_SM",
    "skid_SMcolor",
    "skid_SFX",
    "eof",
];

/// Truncate a line the way `FUN_004b6340` does: cut at the first `;`, `#`
/// or `/` byte (each searched with `strchr` and NUL-patched).
fn strip_comment(line: &str) -> &str {
    let mut end = line.len();
    for b in *b";#/" {
        if let Some(i) = line.bytes().position(|c| c == b) {
            end = end.min(i);
        }
    }
    line[..end].trim()
}

fn parse_u32(token: &str) -> Option<u32> {
    let t = token.trim().strip_prefix('+').unwrap_or(token.trim());
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<u32>().ok()
}

/// Parse one `.scm` text file.
/// exe: `FUN_004b6480` (memory range) / `FUN_004b6520` (file path) line loop
/// with `FUN_004b6340` dispatch; unknown keywords are ignored by the exe,
/// but this parser rejects them so corpus drift surfaces loudly.
pub fn parse(data: &[u8]) -> Result<Ground> {
    // Text only: the binary addon `.SCM` variants (MP1, Physic_Lab, Caynon)
    // are a different format (see module docs); refuse them explicitly.
    if data.contains(&0) {
        return Err(FormatError::Inconsistent {
            what: "scm file",
            detail: "NUL byte: binary .SCM variant, not the text table".to_string(),
        });
    }
    let text = data.iter().map(|&b| b as char).collect::<String>();
    let mut ground = Ground::default();
    let mut declared = false;
    let mut current: Option<u32> = None;
    for (no, raw) in text.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let code = strip_comment(line);
        if code.is_empty() {
            continue;
        }
        let mut words = code.split_whitespace();
        let kw = words.next().unwrap_or("");
        if !KEYWORDS.contains(&kw) {
            return Err(FormatError::Inconsistent {
                what: "scm keyword",
                detail: format!("line {}: {kw:?}", no + 1),
            });
        }
        let rest: Vec<&str> = code[kw.len()..].split_whitespace().collect();
        match kw {
            "materials" => {
                if declared || rest.len() != 1 {
                    return Err(FormatError::Inconsistent {
                        what: "scm materials",
                        detail: format!("line {}", no + 1),
                    });
                }
                ground.count = parse_u32(rest[0]).ok_or(FormatError::Inconsistent {
                    what: "scm materials count",
                    detail: format!("line {}", no + 1),
                })?;
                declared = true;
            }
            "material" => {
                // The exe's `%d %31s` scan takes the index and the next
                // token, ignoring anything after (`1 - Default` stores
                // `1`; `Material #7678` arrives `#`-truncated as
                // `Material`). Require at least both conversions.
                if rest.len() < 2 {
                    return Err(FormatError::Inconsistent {
                        what: "scm material",
                        detail: format!("line {}: {} args", no + 1, rest.len()),
                    });
                }
                let index = parse_u32(rest[0]).ok_or(FormatError::Inconsistent {
                    what: "scm material index",
                    detail: format!("line {}", no + 1),
                })?;
                let mut name = rest[1].to_string();
                name.truncate(MAX_NAME);
                current = Some(index);
                ground.materials.push(MaterialEntry { index, name });
            }
            _ => {
                let Some(cur) = current else {
                    return Err(FormatError::Inconsistent {
                        what: "scm param",
                        detail: format!("line {}: {kw} before any material", no + 1),
                    });
                };
                ground.params.push(Param {
                    keyword: kw.to_string(),
                    args: rest.iter().map(|s| s.to_string()).collect(),
                    material: cur,
                });
            }
        }
    }
    if !declared {
        return Err(FormatError::Inconsistent {
            what: "scm file",
            detail: "missing materials header".to_string(),
        });
    }
    if ground.materials.len() as u32 != ground.count {
        return Err(FormatError::Inconsistent {
            what: "scm count",
            detail: format!(
                "header {} but {} material lines",
                ground.count,
                ground.materials.len()
            ),
        });
    }
    // Indices must be dense `0..N` (the exe sizes the tables by `N` and the
    // `.scy` groups line up positionally).
    for (i, m) in ground.materials.iter().enumerate() {
        if m.index as usize != i {
            return Err(FormatError::Inconsistent {
                what: "scm index",
                detail: format!("entry {} has index {}", i, m.index),
            });
        }
    }
    Ok(ground)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_truncation_matches_exe() {
        // `;`, `#`, `/` all truncate (FUN_004b6340 strchr patching).
        assert_eq!(
            strip_comment("material 74 Material #7678"),
            "material 74 Material"
        );
        assert_eq!(strip_comment("skid_SM 0x1 // comment"), "skid_SM 0x1");
        assert_eq!(strip_comment("a;b"), "a");
        assert_eq!(strip_comment("a/b"), "a");
    }

    #[test]
    fn rejects_binary_variant() {
        assert!(parse(b"materials 1\n\x00").is_err());
    }
}
