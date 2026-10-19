//! Game path → native path conversion (M1-09).
//!
//! Every file-layer entry point first copies the caller's path with `0x554860` (a plain
//! NUL-terminated string copy with no length bound) and then rewrites `'/'` to `'\\'` in the
//! copy before handing it to the CRT (`0x54c880` open-read, `0x54caf0` open-write, `0x54c170`
//! exists, `0x54c550` copy, `0x54c650` move, `0x54c7c0` delete, `0x54cf50` find-first).
//!
//! The copy has no bound in the original (paths longer than the 0x100-byte stack buffers
//! overflow); the port keeps the whole string ([`dos_path`]) and documents that divergence.

use std::path::PathBuf;

/// `0x554860` copy plus the caller's in-place `'/'` → `'\\'` loop.
///
/// exe: 0x554860 (copy), 0x54c8a5 / 0x54cd35 / 0x54c7e5 (conversion loops)
pub fn dos_path(path: &str) -> String {
    path.chars()
        .map(|c| if c == '/' { '\\' } else { c })
        .collect()
}

/// The component walk the file layer's path resolution and `FindFile`'s directory
/// resolution share: a DOS path ([`dos_path`]'s output) as a relative path under a root —
/// the first component's `C:` drive prefix is stripped, `""` and `"."` drop out, and every
/// `".."` or other `':'` component refuses the path (`None`), so nothing escapes the root.
///
/// An empty path yields the empty (root) relative path; [`crate::fileio::FileLayer`] refuses
/// it for a file name, `crate::findfile` accepts it for a search directory.
///
/// exe: 0x554860 (the copy) plus the callers' `'/'` → `'\\'` loop; the root itself is a
/// port policy (see `crate::fileio`'s module docs).
pub fn dos_relative(dos: &str) -> Option<PathBuf> {
    let forward: String = dos
        .chars()
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    let mut rel = PathBuf::new();
    let mut first = true;
    for comp in forward.split('/') {
        if first && comp.len() == 2 && comp.ends_with(':') {
            // `C:` drive prefix: the original resolves it against the process drive.
            first = false;
            continue;
        }
        first = false;
        match comp {
            "" | "." => continue,
            ".." => return None,
            _ if comp.contains(':') => return None,
            _ => rel.push(comp),
        }
    }
    Some(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slashes_become_backslashes() {
        assert_eq!(dos_path("data/cars/gti.car"), "data\\cars\\gti.car");
        assert_eq!(dos_path("data\\cars/gti.car"), "data\\cars\\gti.car");
        assert_eq!(dos_path("/abs/path"), "\\abs\\path");
    }

    #[test]
    fn everything_else_is_copied_verbatim() {
        assert_eq!(dos_path(""), "");
        assert_eq!(dos_path("a b.c-d_e"), "a b.c-d_e");
        assert_eq!(dos_path("ünïcode"), "ünïcode");
    }

    #[test]
    fn relative_components_drop_dots_and_a_drive_prefix() {
        assert_eq!(
            dos_relative("data\\cars\\gti.car"),
            Some(PathBuf::from("data/cars/gti.car"))
        );
        assert_eq!(dos_relative("C:\\data\\x"), Some(PathBuf::from("data/x")));
        assert_eq!(dos_relative("\\.\\data"), Some(PathBuf::from("data")));
        // a leading separator and an empty path name the root itself
        assert_eq!(dos_relative("\\data"), Some(PathBuf::from("data")));
        assert_eq!(dos_relative(""), Some(PathBuf::new()));
        assert_eq!(dos_relative("*.car"), Some(PathBuf::from("*.car")));
    }

    #[test]
    fn relative_components_refuse_escapes() {
        assert_eq!(dos_relative("..\\data"), None);
        assert_eq!(dos_relative("data\\..\\other"), None);
        assert_eq!(dos_relative("data\\C:other"), None);
        assert_eq!(dos_relative("D:data"), None);
    }
}
